//! Opt-in tasks, shared across desktop and WebUI through a process lock.
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub cn: RegionSettings,
    pub ai: RegionSettings,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct RegionSettings {
    pub refresh_enabled: bool,
    pub interval_minutes: u32,
    pub daily_enabled: bool,
    pub daily_time: String,
    pub alerts_enabled: bool,
    pub expiry_days: u32,
    pub low_balance: f64,
    pub rotate_enabled: bool,
    pub rotate_below: f64,
    pub cooldown_minutes: u32,
}
impl Default for RegionSettings {
    fn default() -> Self {
        Self {
            refresh_enabled: false,
            interval_minutes: 30,
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
impl Settings {
    fn validate(&self) -> Result<(), String> {
        for s in [&self.cn, &self.ai] {
            if !(5..=1440).contains(&s.interval_minutes)
                || !(5..=1440).contains(&s.cooldown_minutes)
                || s.expiry_days > 90
                || !s.low_balance.is_finite()
                || !s.rotate_below.is_finite()
                || !(0.0..=1_000_000.0).contains(&s.low_balance)
                || !(0.0..=1_000_000.0).contains(&s.rotate_below)
                || time_minutes(&s.daily_time).is_none()
            {
                return Err(
                    "TRAE 设置无效：间隔与冷却时间为 5–1440 分钟，阈值为非负数，签到时间为 HH:mm"
                        .into(),
                );
            }
        }
        Ok(())
    }
    fn enabled(&self) -> bool {
        [&self.cn, &self.ai]
            .iter()
            .any(|s| s.refresh_enabled || s.alerts_enabled || s.rotate_enabled || s.daily_enabled)
    }
}
/// 本机时区的 HH:mm，换算成当天分钟数。
fn time_minutes(value: &str) -> Option<u32> {
    if value.len() != 5 || value.as_bytes().get(2) != Some(&b':') {
        return None;
    }
    let (hour, minute) = value.split_once(':')?;
    let (hour, minute): (u32, u32) = (hour.parse().ok()?, minute.parse().ok()?);
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct State {
    checked: BTreeMap<String, i64>,
    rotated: BTreeMap<String, i64>,
    alerted: BTreeMap<String, i64>,
    claims: BTreeMap<String, Attempt>,
    logs: Vec<Value>,
}
/// 每日签到的当日尝试记录。同一天最多尝试 3 次，间隔 30 分钟。
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Attempt {
    day: String,
    count: u32,
    at: i64,
    done: bool,
}
fn due(attempt: Option<&Attempt>, day: &str, now: i64) -> bool {
    attempt.is_none_or(|attempt| {
        attempt.day != day || (!attempt.done && attempt.count < 3 && now - attempt.at > 30 * 60)
    })
}
/// 签到回执分类。成功以 `ok` 为准：没有 `state` 或 `state` 写错时，也不能记成失败。
fn classify(result: &Value) -> &'static str {
    let ok = result["ok"].as_bool() == Some(true);
    match (ok, result["state"].as_str()) {
        (true, Some("already")) => "already",
        (true, _) => "claimed",
        (false, Some("busy")) => "busy",
        (false, Some("closed")) => "closed",
        (false, Some("auth")) => "auth",
        (false, Some("unconfirmed")) => "unconfirmed",
        (false, _) => "failed",
    }
}
fn settled(outcome: &str) -> bool {
    matches!(outcome, "claimed" | "already")
}
fn read<T: serde::de::DeserializeOwned + Default>(path: &std::path::Path) -> Result<T, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "TRAE 自动任务文件损坏，已保留原文件".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(_) => Err("无法读取 TRAE 自动任务配置".into()),
    }
}
fn write<T: Serialize>(engine: &Engine, name: &str, value: &T) -> Result<(), String> {
    write_private(
        &engine.paths.store.join(name),
        &serde_json::to_string(value).map_err(|_| "配置编码失败")?,
    )
    .map_err(|_| "无法保存 TRAE 自动任务配置".into())
}
fn settings(engine: &Engine) -> Result<Settings, String> {
    let s: Settings = read(&engine.paths.store.join("automation-config.json"))?;
    s.validate()?;
    Ok(s)
}
pub(super) fn status(engine: &Engine) -> Result<Value, String> {
    let state: State = read(&engine.paths.store.join("automation-state.json"))?;
    Ok(
        json!({"settings":settings(engine)?,"logs":state.logs.into_iter().rev().collect::<Vec<_>>()}),
    )
}
pub(super) fn save(engine: &Engine, value: Value) -> Result<Value, String> {
    let config: Settings =
        serde_json::from_value(value).map_err(|_| "TRAE 设置包含未知字段或类型错误")?;
    config.validate()?;
    let _guard = StateLock::try_at(&engine.paths.store.join("automation.lock"))
        .map_err(|_| "TRAE 自动任务正在执行，请稍后保存")?;
    write(engine, "automation-config.json", &config)?;
    status(engine)
}
pub fn patch_region(region: Region, patch: Value) -> Result<Value, String> {
    let engine = Engine::real();
    let _guard = StateLock::try_at(&engine.paths.store.join("automation.lock"))
        .map_err(|_| "TRAE 自动任务正在执行")?;
    let mut config = serde_json::to_value(settings(&engine)?).map_err(|_| "无法读取设置")?;
    for (key, value) in patch.as_object().ok_or("设置须为 JSON 对象")? {
        config[region.text()][key] = value.clone();
    }
    let config: Settings =
        serde_json::from_value(config).map_err(|_| "未知或无效的 TRAE 设置字段")?;
    config.validate()?;
    write(&engine, "automation-config.json", &config)?;
    status(&engine)
}
fn log(state: &mut State, region: Region, action: &str, outcome: &str) {
    state.logs.push(json!({"at":chrono::Utc::now().timestamp(),"region":region,"action":action,"outcome":outcome}));
    if state.logs.len() > 200 {
        state.logs.drain(..state.logs.len() - 200);
    }
}
/// 同类提醒每天最多一次，避免签到失败时反复打扰。
fn alert(
    state: &mut State,
    notices: &mut Vec<String>,
    key: String,
    title: &str,
    body: &str,
    now: i64,
) {
    if state.alerted.get(&key).is_some_and(|at| now - at < 86400) {
        return;
    }
    if crate::modules::notifications::record("warning", title, Some(body)).is_ok() {
        state.alerted.insert(key, now);
        notices.push(title.to_string());
    }
}
/// 国内版每日签到。每个账号用自己快照里的官方设备号。
async fn daily(
    engine: &Engine,
    state: &mut State,
    c: &RegionSettings,
    notices: &mut Vec<String>,
    now: i64,
) -> Result<(), String> {
    let local = chrono::Local::now();
    let day = local.format("%Y-%m-%d").to_string();
    let current = time_minutes(&local.format("%H:%M").to_string()).unwrap_or(0);
    if current < time_minutes(&c.daily_time).unwrap_or(540) {
        return Ok(());
    }
    let accounts = {
        let _lock = engine.lock()?;
        engine
            .load()?
            .accounts
            .into_iter()
            .filter(|account| account.variant == Region::Cn)
            .collect::<Vec<_>>()
    };
    state
        .claims
        .retain(|id, _| accounts.iter().any(|account| &account.id == id));
    for account in &accounts {
        if !due(state.claims.get(&account.id), &day, now) {
            continue;
        }
        // 先落盘计数，进程崩溃也不会立刻重复领取。
        let count = state
            .claims
            .get(&account.id)
            .filter(|attempt| attempt.day == day)
            .map_or(0, |attempt| attempt.count);
        state.claims.insert(
            account.id.clone(),
            Attempt {
                day: day.clone(),
                count: count + 1,
                at: now,
                done: false,
            },
        );
        write(engine, "automation-state.json", state)?;
        let args: Args = serde_json::from_value(json!({"accountId": account.id}))
            .map_err(|_| "自动签到参数无效".to_string())?;
        let (outcome, message) = match engine.checkin(&args).await {
            Ok(result) => {
                let outcome = classify(&result);
                if let Some(attempt) = state.claims.get_mut(&account.id) {
                    attempt.done = settled(outcome);
                }
                if settled(outcome) {
                    // 签到成功后刷新余额，让页面显示到账后的积分。
                    let _ = engine.credit(&args).await;
                }
                (
                    outcome.to_string(),
                    result["message"].as_str().unwrap_or("").to_string(),
                )
            }
            Err(error) => ("failed".to_string(), error),
        };
        log(state, Region::Cn, "daily", &outcome);
        // 限流只是排队，等下一轮补签；其余失败每天提醒一次。
        if !matches!(outcome.as_str(), "claimed" | "already" | "busy") {
            alert(
                state,
                notices,
                format!("daily:{}", account.id),
                &format!("TRAE 每日签到失败 · {}", account.name),
                &if message.is_empty() {
                    "请前往 TRAE 账号管理查看详情".to_string()
                } else {
                    message.clone()
                },
                now,
            );
        }
        write(engine, "automation-state.json", state)?;
    }
    Ok(())
}
fn finite_balance(account: &Account, now: i64, max_age: i64) -> Option<(f64, String)> {
    if account.expires_at.is_some_and(|t| t <= now) {
        return None;
    }
    let c = account.credits.as_ref()?;
    let unit = c["unit"].as_str()?;
    if c["available"] != true
        || c["unlimited"] == true
        || !matches!(unit, "credits" | "usd")
        || now - c["checkedAt"].as_i64()? > max_age
    {
        return None;
    }
    Some((
        c["remaining"]
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.0)?,
        unit.into(),
    ))
}
fn candidate<'a>(
    accounts: &'a [Account],
    current: &Account,
    region: Region,
    config: &RegionSettings,
    now: i64,
) -> Option<&'a Account> {
    let (balance, unit) = finite_balance(current, now, config.interval_minutes as i64 * 120)?;
    if balance >= config.rotate_below {
        return None;
    }
    accounts
        .iter()
        .filter(|a| a.variant == region && a.id != current.id)
        .filter_map(|a| {
            finite_balance(a, now, config.interval_minutes as i64 * 120)
                .filter(|(b, u)| u == &unit && *b > balance && *b > config.rotate_below)
                .map(|(b, _)| (a, b))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(a, _)| a)
}
pub(super) fn rotate(
    engine: &Engine,
    accounts: &[Account],
    region: Region,
    c: &RegionSettings,
    state: &mut State,
    now: i64,
) -> Result<(), String> {
    let key = region.text();
    let live = engine
        .storage(region, Client::Desktop)
        .and_then(|v| protocol::auth_keys(&v))
        .and_then(|v| protocol::identity(&v));
    if let Ok(live) = live {
        if let Some(current) = accounts
            .iter()
            .find(|a| a.variant == region && a.uid == live.uid)
        {
            if let Some(next) = candidate(accounts, current, region, c, now) {
                state.rotated.insert(key.into(), now);
                let outcome = match engine.processes.running(region, Client::Desktop) {
                    Ok(true) => "running",
                    Err(_) => "unknown-process-state",
                    Ok(false) => {
                        let mut args: Args =
                            serde_json::from_value(json!({"variant":region,"accountId":next.id}))
                                .map_err(|_| "轮换参数无效")?;
                        args.expected_current_uid = Some(live.uid.clone());
                        if engine.switch(&args).is_ok() {
                            "switched"
                        } else {
                            "deferred"
                        }
                    }
                };
                log(state, region, "rotate", outcome);
            }
        }
    }
    Ok(())
}
pub(super) async fn run(
    engine: &Engine,
    force: bool,
    only: Option<Region>,
) -> Result<Value, String> {
    let config = settings(engine)?;
    if !config.enabled() {
        return Ok(json!({"notices":[],"ran":false}));
    }
    let _guard = StateLock::try_at(&engine.paths.store.join("automation.lock"))
        .map_err(|_| "TRAE 自动任务正在执行")?;
    let mut state: State = read(&engine.paths.store.join("automation-state.json"))?;
    let mut notices = Vec::<String>::new();
    for (region, c) in [(Region::Cn, &config.cn), (Region::Ai, &config.ai)] {
        if only.is_some_and(|r| r != region) {
            continue;
        }
        if !(c.refresh_enabled || c.alerts_enabled || c.rotate_enabled || c.daily_enabled) {
            continue;
        }
        let now = chrono::Utc::now().timestamp();
        let key = region.text();
        // 每日签到按自己的时间表执行，签到接口只存在于国内版。
        if c.daily_enabled && region == Region::Cn {
            daily(engine, &mut state, c, &mut notices, now).await?;
        }
        if !(c.refresh_enabled || c.alerts_enabled || c.rotate_enabled) {
            continue;
        }
        if !force
            && state
                .checked
                .get(key)
                .is_some_and(|at| now - at < (c.interval_minutes as i64 * 60))
        {
            continue;
        }
        state.checked.insert(key.into(), now);
        write(engine, "automation-state.json", &state)?;
        let accounts = {
            let _lock = engine.lock()?;
            engine
                .load()?
                .accounts
                .into_iter()
                .filter(|a| a.variant == region)
                .collect::<Vec<_>>()
        };
        if accounts.is_empty() {
            log(&mut state, region, "refresh", "no-accounts");
            write(engine, "automation-state.json", &state)?;
            continue;
        }
        let mut failed = false;
        for a in &accounts {
            let args: Args = serde_json::from_value(json!({"accountId":a.id}))
                .map_err(|_| "自动查询参数无效")?;
            if engine.credit(&args).await.is_err() {
                failed = true;
            }
        }
        log(
            &mut state,
            region,
            "refresh",
            if failed { "partial" } else { "updated" },
        );
        let accounts = {
            let _lock = engine.lock()?;
            engine.load()?.accounts
        };
        if c.alerts_enabled {
            for a in accounts.iter().filter(|a| a.variant == region) {
                let low = finite_balance(a, now, c.interval_minutes as i64 * 120)
                    .is_some_and(|(b, _)| b < c.low_balance);
                let expiring = a
                    .credits
                    .as_ref()
                    .filter(|v| v["available"] == true)
                    .and_then(|v| v["expiresAt"].as_i64())
                    .is_some_and(|t| t > now && t - now <= c.expiry_days as i64 * 86400);
                let expired = a.expires_at.is_some_and(|at| at <= now);
                for (kind, enabled, title) in [
                    ("low", low, "积分余额不足"),
                    ("expiry", expiring, "积分即将到期"),
                    ("login", expired, "登录已过期"),
                ] {
                    let alert_key = format!("{}:{kind}", a.id);
                    if enabled
                        && state
                            .alerted
                            .get(&alert_key)
                            .is_none_or(|at| now - at >= 86400)
                    {
                        let title = format!("TRAE {} · {title}", a.name);
                        if crate::modules::notifications::record(
                            "warning",
                            &title,
                            Some("请前往 TRAE 账号管理查看详情"),
                        )
                        .is_ok()
                        {
                            state.alerted.insert(alert_key, now);
                            notices.push(title);
                        }
                    }
                }
            }
        }
        if c.rotate_enabled
            && state
                .rotated
                .get(key)
                .is_none_or(|at| now - at >= c.cooldown_minutes as i64 * 60)
        {
            rotate(engine, &accounts, region, c, &mut state, now)?;
        }
        write(engine, "automation-state.json", &state)?;
    }
    Ok(json!({"ran":true,"notices":notices}))
}
pub async fn cycle() -> Result<Value, String> {
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || runtime.block_on(run(&Engine::real(), false, None)))
        .await
        .map_err(|_| "TRAE 自动任务异常结束".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_opt_in_and_invalid_values_rejected() {
        assert!(!Settings::default().enabled());
        let mut s = Settings::default();
        s.cn.interval_minutes = 0;
        assert!(s.validate().is_err());
    }
    #[test]
    fn daily_settings_are_opt_in_and_require_a_real_time() {
        let mut s = Settings::default();
        assert!(!s.enabled());
        assert_eq!(s.cn.daily_time, "09:00");
        s.cn.daily_enabled = true;
        assert!(s.enabled());
        assert!(s.validate().is_ok());
        for time in ["24:00", "12:60", "9:00", "09:0", "ab:cd", ""] {
            s.cn.daily_time = time.to_string();
            assert!(s.validate().is_err(), "{time} should be rejected");
        }
        assert_eq!(time_minutes("09:00"), Some(540));
        assert_eq!(time_minutes("00:00"), Some(0));
        assert_eq!(time_minutes("23:59"), Some(1439));
    }
    #[test]
    fn daily_attempts_are_day_scoped_and_bounded() {
        let attempt = Attempt {
            day: "today".into(),
            count: 1,
            at: 0,
            done: false,
        };
        assert!(due(None, "today", 10));
        assert!(!due(Some(&attempt), "today", 10));
        // 间隔超过 30 分钟后才允许再试。
        assert!(due(Some(&attempt), "today", 1801));
        assert!(!due(Some(&attempt), "today", 1800));
        // 已经签到的账号当天不再重复领取。
        assert!(!due(
            Some(&Attempt {
                done: true,
                ..attempt.clone()
            }),
            "today",
            100_000
        ));
        // 当天最多 3 次。
        assert!(!due(
            Some(&Attempt {
                count: 3,
                ..attempt.clone()
            }),
            "today",
            100_000
        ));
        // 换一天重新开始。
        assert!(due(Some(&attempt), "tomorrow", 100_000));
    }
    #[test]
    fn successful_checkin_is_not_recorded_as_failure() {
        assert_eq!(
            classify(&json!({"ok": true, "state": "claimed"})),
            "claimed"
        );
        assert_eq!(
            classify(&json!({"ok": true, "state": "already"})),
            "already"
        );
        // 回执缺少 state，或把 state 写成失败，成功仍然记为已领取。
        assert_eq!(classify(&json!({"ok": true})), "claimed");
        assert_eq!(classify(&json!({"ok": true, "state": "failed"})), "claimed");
        assert_eq!(classify(&json!({"ok": false, "state": "busy"})), "busy");
        assert_eq!(classify(&json!({"ok": false})), "failed");
        assert!(settled("claimed") && settled("already"));
        assert!(!settled("busy") && !settled("failed"));
    }
    #[test]
    fn rotation_never_compares_currency_or_unknown_balances() {
        let account = |id: &str, unit: &str, balance: f64| Account {
            id: id.into(),
            uid: id.into(),
            name: id.into(),
            email: None,
            variant: Region::Cn,
            client: Client::Desktop,
            captured_at: 1,
            expires_at: None,
            credits: Some(
                json!({"remaining":balance,"unit":unit,"available":true,"checkedAt":1000,"unlimited":false}),
            ),
            payload: String::new(),
        };
        let current = account("current", "credits", 1.0);
        let mut list = vec![account("usd", "usd", 100.0)];
        let c = RegionSettings::default();
        assert!(candidate(&list, &current, Region::Cn, &c, 1000).is_none());
        list.push(account("credit", "credits", 100.0));
        assert_eq!(
            candidate(&list, &current, Region::Cn, &c, 1000).unwrap().id,
            "credit"
        );
        assert!(candidate(&list, &current, Region::Cn, &c, 999999).is_none());
    }
}
