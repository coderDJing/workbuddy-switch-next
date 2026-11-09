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
            {
                return Err("TRAE 设置无效：间隔与冷却时间为 5–1440 分钟，阈值为非负数".into());
            }
        }
        Ok(())
    }
    fn enabled(&self) -> bool {
        [&self.cn, &self.ai]
            .iter()
            .any(|s| s.refresh_enabled || s.alerts_enabled || s.rotate_enabled)
    }
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct State {
    checked: BTreeMap<String, i64>,
    rotated: BTreeMap<String, i64>,
    alerted: BTreeMap<String, i64>,
    logs: Vec<Value>,
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
        if !(c.refresh_enabled || c.alerts_enabled || c.rotate_enabled) {
            continue;
        }
        let now = chrono::Utc::now().timestamp();
        let key = region.text();
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
