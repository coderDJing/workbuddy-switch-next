//! Doubao desktop integration. Protocol references and verification boundaries: docs/doubao-integration.md.
pub mod automation;
pub(crate) mod backup;
mod network;
mod paths;
mod process;
mod profile;
#[cfg(test)]
mod tests;
mod vault;

use crate::modules::state_lock::StateLock;
use paths::Paths;
use process::{NativeProcesses, Processes};
use profile::{Identity, Snapshot};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
use vault::{SystemVault, Vault};

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn write(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let text = serde_json::to_string(value).map_err(|_| "无法编码豆包数据")?;
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|_| "无法创建豆包账号目录")?;
    }
    crate::modules::config::atomic_write(path, &text).map_err(|_| "无法保存豆包数据".into())
}
fn read<T: serde::de::DeserializeOwned>(path: &Path, max: u64) -> Result<T, String> {
    if std::fs::metadata(path)
        .map_err(|_| "无法读取豆包数据")?
        .len()
        > max
    {
        return Err("豆包数据超过读取上限".into());
    }
    serde_json::from_slice(&std::fs::read(path).map_err(|_| "无法读取豆包数据")?)
        .map_err(|_| "豆包数据损坏，已停止操作并保留原文件".into())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Account {
    id: String,
    uid: String,
    name: String,
    captured_at: i64,
    custom_name: bool,
    credential: String,
    snapshot: Option<String>,
    origin: Option<String>,
    #[serde(default)]
    needs_login: bool,
    #[serde(default)]
    credits: Option<Value>,
    #[serde(default)]
    checkin: Option<Value>,
}
impl Account {
    fn meta(&self, active: bool, origin: &str) -> Value {
        json!({"id":self.id,"uid":self.uid,"name":self.name,"capturedAt":self.captured_at,"active":active,"needsLogin":self.needs_login,"switchable":self.snapshot.is_some()&&self.origin.as_deref()==Some(origin),"credits":self.credits,"checkin":self.checkin})
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    version: u32,
    accounts: Vec<Account>,
    #[serde(default)]
    snapshots: Vec<Value>,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            version: 1,
            accounts: vec![],
            snapshots: vec![],
        }
    }
}
pub struct Engine {
    paths: Paths,
    vault: Box<dyn Vault>,
    processes: Box<dyn Processes>,
}
impl Engine {
    pub fn real() -> Self {
        Self {
            paths: Paths::real(),
            vault: Box::new(SystemVault),
            processes: Box::new(NativeProcesses),
        }
    }
    fn lock(&self) -> Result<StateLock, String> {
        StateLock::wait_at(
            &self.paths.store.join("accounts.lock"),
            Duration::from_secs(3),
        )
        .map_err(|_| "豆包账号正在更新，请稍后重试".into())
    }
    fn load(&self) -> Result<Store, String> {
        let p = self.paths.store.join("accounts.json");
        if !p.exists() {
            return Ok(Store::default());
        }
        let s: Store = read(&p, 16 * 1024 * 1024)?;
        if s.version != 1 || s.accounts.len() > 1000 {
            return Err("豆包账号库版本或大小不受支持".into());
        }
        let mut ids = std::collections::HashSet::new();
        for a in &s.accounts {
            if !ids.insert(&a.id)
                || a.id != format!("doubao-{}", a.uid)
                || !a.uid.bytes().all(|b| b.is_ascii_digit())
                || a.uid.is_empty()
                || a.uid.len() > 64
                || a.snapshot
                    .as_ref()
                    .is_some_and(|v| v.len() != 64 || !v.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return Err("豆包账号库身份或快照标识无效".into());
            }
        }
        Ok(s)
    }
    fn save(&self, s: &Store) -> Result<(), String> {
        write(&self.paths.store.join("accounts.json"), s)
    }
    fn origin(&self) -> Result<String, String> {
        let p = self.paths.store.join("device-id.json");
        if p.exists() {
            return read(&p, 256);
        }
        let id = uuid::Uuid::new_v4().to_string();
        write(&p, &id)?;
        Ok(id)
    }
    fn snapshot_path(&self, key: &str) -> PathBuf {
        self.paths.store.join("profiles").join(format!("{key}.enc"))
    }
    fn read_snapshot(&self, a: &Account) -> Result<Snapshot, String> {
        if a.origin.as_deref() != Some(self.origin()?.as_str()) {
            return Err(
                "此账号来自另一台设备，请在本机登录并识别一次后再切换；额度和签到可直接使用".into(),
            );
        }
        let key = a
            .snapshot
            .as_deref()
            .ok_or("此账号尚未保存本机登录态，请先识别本机账号")?;
        let sealed: String = read(&self.snapshot_path(key), 128 * 1024 * 1024)?;
        let bytes = self.vault.open(&sealed)?;
        if hash(&bytes) != key {
            return Err("豆包账号快照校验失败".into());
        }
        let snapshot: Snapshot = serde_json::from_slice(&bytes).map_err(|_| "豆包账号快照损坏")?;
        profile::validate(&snapshot, true)?;
        if profile::snapshot_identity(&snapshot)?.is_none_or(|id| id.uid != a.uid) {
            return Err("豆包快照与账号身份不一致".into());
        }
        Ok(snapshot)
    }
    fn credential(&self, a: &Account) -> Result<Identity, String> {
        let id: Identity = serde_json::from_slice(&self.vault.open(&a.credential)?)
            .map_err(|_| "豆包授权数据损坏")?;
        if id.uid != a.uid {
            return Err("豆包授权与账号身份不一致".into());
        }
        Ok(id)
    }
    /// 客户端重新登录或重启会轮换 `x-tt-multi-sids`，此时账号库里的会话快照
    /// 已经过期，但用户并没有退出登录。判定时只比对本机当前会话，不联网。
    fn session_updated(&self, a: &Account) -> bool {
        let Ok(saved) = self.credential(a) else {
            return false;
        };
        matches!(
            profile::live(&self.paths.data),
            Ok(Some(live)) if live.uid == a.uid && live.session != saved.session
        )
    }
    /// 认证失败时给出可执行的提示：会话轮换只需重新识别，不必重新登录。
    fn auth_failure(&self, a: &Account, e: String) -> String {
        if network::auth_error(&e) && self.session_updated(a) {
            network::SESSION_UPDATED.into()
        } else {
            e
        }
    }
    fn put(
        &self,
        store: &mut Store,
        id: &Identity,
        snapshot: Option<&Snapshot>,
    ) -> Result<Account, String> {
        let account_id = format!("doubao-{}", id.uid);
        let sealed = self
            .vault
            .seal(&serde_json::to_vec(id).map_err(|_| "无法保存豆包授权")?)?;
        let captured = now() * 1000;
        let mut a = store
            .accounts
            .iter()
            .find(|a| a.id == account_id)
            .cloned()
            .unwrap_or(Account {
                id: account_id,
                uid: id.uid.clone(),
                name: id.name.clone(),
                captured_at: captured,
                custom_name: false,
                credential: String::new(),
                snapshot: None,
                origin: None,
                needs_login: false,
                credits: None,
                checkin: None,
            });
        if !a.custom_name {
            a.name = id.name.clone();
        }
        a.credential = sealed;
        a.needs_login = false;
        if let Some(s) = snapshot {
            let bytes = serde_json::to_vec(s).map_err(|_| "无法编码豆包快照")?;
            let key = hash(&bytes);
            write(&self.snapshot_path(&key), &self.vault.seal(&bytes)?)?;
            a.snapshot = Some(key);
            a.origin = Some(self.origin()?);
            a.captured_at = captured;
        }
        store.accounts.retain(|v| v.id != a.id);
        store.accounts.push(a.clone());
        Ok(a)
    }
    fn recovery_file(&self) -> PathBuf {
        self.paths.store.join("recovery.json")
    }
    fn recover(&self) -> Result<(), String> {
        if !self.recovery_file().exists() {
            return Ok(());
        }
        if self.processes.running()? {
            return Err("上次豆包切换中断，请先完全退出豆包，再重试以恢复原登录".into());
        }
        let sealed: String = read(&self.recovery_file(), 128 * 1024 * 1024)?;
        let snapshot: Snapshot =
            serde_json::from_slice(&self.vault.open(&sealed)?).map_err(|_| "豆包恢复记录损坏")?;
        profile::restore(&self.paths.data, &snapshot)
            .map_err(|_| "无法恢复上次豆包登录，请保留 recovery.json 并重试")?;
        std::fs::remove_file(self.recovery_file()).map_err(|_| "无法完成豆包登录恢复")?;
        Ok(())
    }
    fn require_closed(&self, close: bool) -> Result<(), String> {
        if self.processes.running()? {
            if !close {
                return Err("豆包正在运行，请完全退出后重试，或选择关闭并继续".into());
            }
            self.processes.close()?;
        }
        if self.processes.running()? {
            return Err("豆包仍在运行，未修改登录状态".into());
        }
        Ok(())
    }
    pub fn status(&self) -> Result<Value, String> {
        let _guard = self.lock()?;
        let s = self.load()?;
        let origin = self.origin()?;
        let running = if self.paths.isolated {
            Ok(false)
        } else {
            self.processes.running()
        };
        let (identity, mut recognition) = match profile::live(&self.paths.data) {
            Ok(v) => (v, None),
            Err(e) => (None, Some(e)),
        };
        if self.recovery_file().exists() {
            recognition = Some("上次切换中断，完全退出豆包后重试可恢复原登录".into());
        }
        let active = identity
            .as_ref()
            .and_then(|i| s.accounts.iter().find(|a| a.uid == i.uid));
        Ok(
            json!({"accounts":s.accounts.iter().map(|a|a.meta(active.is_some_and(|v|v.id==a.id),&origin)).collect::<Vec<_>>(),"client":{"name":"豆包","detected":self.paths.data.exists()||self.paths.executable().is_some(),"configured":identity.is_some(),"running":running.as_ref().ok(),"activeAccountId":active.map(|a|&a.id),"identity":identity.as_ref().map(|i|json!({"uid":i.uid,"name":i.name})),"recognitionError":recognition.or_else(||running.err())},"capabilities":{"nativeCli":false,"tokenStatistics":false,"portableDesktopSession":false}}),
        )
    }
    fn capture(&self, close: bool, relaunch: bool) -> Result<Value, String> {
        let _guard = self.lock()?;
        self.require_closed(close)?;
        self.recover()?;
        let id = profile::live(&self.paths.data)?
            .ok_or("尚未识别到豆包登录，请在官方客户端登录后重试")?;
        let snapshot = profile::capture(&self.paths.data)?;
        profile::validate(&snapshot, true)?;
        if profile::snapshot_identity(&snapshot)?
            .is_none_or(|v| v.uid != id.uid || v.session != id.session)
        {
            return Err("豆包登录在读取时发生变化，请重试".into());
        }
        let mut store = self.load()?;
        let a = self.put(&mut store, &id, Some(&snapshot))?;
        self.save(&store)?;
        let mut result = a.meta(true, &self.origin()?);
        if relaunch {
            if let Err(e) = self.processes.launch(&self.paths) {
                result["warning"] = json!(e);
            }
        }
        Ok(result)
    }
    fn switch(
        &self,
        id: &str,
        close: bool,
        relaunch: bool,
        dry_run: bool,
        expected: Option<&str>,
    ) -> Result<Value, String> {
        let _guard = self.lock()?;
        let mut store = self.load()?;
        let a = store
            .accounts
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or("豆包账号不存在")?;
        let target = self.read_snapshot(&a)?;
        if dry_run {
            return Ok(
                json!({"dryRun":true,"accountId":id,"running":self.processes.running()?,"requiresClose":true,"switchable":true}),
            );
        }
        self.require_closed(close)?;
        self.recover()?;
        let live = profile::live(&self.paths.data)?;
        if expected.is_some_and(|uid| live.as_ref().is_none_or(|i| i.uid != uid)) {
            return Err("豆包当前账号已变化，已取消自动轮换".into());
        }
        let previous = profile::capture(&self.paths.data)?;
        if let Some(live) = live.as_ref() {
            self.put(&mut store, live, Some(&previous))?;
            self.save(&store)?;
        }
        let backup = self
            .vault
            .seal(&serde_json::to_vec(&previous).map_err(|_| "无法备份豆包登录")?)?;
        write(&self.recovery_file(), &backup)?;
        let result = profile::restore(&self.paths.data, &target).and_then(|_| {
            if profile::live(&self.paths.data)?.is_none_or(|v| v.uid != a.uid) {
                Err("豆包切换后身份校验失败".into())
            } else {
                Ok(())
            }
        });
        if let Err(e) = result {
            self.recover()
                .map_err(|_| "豆包切换失败且恢复尚未完成，请保留恢复文件并重试")?;
            return Err(format!("{e}；原登录已恢复"));
        }
        let backups = self.paths.store.join("backups");
        write(
            &backups.join(format!("{}-{}.json", now(), uuid::Uuid::new_v4().simple())),
            &backup,
        )?;
        std::fs::remove_file(self.recovery_file())
            .map_err(|_| "登录已切换，但无法完成恢复记录清理")?;
        if let Ok(entries) = std::fs::read_dir(backups) {
            let mut entries = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|v| v == "json"))
                .collect::<Vec<_>>();
            entries.sort();
            let count = entries.len().saturating_sub(5);
            for p in entries.into_iter().take(count) {
                let _ = std::fs::remove_file(p);
            }
        }
        let mut out = json!({"ok":true,"accountId":id,"verified":true});
        if relaunch {
            if let Err(e) = self.processes.launch(&self.paths) {
                out["warning"] = json!(e);
            }
        }
        Ok(out)
    }
    async fn credit(&self, id: &str, timeout: Option<u64>) -> Result<Value, String> {
        let (a, identity) = {
            let _g = self.lock()?;
            let a = self
                .load()?
                .accounts
                .into_iter()
                .find(|a| a.id == id)
                .ok_or("豆包账号不存在")?;
            let identity = self.credential(&a)?;
            (a, identity)
        };
        let result = network::credits(&identity, timeout).await;
        let _g = self.lock()?;
        let mut s = self.load()?;
        let Some(index) = s
            .accounts
            .iter()
            .position(|v| v.id == id && v.credential == a.credential)
        else {
            return Err("豆包账号授权已变化，请重试查询".into());
        };
        match result {
            Ok(credit) => {
                s.accounts[index].credits = Some(credit.clone());
                s.accounts[index].needs_login = false;
                s.snapshots.retain(|v| {
                    v["at"].as_i64().is_some_and(|at| at >= now() - 90 * 86400)
                        && !(v["accountId"] == id && v["at"] == now())
                });
                s.snapshots
                    .push(json!({"accountId":id,"name":a.name,"at":now(),"credit":credit}));
                if s.snapshots.len() > 5000 {
                    s.snapshots.drain(..s.snapshots.len() - 5000);
                }
                self.save(&s)?;
                Ok(credit)
            }
            Err(e) => {
                if network::auth_error(&e) {
                    s.accounts[index].needs_login = true;
                    self.save(&s)?;
                }
                Err(self.auth_failure(&a, e))
            }
        }
    }
    async fn checkin(&self, id: &str, submit: bool, timeout: Option<u64>) -> Result<Value, String> {
        let _claim = StateLock::try_at(
            &self
                .paths
                .store
                .join(format!("reward-{}.lock", hash(id.as_bytes()))),
        )
        .map_err(|_| "该豆包账号正在领取，请稍后查看结果")?;
        let (a, identity) = {
            let _g = self.lock()?;
            let a = self
                .load()?
                .accounts
                .into_iter()
                .find(|a| a.id == id)
                .ok_or("豆包账号不存在")?;
            let identity = self.credential(&a)?;
            (a, identity)
        };
        let mut result = network::checkin(&identity, submit, timeout)
            .await
            .map_err(|e| self.auth_failure(&a, e))?;
        {
            let _g = self.lock()?;
            let mut s = self.load()?;
            if let Some(v) = s
                .accounts
                .iter_mut()
                .find(|v| v.id == id && v.credential == a.credential)
            {
                v.checkin = Some(result.clone());
                self.save(&s)?;
            }
        }
        if submit && result["ok"] == true {
            match self.credit(id, timeout).await {
                Ok(v) => result["balance"] = v,
                Err(e) => result["warning"] = json!(e),
            }
        }
        Ok(result)
    }
    async fn statistics(&self, refresh: bool) -> Result<Value, String> {
        let ids = {
            let _g = self.lock()?;
            self.load()?
                .accounts
                .into_iter()
                .map(|a| a.id)
                .collect::<Vec<_>>()
        };
        let mut errors = vec![];
        if refresh {
            for id in ids {
                if let Err(e) = self.credit(&id, None).await {
                    errors.push(json!({"accountId":id,"message":e}));
                }
            }
        }
        let _g = self.lock()?;
        let s = self.load()?;
        let origin = self.origin()?;
        Ok(
            json!({"generatedAt":now(),"retentionDays":90,"accounts":s.accounts.iter().map(|a|a.meta(false,&origin)).collect::<Vec<_>>(),"snapshots":s.snapshots.into_iter().filter(|s|s["at"].as_i64().is_some_and(|at|at>=now()-90*86400)).collect::<Vec<_>>(),"errors":errors}),
        )
    }
    async fn action(&self, action: &str, input: Value) -> Result<Value, String> {
        let args: Args =
            serde_json::from_value(input).map_err(|_| "豆包操作参数包含未知字段或类型错误")?;
        let id = args.account_id.as_deref().unwrap_or("");
        match action {
            "capture" => self.capture(
                args.close_running.unwrap_or(false),
                args.relaunch.unwrap_or(false),
            ),
            "switch" => self.switch(
                id,
                args.close_running.unwrap_or(false),
                args.relaunch.unwrap_or(false),
                args.dry_run.unwrap_or(false),
                None,
            ),
            "open" => {
                self.processes.launch(&self.paths)?;
                Ok(json!({"ok":true}))
            }
            "rename" | "delete" => {
                let _g = self.lock()?;
                let mut s = self.load()?;
                let a = s
                    .accounts
                    .iter_mut()
                    .find(|a| a.id == id)
                    .ok_or("豆包账号不存在")?;
                if action == "rename" {
                    let name = args.name.as_deref().unwrap_or("").trim();
                    if name.is_empty() || name.chars().count() > 80 {
                        return Err("名称须为 1–80 个字符".into());
                    }
                    a.name = name.into();
                    a.custom_name = true;
                } else {
                    s.accounts.retain(|a| a.id != id);
                    s.snapshots.retain(|v| v["accountId"] != id);
                }
                self.save(&s)?;
                Ok(json!({"ok":true}))
            }
            "credits" => self.credit(id, args.timeout_seconds).await,
            "checkin" | "checkin_status" => {
                self.checkin(id, action == "checkin", args.timeout_seconds)
                    .await
            }
            "statistics" => self.statistics(args.refresh.unwrap_or(false)).await,
            "token_stats" => Ok(
                json!({"available":false,"reason":"豆包未提供可核验的本机 Token 统计来源，额度百分比不能换算成 Token"}),
            ),
            "get_settings" => automation::status(self),
            "save_settings" => automation::save(self, args.settings.ok_or("缺少设置")?),
            "automation_run" => automation::run(self).await,
            "backup_export" => self.export_backup(
                args.passphrase.as_deref().unwrap_or(""),
                args.path.as_deref(),
            ),
            "backup_preview" => self.preview_backup(
                args.data.as_deref().unwrap_or(""),
                args.passphrase.as_deref().unwrap_or(""),
            ),
            "backup_import" => self.import_backup(
                args.data.as_deref().unwrap_or(""),
                args.passphrase.as_deref().unwrap_or(""),
                args.snapshot_id.as_deref().unwrap_or(""),
            ),
            "cloud_backup" | "cloud_preview" | "cloud_restore" | "cloud_sync" => {
                crate::modules::webdav_sync::doubao_cloud(
                    self,
                    action,
                    args.passphrase.as_deref().unwrap_or(""),
                    args.snapshot_id.as_deref(),
                )
                .await
            }
            _ => Err("未知豆包操作".into()),
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    account_id: Option<String>,
    name: Option<String>,
    close_running: Option<bool>,
    relaunch: Option<bool>,
    dry_run: Option<bool>,
    timeout_seconds: Option<u64>,
    refresh: Option<bool>,
    settings: Option<Value>,
    passphrase: Option<String>,
    data: Option<String>,
    snapshot_id: Option<String>,
    path: Option<String>,
}
pub fn status() -> Result<Value, String> {
    Engine::real().status()
}
pub async fn action(action: &str, input: Value) -> Result<Value, String> {
    let runtime = tokio::runtime::Handle::current();
    let name = action.to_string();
    tokio::task::spawn_blocking(move || runtime.block_on(Engine::real().action(&name, input)))
        .await
        .map_err(|_| "豆包操作异常结束".to_string())?
}
