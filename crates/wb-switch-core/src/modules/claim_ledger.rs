//! 每日领取状态记忆（claim ledger）。
//!
//! 统一记录「任务日 × 平台 × 账号 × scope」的领取结果，供账号页 / 平台页派生
//! 「今日已领完 / 还剩 N 项 / 今日无可领取」。此前只有签到日志（只记签到）和豆包
//! 后台的 `Attempt`（只影响自动重试），页面按钮完全不知道今天领没领完。
//!
//! 关键口径（与 `docs/claim-ledger-design.md` 一致）：
//! - 任务日固定用 **Asia/Shanghai**。平台按北京时间切日，daily-runner 内部也用
//!   `beijing_today()`；跨时区的本地日期会让「今天」与平台不一致。
//! - 「已领完」= `claimable == 0`，不是 `claimed == total`。成长任务里有长期锁定、
//!   未达标、未下发的项永远不会变成 claimed，用后者会让按钮整天显示未领完。
//! - 本模块只做记忆，不做判定真相：能查到官方状态的平台仍以官方为准，
//!   查询结果以 `source = "server"` 回写，覆盖同一天同一账号的记忆。
//!
//! 写入方只有后端（手动领取、自动周期、状态查询），前端只读。

use chrono::{FixedOffset, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::modules::config::{atomic_write, now_ms, store_dir};

/// 账号日态保留天数。
pub const KEEP_DAYS: i64 = 30;
/// 任务明细保留天数（更早的条目只留汇总，控制文件体积）。
pub const TASK_DETAIL_DAYS: i64 = 7;
/// 单个条目最多保留的任务明细条数（上游任务表异常放大时兜底）。
const MAX_TASKS_PER_ENTRY: usize = 60;
/// 写入互斥：同进程内并发领取时避免读改写互相覆盖。
static LEDGER_WRITE_LOCK: Mutex<()> = Mutex::new(());

pub const PLATFORM_WORKBUDDY: &str = "workbuddy";
pub const PLATFORM_MINIMAX: &str = "minimax";
pub const PLATFORM_TRAE: &str = "trae";
pub const PLATFORM_QODER: &str = "qoder";
pub const PLATFORM_DOUBAO: &str = "doubao";

pub const SCOPE_CHECKIN: &str = "checkin";
pub const SCOPE_DAILY: &str = "daily";
pub const SCOPE_REWARDS: &str = "rewards";

pub const STATE_CLAIMED: &str = "claimed";
pub const STATE_PARTIAL: &str = "partial";
pub const STATE_BLOCKED: &str = "blocked";
pub const STATE_ERROR: &str = "error";
pub const STATE_UNCONFIRMED: &str = "unconfirmed";
pub const STATE_SKIPPED: &str = "skipped";

pub const SOURCE_MANUAL: &str = "manual";
pub const SOURCE_AUTO: &str = "auto";
pub const SOURCE_SERVER: &str = "server";

/// 任务计数。`blocked` = 锁定 / 未达标 / 未下发，永不计入「未领完」。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Counts {
    pub total: u32,
    pub claimed: u32,
    pub claimable: u32,
    pub blocked: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskState {
    /// 任务展示名（后端已转成中文；未知任务回落到原始 code）。
    pub label: String,
    pub state: String,
}

/// 单次领取结果，供各平台在命令结束时写入。
#[derive(Debug, Clone)]
pub struct Outcome {
    pub state: String,
    pub counts: Counts,
    pub tasks: Vec<TaskState>,
    pub message: String,
}

impl Outcome {
    /// 单任务平台（签到 / 单次奖励）：整条记录只有一个任务项。
    pub fn single(state: &str, message: impl Into<String>) -> Self {
        let (state, counts) = normalize_single(state);
        Self {
            state: state.to_string(),
            counts,
            tasks: Vec::new(),
            message: message.into(),
        }
    }

    /// 多任务平台（WorkBuddy 每日任务）：由 runner 的 done/total/claimable 构造。
    pub fn daily(
        state: &str,
        counts: Counts,
        tasks: Vec<TaskState>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            state: state.to_string(),
            counts,
            tasks,
            message: message.into(),
        }
    }

    /// 跳过（关闭自动签到、窗口外、档位不支持）：记下来但不参与聚合。
    pub fn skipped(message: impl Into<String>) -> Self {
        Self::single(STATE_SKIPPED, message)
    }
}

/// 把平台自己的单任务状态折成 ledger 状态与计数。
pub fn normalize_single(state: &str) -> (&'static str, Counts) {
    let counts = |claimed: u32, claimable: u32, blocked: u32| Counts {
        total: claimed + claimable + blocked,
        claimed,
        claimable,
        blocked,
    };
    match state.trim().to_ascii_lowercase().as_str() {
        "claimed" | "success" | "already" => (STATE_CLAIMED, counts(1, 0, 0)),
        "available" | "ready" => (STATE_PARTIAL, counts(0, 1, 0)),
        // 官方未开放 / 活动未下发 / 企业版 / 账号受限：今天没有可领的东西。
        "closed" | "inactive" | "unsupported" | "unsupported_variant" | "blocked" => {
            (STATE_BLOCKED, counts(0, 0, 1))
        }
        "skipped" => (STATE_SKIPPED, counts(0, 0, 1)),
        "unconfirmed" | "busy" => (STATE_UNCONFIRMED, Counts::default()),
        _ => (STATE_ERROR, Counts::default()),
    }
}

fn shanghai() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap())
}

/// 任务日（Asia/Shanghai）。
pub fn day_of(ts_ms: i64) -> String {
    shanghai()
        .timestamp_millis_opt(ts_ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string())
}

pub fn today() -> String {
    day_of(now_ms())
}

pub fn ledger_file() -> PathBuf {
    store_dir().join("claim_ledger.json")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    day: String,
    platform: String,
    variant: Option<String>,
    account_id: String,
    scope: String,
    state: String,
    counts: Counts,
    tasks: Vec<TaskState>,
    message: String,
    updated_at: i64,
    source: String,
    attempt_count: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Ledger {
    version: u32,
    entries: Vec<Entry>,
}

fn parse_ledger(text: &str) -> Ledger {
    let mut ledger: Ledger = serde_json::from_str(text).unwrap_or_default();
    ledger.version = 1;
    ledger
}

fn read_ledger(path: &Path) -> Ledger {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| parse_ledger(&text))
        .unwrap_or_default()
}

fn write_ledger(path: &Path, ledger: &mut Ledger) -> std::io::Result<()> {
    ledger.version = 1;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(ledger).unwrap_or_else(|_| "{}".into());
    atomic_write(path, &content)
}

fn day_minus(day: &str, days: i64) -> String {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map(|date| (date - chrono::Duration::days(days)).to_string())
        .unwrap_or_else(|_| day.to_string())
}

/// 写入一条记忆。同一天、同平台、同账号、同 scope 覆盖更新，并累加尝试次数。
pub fn record(
    platform: &str,
    variant: Option<&str>,
    account_id: &str,
    scope: &str,
    source: &str,
    outcome: Outcome,
) {
    // 单元测试里的账号是 fixture，绝不能写进用户真实的 `~/.wb-switch`。
    // 需要验证读写的测试直接调 `record_at`（带临时路径）。
    if cfg!(test) {
        return;
    }
    let at = now_ms();
    let day = day_of(at);
    let Ok(_guard) = LEDGER_WRITE_LOCK.lock() else {
        return;
    };
    if let Err(error) = record_at(
        &ledger_file(),
        &day,
        at,
        platform,
        variant,
        account_id,
        scope,
        source,
        outcome,
    ) {
        // 记忆失败不能影响领取结果本身。
        eprintln!("[claim-ledger] 写入失败: {error}");
    }
}

#[allow(clippy::too_many_arguments)]
fn record_at(
    path: &Path,
    day: &str,
    at: i64,
    platform: &str,
    variant: Option<&str>,
    account_id: &str,
    scope: &str,
    source: &str,
    outcome: Outcome,
) -> std::io::Result<()> {
    if platform.trim().is_empty() || account_id.trim().is_empty() || scope.trim().is_empty() {
        return Ok(());
    }
    let mut ledger = read_ledger(path);
    let attempt_count = ledger
        .entries
        .iter()
        .find(|entry| {
            entry.day == day
                && entry.platform == platform
                && entry.account_id == account_id
                && entry.scope == scope
        })
        .map_or(1, |entry| entry.attempt_count.saturating_add(1));
    let tasks: Vec<TaskState> = outcome
        .tasks
        .into_iter()
        .take(MAX_TASKS_PER_ENTRY)
        .collect();
    let entry = Entry {
        day: day.to_string(),
        platform: platform.to_string(),
        variant: variant.map(|value| value.to_string()),
        account_id: account_id.to_string(),
        scope: scope.to_string(),
        state: outcome.state,
        counts: outcome.counts,
        tasks,
        message: outcome.message.chars().take(240).collect(),
        updated_at: at,
        source: source.to_string(),
        attempt_count,
    };
    ledger.entries.retain(|current| {
        !(current.day == entry.day
            && current.platform == entry.platform
            && current.account_id == entry.account_id
            && current.scope == entry.scope)
    });
    ledger.entries.push(entry);
    normalize(&mut ledger, day);
    write_ledger(path, &mut ledger)
}

/// 裁剪：整条 30 天，任务明细 7 天。按 `day` 字符串比较（ISO 日期可直接字典序）。
fn normalize(ledger: &mut Ledger, day: &str) {
    let keep_from = day_minus(day, KEEP_DAYS - 1);
    let detail_from = day_minus(day, TASK_DETAIL_DAYS - 1);
    ledger.entries.retain(|entry| entry.day >= keep_from);
    for entry in &mut ledger.entries {
        if entry.day < detail_from {
            entry.tasks.clear();
        }
    }
    ledger.entries.sort_by(|a, b| {
        a.day
            .cmp(&b.day)
            .then_with(|| a.platform.cmp(&b.platform))
            .then_with(|| a.account_id.cmp(&b.account_id))
            .then_with(|| a.scope.cmp(&b.scope))
    });
}

/// 今日记忆快照。前端据此派生按钮态；不返回 30 天历史，避免无谓传输。
pub fn snapshot() -> Value {
    let day = today();
    let ledger = read_ledger(&ledger_file());
    let entries: Vec<&Entry> = ledger
        .entries
        .iter()
        .filter(|entry| entry.day == day)
        .collect();
    json!({
        "day": day,
        "timezone": "Asia/Shanghai",
        "entries": entries.into_iter().map(|entry| json!({
            "day": entry.day,
            "platform": entry.platform,
            "variant": entry.variant,
            "accountId": entry.account_id,
            "scope": entry.scope,
            "state": entry.state,
            "counts": entry.counts,
            "tasks": entry.tasks,
            "message": entry.message,
            "updatedAt": entry.updated_at,
            "source": entry.source,
            "attemptCount": entry.attempt_count,
        })).collect::<Vec<Value>>(),
    })
}

/// 清除今日记忆（设置页「清除今日领取记录」）。不传 platform / scope 时清空今天全部。
pub fn clear(platform: Option<&str>, scope: Option<&str>) -> std::io::Result<usize> {
    let day = today();
    let Ok(_guard) = LEDGER_WRITE_LOCK.lock() else {
        return Ok(0);
    };
    clear_at(&ledger_file(), &day, platform, scope)
}

fn clear_at(
    path: &Path,
    day: &str,
    platform: Option<&str>,
    scope: Option<&str>,
) -> std::io::Result<usize> {
    let mut ledger = read_ledger(path);
    let before = ledger.entries.len();
    ledger.entries.retain(|entry| {
        entry.day != day
            || platform.is_some_and(|value| entry.platform != value)
            || scope.is_some_and(|value| entry.scope != value)
    });
    let removed = before - ledger.entries.len();
    if removed > 0 {
        write_ledger(path, &mut ledger)?;
    }
    Ok(removed)
}

/// 从 runner 的返回里解析多任务结果。
///
/// 兼容字段缺失（旧 runner）：没有 `claimable` 时退化为「有 remaining 就算未领完」，
/// 属于安全侧偏差——宁可让用户多点一次，也不误报已领完。
pub fn daily_outcome(result: &Value) -> Outcome {
    let number = |key: &str| -> Option<u32> {
        result
            .get(key)
            .and_then(Value::as_u64)
            .map(|value| value.min(u32::MAX as u64) as u32)
    };
    let message = result
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("每日任务已执行")
        .to_string();
    let remaining: Vec<String> = result
        .get("remaining")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(|label| label.chars().take(80).collect())
                .collect()
        })
        .unwrap_or_default();

    if result.get("authInvalid").and_then(Value::as_bool) == Some(true) {
        return Outcome {
            state: STATE_BLOCKED.into(),
            counts: Counts::default(),
            tasks: Vec::new(),
            message,
        };
    }

    let total = number("total").unwrap_or(0);
    let claimed = number("claimed").or_else(|| number("done")).unwrap_or(0);
    if total == 0 {
        // 企业版 / 账号受限 / 活动未开放 → 今天确实没有可领项，标 blocked；
        // 其余（任务表没读到、组件异常）是**可重试的失败**，不能谎报「已领完』。
        let ok = result.get("ok").and_then(Value::as_bool) == Some(true);
        let unavailable = ["企业", "受限", "拒绝", "未开放", "无个人成长"]
            .iter()
            .any(|marker| message.contains(marker));
        return Outcome {
            state: if ok || unavailable {
                STATE_BLOCKED
            } else {
                STATE_ERROR
            }
            .into(),
            counts: Counts::default(),
            tasks: remaining
                .iter()
                .map(|label| TaskState {
                    label: label.clone(),
                    state: STATE_BLOCKED.into(),
                })
                .collect(),
            message,
        };
    }

    let claimable = number("claimable").unwrap_or_else(|| {
        if remaining.is_empty() {
            0
        } else {
            total.saturating_sub(claimed).max(remaining.len() as u32)
        }
    });
    let claimed = claimed.min(total);
    let claimable = claimable.min(total.saturating_sub(claimed));
    let blocked = total.saturating_sub(claimed).saturating_sub(claimable);

    // 明细：remaining 里的 code 默认算「可领未领」，没有 claimable 计数时无法细分。
    let tasks: Vec<TaskState> = remaining
        .iter()
        .map(|label| TaskState {
            label: label.clone(),
            state: if claimable > 0 {
                STATE_PARTIAL.into()
            } else {
                STATE_BLOCKED.into()
            },
        })
        .collect();
    let counts = Counts {
        total,
        claimed,
        claimable,
        blocked,
    };
    let state = if claimable > 0 {
        STATE_PARTIAL
    } else {
        STATE_CLAIMED
    };
    Outcome {
        state: state.into(),
        counts,
        tasks,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        std::env::temp_dir()
            .join(format!("wb-claim-ledger-{}", uuid::Uuid::new_v4()))
            .join("claim_ledger.json")
    }

    fn counts(total: u32, claimed: u32, claimable: u32, blocked: u32) -> Counts {
        Counts {
            total,
            claimed,
            claimable,
            blocked,
        }
    }

    #[test]
    fn day_uses_shanghai_midnight() {
        // 2026-11-12T15:59:59Z = 2026-11-12 23:59:59 +08:00
        assert_eq!(day_of(1_794_499_199_000), "2026-11-12");
        // 2026-11-12T16:00:00Z = 2026-11-13 00:00:00 +08:00
        assert_eq!(day_of(1_794_499_200_000), "2026-11-13");
    }

    #[test]
    fn record_overwrites_same_key_and_counts_attempts() {
        let path = temp_path();
        let outcome = || Outcome::single("already", "今日已领取");
        record_at(
            &path,
            "2026-11-12",
            1,
            PLATFORM_MINIMAX,
            Some("cn"),
            "acc-1",
            SCOPE_REWARDS,
            SOURCE_MANUAL,
            outcome(),
        )
        .unwrap();
        record_at(
            &path,
            "2026-11-12",
            2,
            PLATFORM_MINIMAX,
            Some("cn"),
            "acc-1",
            SCOPE_REWARDS,
            SOURCE_AUTO,
            outcome(),
        )
        .unwrap();
        let ledger = read_ledger(&path);
        assert_eq!(ledger.entries.len(), 1);
        let entry = &ledger.entries[0];
        assert_eq!(entry.attempt_count, 2);
        assert_eq!(entry.source, SOURCE_AUTO);
        assert_eq!(entry.state, STATE_CLAIMED);
        assert_eq!(entry.counts, counts(1, 1, 0, 0));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn different_scopes_are_independent() {
        let path = temp_path();
        for scope in [SCOPE_CHECKIN, SCOPE_DAILY] {
            record_at(
                &path,
                "2026-11-12",
                1,
                PLATFORM_WORKBUDDY,
                Some("cn"),
                "acc-1",
                scope,
                SOURCE_MANUAL,
                Outcome::single("success", "ok"),
            )
            .unwrap();
        }
        assert_eq!(read_ledger(&path).entries.len(), 2);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn prune_drops_old_days_and_old_task_details() {
        let path = temp_path();
        for day in ["2026-09-01", "2026-11-01", "2026-11-12"] {
            let mut outcome = Outcome::single("success", "ok");
            outcome.tasks = vec![TaskState {
                label: "和 AI 聊天 5 次".into(),
                state: "claimed".into(),
            }];
            record_at(
                &path,
                day,
                1,
                PLATFORM_WORKBUDDY,
                Some("cn"),
                "acc-1",
                SCOPE_DAILY,
                SOURCE_MANUAL,
                outcome,
            )
            .unwrap();
        }
        let ledger = read_ledger(&path);
        // 30 天窗口：09-01 被丢弃。
        assert_eq!(ledger.entries.len(), 2);
        // 任务明细只留 7 天：11-01 的 tasks 被清空。
        assert!(ledger.entries[0].tasks.is_empty());
        assert_eq!(ledger.entries[1].tasks.len(), 1);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn clear_only_touches_requested_scope() {
        let path = temp_path();
        for scope in [SCOPE_CHECKIN, SCOPE_DAILY] {
            record_at(
                &path,
                "2026-11-12",
                1,
                PLATFORM_WORKBUDDY,
                Some("cn"),
                "acc-1",
                scope,
                SOURCE_MANUAL,
                Outcome::single("success", "ok"),
            )
            .unwrap();
        }
        let removed = clear_at(
            &path,
            "2026-11-12",
            Some(PLATFORM_WORKBUDDY),
            Some(SCOPE_DAILY),
        )
        .unwrap();
        assert_eq!(removed, 1);
        let ledger = read_ledger(&path);
        assert_eq!(ledger.entries.len(), 1);
        assert_eq!(ledger.entries[0].scope, SCOPE_CHECKIN);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn single_state_mapping_covers_platform_states() {
        assert_eq!(normalize_single("already").0, STATE_CLAIMED);
        assert_eq!(normalize_single("available").0, STATE_PARTIAL);
        assert_eq!(normalize_single("closed").0, STATE_BLOCKED);
        assert_eq!(normalize_single("inactive").0, STATE_BLOCKED);
        assert_eq!(normalize_single("unconfirmed").0, STATE_UNCONFIRMED);
        assert_eq!(normalize_single("busy").0, STATE_UNCONFIRMED);
        assert_eq!(normalize_single("failed").0, STATE_ERROR);
        assert_eq!(normalize_single("skipped").0, STATE_SKIPPED);
        assert_eq!(normalize_single("already").1, counts(1, 1, 0, 0));
    }

    #[test]
    fn daily_outcome_prefers_claimable_over_remaining() {
        let result = json!({
            "ok": false,
            "message": "成长任务完成 18/24；部分任务尚未完成",
            "done": 18, "total": 24, "claimable": 2, "blocked": 4,
            "remaining": ["chat_5", "black_cat", "badges", "buddy_info"]
        });
        let outcome = daily_outcome(&result);
        assert_eq!(outcome.state, STATE_PARTIAL);
        assert_eq!(outcome.counts, counts(24, 18, 2, 4));

        let done = daily_outcome(&json!({
            "ok": true, "done": 24, "total": 24, "claimable": 0, "remaining": []
        }));
        assert_eq!(done.state, STATE_CLAIMED);
        assert_eq!(done.counts.claimable, 0);
    }

    #[test]
    fn daily_outcome_falls_back_when_claimable_missing() {
        let outcome = daily_outcome(&json!({
            "ok": false, "done": 3, "total": 5, "remaining": ["a", "b"]
        }));
        assert_eq!(outcome.state, STATE_PARTIAL);
        assert_eq!(outcome.counts.claimable, 2);
    }

    #[test]
    fn daily_outcome_treats_auth_invalid_and_empty_total_as_blocked() {
        assert_eq!(
            daily_outcome(&json!({"ok": false, "authInvalid": true, "message": "登录失效"})).state,
            STATE_BLOCKED
        );
        assert_eq!(
            daily_outcome(&json!({"ok": false, "message": "企业账号未开放个人成长任务"})).state,
            STATE_BLOCKED
        );
        assert_eq!(
            daily_outcome(&json!({"ok": false, "message": "账号受限，本轮已停止"})).state,
            STATE_BLOCKED
        );
        // 读不到任务表是可重试的失败，不能显示成「今日无可领取」。
        assert_eq!(
            daily_outcome(&json!({"ok": false, "message": "未能确认成长任务状态"})).state,
            STATE_ERROR
        );
    }
}
