//! Opt-in background work. The file lock also coordinates the desktop and web server.
use super::{
    credits,
    proc::{QoderProcessOps, RealProcessOps},
    store, QoderRoots, QoderTarget,
};
use crate::modules::{config, notifications, state_lock::StateLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct RegionConfig {
    pub daily_enabled: bool,
    pub daily_time: String,
    pub alerts_enabled: bool,
    pub expiry_days: u32,
    pub low_balance: f64,
    pub rotate_enabled: bool,
    pub rotate_below: f64,
    pub cooldown_minutes: u32,
}
impl Default for RegionConfig {
    fn default() -> Self {
        Self {
            daily_enabled: false,
            daily_time: "09:00".into(),
            alerts_enabled: false,
            expiry_days: 7,
            low_balance: 20.0,
            rotate_enabled: false,
            rotate_below: 10.0,
            cooldown_minutes: 30,
        }
    }
}
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub cn: RegionConfig,
    pub ai: RegionConfig,
}
impl Settings {
    fn validate(&self) -> Result<(), String> {
        for c in [&self.cn, &self.ai] {
            if time_minutes(&c.daily_time).is_none()
                || c.expiry_days > 90
                || !(5..=1440).contains(&c.cooldown_minutes)
                || !c.low_balance.is_finite()
                || !(0.0..=1_000_000.0).contains(&c.low_balance)
                || !c.rotate_below.is_finite()
                || !(0.0..=1_000_000.0).contains(&c.rotate_below)
            {
                return Err("Qoder 设置无效：时间使用 HH:mm，冷却时间为 5–1440 分钟".into());
            }
        }
        Ok(())
    }
}
fn time_minutes(s: &str) -> Option<u32> {
    if s.len() != 5 || s.as_bytes()[2] != b':' {
        return None;
    }
    let (h, m) = s.split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}
fn read<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "Qoder 自动任务文件损坏，已保留原文件".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(_) => Err("无法读取 Qoder 自动任务文件".into()),
    }
}
fn write<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let text = serde_json::to_string(value).map_err(|_| "无法保存 Qoder 自动任务")?;
    config::atomic_write(path, &text).map_err(|_| "无法保存 Qoder 自动任务".into())
}
pub fn settings() -> Result<Settings, String> {
    let settings: Settings = read(&store::default_store().join("automation-config.json"))?;
    settings.validate()?;
    Ok(settings)
}
pub fn save(value: Value) -> Result<Settings, String> {
    let settings: Settings = serde_json::from_value(value).map_err(|_| "Qoder 设置格式无效")?;
    settings.validate()?;
    let root = store::default_store();
    let _lock = StateLock::try_at(&root.join("automation.lock"))
        .map_err(|_| "Qoder 自动任务正在执行，请稍后保存")?;
    write(&root.join("automation-config.json"), &settings)?;
    Ok(settings)
}

/// Apply a region patch under the same lock as the read, avoiding lost updates from another host.
pub fn patch_region(global: bool, patch: Value) -> Result<Value, String> {
    let root = store::default_store();
    let _lock = StateLock::try_at(&root.join("automation.lock")).map_err(|e| e.to_string())?;
    let mut value = serde_json::to_value(settings()?).map_err(|_| "Cannot read Qoder settings")?;
    let key = if global { "ai" } else { "cn" };
    for (field, entry) in patch
        .as_object()
        .ok_or("Settings patch must be an object")?
    {
        value[key][field] = entry.clone();
    }
    let merged: Settings =
        serde_json::from_value(value).map_err(|_| "Unknown or invalid Qoder settings field")?;
    merged.validate()?;
    write(&root.join("automation-config.json"), &merged)?;
    Ok(json!({"region":key,"settings":if global {merged.ai}else{merged.cn}}))
}
#[derive(Default, Serialize, Deserialize)]
struct Attempt {
    day: String,
    count: u32,
    at: i64,
    done: bool,
}
#[derive(Default, Serialize, Deserialize)]
struct State {
    #[serde(skip)]
    outcomes: Vec<Value>,
    #[serde(skip)]
    notices: Vec<String>,
    claims: BTreeMap<String, Attempt>,
    alerts: BTreeMap<String, i64>,
    checked: BTreeMap<String, i64>,
    rotated: BTreeMap<String, i64>,
    logs: Vec<Value>,
}
fn due(a: Option<&Attempt>, day: &str, now: i64) -> bool {
    a.is_none_or(|a| a.day != day || (!a.done && a.count < 3 && now - a.at >= 30 * 60_000))
}
pub fn status() -> Result<Value, String> {
    let state: State = read(&store::default_store().join("automation-state.json"))?;
    Ok(json!({"settings":settings()?,"logs":state.logs.into_iter().rev().collect::<Vec<_>>()}))
}
fn log(state: &mut State, region: &str, action: &str, outcome: &str, now: i64) {
    state
        .outcomes
        .push(json!({"at":now,"region":region,"action":action,"outcome":outcome}));
    state
        .logs
        .push(json!({"at":now,"region":region,"action":action,"outcome":outcome}));
    if state.logs.len() > 100 {
        state.logs.drain(..state.logs.len() - 100);
    }
}
/// Earliest explicit expiry with a positive remaining bucket; never infer add-on expiry.
pub fn expiry(credit: &Value, now: i64) -> Option<i64> {
    let plan = (credit["plan"]["remaining"].as_f64().unwrap_or(0.0) > 0.0)
        .then(|| credit["planExpiresAt"].as_i64())
        .flatten();
    plan.into_iter()
        .chain(
            credit["dedicated"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["remaining"].as_f64().unwrap_or(0.0) > 0.0)
                .filter_map(|p| p["expiresAt"].as_i64()),
        )
        .filter(|t| *t > now)
        .min()
}
fn alert(state: &mut State, key: String, title: &str, now: i64) {
    if state
        .alerts
        .get(&key)
        .is_some_and(|at| now - at < 24 * 60 * 60_000)
    {
        return;
    }
    if notifications::record("warning", title, Some("请前往 Qoder 账号管理查看详情")).is_ok()
    {
        if !state.notices.iter().any(|s| s == title) {
            state.notices.push(title.to_string());
        }
        state.alerts.insert(key, now);
    }
}
/// Runs enabled tasks only, including startup catch-up. No forced claim/rotation bypass.
pub async fn cycle() -> Result<Value, String> {
    cycle_for(None, &|| false).await
}

/// Region-scoped one-shot execution; cancellation stops scheduling new account work.
pub async fn cycle_for(
    global_filter: Option<bool>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Value, String> {
    let root = store::default_store();
    let _lock = match StateLock::try_at(&root.join("automation.lock")) {
        Ok(lock) => lock,
        Err(_) => return Ok(json!({"skipped":true})),
    };
    let settings = settings()?;
    if [&settings.cn, &settings.ai]
        .iter()
        .all(|c| !c.daily_enabled && !c.alerts_enabled && !c.rotate_enabled)
    {
        return Ok(json!({"ok":true}));
    }
    let state_path = root.join("automation-state.json");
    let mut state: State = read(&state_path)?;
    let now = config::now_ms();
    let local = chrono::Local::now();
    let day = local.format("%Y-%m-%d").to_string();
    let minutes = time_minutes(&local.format("%H:%M").to_string()).unwrap_or(0);
    state.alerts.retain(|_, t| now - *t < 2 * 24 * 60 * 60_000);
    let bundles = store::list(&root, None);
    state
        .claims
        .retain(|key, _| bundles.iter().any(|b| b.identity_key.as_ref() == Some(key)));
    for (region, global, cfg, target, desktop) in [
        (
            "cn",
            false,
            &settings.cn,
            QoderTarget::Cli,
            QoderTarget::Desktop,
        ),
        (
            "ai",
            true,
            &settings.ai,
            QoderTarget::GlobalCli,
            QoderTarget::GlobalDesktop,
        ),
    ] {
        if cancelled() {
            break;
        }
        if global_filter.is_some_and(|filter| filter != global) {
            continue;
        }
        if !cfg.daily_enabled && !cfg.alerts_enabled && !cfg.rotate_enabled {
            continue;
        }
        let mut groups: BTreeMap<String, Vec<&store::BundleMeta>> = BTreeMap::new();
        for b in bundles.iter().filter(|b| b.target.is_global() == global) {
            if let Some(key) = &b.identity_key {
                groups.entry(key.clone()).or_default().push(b);
            }
        }
        let check = (cfg.alerts_enabled || cfg.rotate_enabled)
            && state
                .checked
                .get(region)
                .is_none_or(|at| now - at >= 5 * 60_000);
        let mut balances = BTreeMap::new();
        for (key, group) in &groups {
            if cancelled() {
                break;
            }
            let ids: Vec<String> = group.iter().map(|b| b.id.clone()).collect();
            if cfg.daily_enabled
                && minutes >= time_minutes(&cfg.daily_time).unwrap_or(540)
                && due(state.claims.get(key), &day, now)
            {
                let old_count = state
                    .claims
                    .get(key)
                    .filter(|a| a.day == day)
                    .map_or(0, |a| a.count);
                // Persist before the external action so process crashes cannot cause a rapid replay.
                state.claims.insert(
                    key.clone(),
                    Attempt {
                        day: day.clone(),
                        count: old_count + 1,
                        at: now,
                        done: false,
                    },
                );
                write(&state_path, &state)?;
                let result = credits::claim(&ids, None).await;
                let ok = result["ok"].as_bool() == Some(true);
                if let Some(a) = state.claims.get_mut(key) {
                    a.done = ok;
                }
                log(
                    &mut state,
                    region,
                    "daily",
                    if ok { "success" } else { "failed" },
                    config::now_ms(),
                );
                if !ok {
                    alert(
                        &mut state,
                        format!("claim:{key}"),
                        "Qoder 自动领取失败",
                        now,
                    );
                }
                write(&state_path, &state)?;
            }
            if check && !cancelled() {
                let mut credit = credits::query(&ids, None).await;
                if cfg.alerts_enabled {
                    if credit["ok"] != true {
                        alert(
                            &mut state,
                            format!("query:{key}"),
                            "Qoder 积分查询异常，请检查登录状态",
                            now,
                        );
                    } else {
                        if credit["remaining"]
                            .as_f64()
                            .is_some_and(|n| n <= cfg.low_balance)
                        {
                            alert(&mut state, format!("low:{key}"), "Qoder 账号积分不足", now);
                        }
                        if expiry(&credit, now)
                            .is_some_and(|t| t - now <= cfg.expiry_days as i64 * 86_400_000)
                        {
                            alert(
                                &mut state,
                                format!("expiry:{key}"),
                                "Qoder 账号积分即将到期",
                                now,
                            );
                        }
                    }
                }
                if credit["ok"] == true {
                    credit["_queriedAt"] = json!(config::now_ms());
                    balances.insert(key.clone(), credit);
                }
            }
        }
        if check && !cancelled() {
            let now = config::now_ms();
            balances.retain(|_, c| {
                c["_queriedAt"]
                    .as_i64()
                    .is_some_and(|at| now - at <= 60_000)
            });
            state.checked.insert(region.into(), now);
            if cfg.rotate_enabled
                && state
                    .rotated
                    .get(region)
                    .is_none_or(|at| now - *at >= cfg.cooldown_minutes as i64 * 60_000)
            {
                let roots = QoderRoots::real();
                let current = store::current_id(&root, &roots, target)
                    .and_then(|id| store::load(&root, &id).ok());
                let current_key = current.as_ref().and_then(|m| m.identity_key.as_ref());
                if let Some(key) = current_key.filter(|key| {
                    balances
                        .get(*key)
                        .and_then(|c| c["remaining"].as_f64())
                        .is_some_and(|n| n <= cfg.rotate_below)
                }) {
                    if !RealProcessOps.running_pids(target).is_empty()
                        || !RealProcessOps.running_pids(desktop).is_empty()
                    {
                        log(&mut state, region, "rotate", "running", now);
                    } else {
                        let mut eligible: Vec<_> = groups
                            .iter()
                            .filter(|(k, _)| *k != key)
                            .filter_map(|(k, g)| {
                                let credit = balances.get(k)?;
                                let remaining = credit["remaining"].as_f64()?;
                                if remaining <= cfg.rotate_below {
                                    return None;
                                }
                                let bundle = g
                                    .iter()
                                    .filter(|b| b.target == target)
                                    .max_by_key(|b| b.captured_at)?;
                                Some((bundle, expiry(credit, now).unwrap_or(i64::MAX), remaining))
                            })
                            .collect();
                        eligible.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| b.2.total_cmp(&a.2)));
                        if let Some((bundle, _, _)) = eligible.first() {
                            let result = super::switch::execute_idle(
                                &root,
                                &roots,
                                &RealProcessOps,
                                &bundle.id,
                                key,
                            );
                            log(
                                &mut state,
                                region,
                                "rotate",
                                if result.is_ok() {
                                    "success"
                                } else {
                                    "deferred"
                                },
                                now,
                            );
                            state.rotated.insert(region.into(), now);
                            if result.is_ok() {
                                let _ =
                                    notifications::record("info", "Qoder CLI 已自动轮换账号", None);
                            }
                        } else {
                            log(&mut state, region, "rotate", "no-candidate", now);
                        }
                    }
                }
            }
        }
    }
    write(&state_path, &state)?;
    Ok(json!({"ok":true,"notices":state.notices,"outcomes":state.outcomes}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_is_bounded_and_day_scoped() {
        let a = Attempt {
            day: "today".into(),
            count: 3,
            at: 0,
            done: false,
        };
        assert!(!due(Some(&a), "today", 9_000_000));
        assert!(due(Some(&a), "tomorrow", 9_000_000));
        let a = Attempt { count: 1, ..a };
        assert!(!due(Some(&a), "today", 10));
        assert!(due(Some(&a), "today", 1_800_000));
        assert!(!due(Some(&Attempt { done: true, ..a }), "today", 9_000_000));
    }
    #[test]
    fn validates_settings_and_expiry_without_guessing() {
        assert!(Settings::default().validate().is_ok());
        for time in ["24:00", "12:60", "1:00", "09:0", "ab:cd"] {
            assert!(time_minutes(time).is_none());
        }
        assert_eq!(
            expiry(
                &json!({"plan":{"remaining":0},"planExpiresAt":200,"addOn":{"remaining":100},"dedicated":[{"remaining":2,"expiresAt":300},{"remaining":3,"expiresAt":90}]}),
                100
            ),
            Some(300)
        );
        assert_eq!(expiry(&json!({"addOn":{"remaining":100}}), 0), None);
    }
}
