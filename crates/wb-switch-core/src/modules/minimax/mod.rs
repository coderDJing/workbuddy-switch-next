//! MiniMax Code 账号管理：识别本机登录、加密保存账号包、整组切换与额度查询。
//!
//! 桌面端与 CLI 共用 `~/.minimax` 的一份登录态，因此只有一个切换目标、两个区域。
//! 凭据是明文 JSON，账号包必须由本工具加密保存；切换时两份文件成组写入并提升代际，
//! 否则官方客户端会把自己的登录判成 `error`。
//!
//! 凭据布局与代际约定参考官方开源实现 `MiniMax-AI/minimax-code`（MIT）；
//! 刷新与额度字段另有 `VICqiufeeng/dsh-minimax-connect`（MIT）的实测记录。
pub mod automation;
pub(crate) mod backup;
mod checkin;
mod credits;
mod identity;
mod insights;
mod oauth;
mod paths;
mod proc;
mod protocol;
mod sign;
#[cfg(test)]
mod tests;
mod usage;
mod vault;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::modules::state_lock::StateLock;

use paths::Paths;
pub use paths::Region;
use proc::{NativeProcesses, Processes};
use protocol::Payload;
use vault::{SystemVault, Vault};

const STORE_VERSION: u32 = 1;
const KEEP_BACKUPS: usize = 10;
const CLOSE_TIMEOUT: Duration = Duration::from_secs(20);
/// 访问令牌在到期前这么多毫秒就认为需要刷新，避免请求发出时刚好过期。
const REFRESH_MARGIN_MS: i64 = 60_000;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn short_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Account {
    id: String,
    /// 平台侧用户 id（`/api/v1/...` 与桌面端配置里的 uid）；读不到身份时退化成 `epoch:<loginEpoch>`。
    /// 重新登录后 `epoch` 会变、这里不变，因此它也用于把同一账号的旧记录替换掉。
    uid: String,
    name: String,
    #[serde(default)]
    email: Option<String>,
    region: Region,
    /// 用于判断现场属于哪个账号；刷新不会改变它。
    epoch: String,
    captured_at: i64,
    #[serde(default)]
    expires_at: Option<i64>,
    #[serde(default)]
    needs_login: bool,
    #[serde(default)]
    custom_name: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credits: Option<Value>,
    /// 加密后的 `{auth, state}`；明文 token 不落盘。
    payload: String,
}

impl Account {
    fn meta(&self, active: bool) -> Value {
        json!({
            "id": self.id,
            "uid": self.uid,
            "name": self.name,
            "email": self.email,
            "region": self.region,
            "capturedAt": self.captured_at,
            "expiresAt": self.expires_at,
            "needsLogin": self.needs_login,
            "active": active,
            "credits": self.credits,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    version: u32,
    #[serde(default)]
    accounts: Vec<Account>,
    #[serde(default)]
    snapshots: Vec<insights::Snapshot>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            accounts: Vec::new(),
            snapshots: Vec::new(),
        }
    }
}

struct Identity {
    uid: String,
    name: Option<String>,
    email: Option<String>,
}

struct Live {
    payload: Payload,
    credential: protocol::Credential,
}

pub struct Engine {
    paths: Paths,
    vault: Box<dyn Vault>,
    processes: Box<dyn Processes>,
}

/// 工具内网页登录（设备码）的一次会话。密钥只留在本进程，不落盘。
struct LoginSession {
    region: Region,
    device_code: String,
    verifier: String,
    expires_at: i64,
    interval_ms: i64,
    next_poll_at: i64,
    result: Option<Value>,
    error: Option<String>,
}

static LOGINS: OnceLock<Mutex<HashMap<String, LoginSession>>> = OnceLock::new();
fn logins() -> &'static Mutex<HashMap<String, LoginSession>> {
    LOGINS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 过期会话的清理由每次 start/poll 顺带完成，不需要单独的后台任务。
fn prune_logins() {
    let now = chrono::Utc::now().timestamp();
    if let Ok(mut map) = logins().lock() {
        map.retain(|_, session| session.expires_at + 60 > now);
    }
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
        StateLock::try_at(&self.paths.store.join("switch.lock"))
            .map_err(|_| "MiniMax 账号正在更新，请稍后重试".into())
    }

    fn lock_for_read(&self) -> Result<StateLock, String> {
        StateLock::wait_at(
            &self.paths.store.join("switch.lock"),
            Duration::from_secs(3),
        )
        .map_err(|_| "MiniMax 账号正在更新，请稍后重试".into())
    }

    fn accounts_file(&self) -> PathBuf {
        self.paths.store.join("accounts.json")
    }

    fn load(&self) -> Result<Store, String> {
        let path = self.accounts_file();
        if !path.exists() {
            return Ok(Store::default());
        }
        let bytes = std::fs::read(&path).map_err(|_| "无法读取 MiniMax 账号库".to_string())?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("MiniMax 账号库超过读取上限，已停止操作".into());
        }
        let store: Store =
            serde_json::from_slice(&bytes).map_err(|_| "MiniMax 账号库损坏，已停止操作")?;
        if store.version != STORE_VERSION {
            return Err("MiniMax 账号库版本不受支持".into());
        }
        Ok(store)
    }

    fn save(&self, store: &Store) -> Result<(), String> {
        let text = serde_json::to_string_pretty(store).map_err(|_| "无法编码 MiniMax 账号库")?;
        if text.len() > 8 * 1024 * 1024 {
            return Err("MiniMax 账号库超过读取上限，未覆盖原文件".into());
        }
        std::fs::create_dir_all(&self.paths.store).map_err(|_| "无法创建 MiniMax 账号目录")?;
        crate::modules::config::atomic_write(&self.accounts_file(), &text)
            .map_err(|_| "无法保存 MiniMax 账号库".into())
    }

    fn seal(&self, payload: &Payload) -> Result<String, String> {
        let bytes = serde_json::to_vec(payload).map_err(|_| "无法编码 MiniMax 账号")?;
        self.vault.seal(&bytes)
    }

    fn unseal(&self, account: &Account) -> Result<Payload, String> {
        let bytes = self.vault.open(&account.payload)?;
        serde_json::from_slice(&bytes)
            .map_err(|_| "MiniMax 账号数据损坏，请重新添加该账号".to_string())
    }

    fn read_identity(&self, region: Region) -> Option<Identity> {
        for path in self.paths.identity_candidates(region) {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            let Some(user) = value
                .get("sharedUser")
                .filter(|value| value.is_object())
                .or_else(|| value.get("user").filter(|value| value.is_object()))
            else {
                continue;
            };
            let get = |key: &str| {
                user.get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_string)
            };
            let uid = get("realUserID").or_else(|| get("userID"));
            let Some(uid) = uid else { continue };
            return Some(Identity {
                uid,
                name: get("subUserName").or_else(|| get("userName")),
                email: get("email"),
            });
        }
        None
    }

    fn live(&self, region: Region) -> Result<Option<Live>, String> {
        let auth_path = self.paths.auth_file(region);
        let state_path = self.paths.state_file(region);
        let (Ok(auth_bytes), Ok(state_bytes)) =
            (std::fs::read(&auth_path), std::fs::read(&state_path))
        else {
            return Ok(None);
        };
        if auth_bytes.is_empty() || state_bytes.is_empty() {
            return Ok(None);
        }
        let payload = Payload::from_bytes(&auth_bytes, &state_bytes)?;
        let credential = protocol::parse(&payload, region)?;
        let _ = (auth_bytes.len(), state_bytes.len());
        Ok(Some(Live {
            payload,
            credential,
        }))
    }

    /// 现场的登录对应库里哪张卡：先按代际精确匹配，代际对不上再按平台 uid 认。
    ///
    /// 只按代际会让「重新登录过的同一账号」显示成两张卡（一张「本机当前登录：尚未添加」
    /// 加一张账号卡）。注意这**只影响展示与 `active` 标记**：真正写入仍以代际为准，
    /// 所以这种情况下点「切换」依然会执行，把新令牌同步进客户端。
    fn active_account_id<'a>(
        &self,
        store: &'a Store,
        region: Region,
        live: Option<&Live>,
        live_uid: Option<&str>,
    ) -> Option<&'a str> {
        let epoch = live?.credential.epoch();
        let by_epoch = store
            .accounts
            .iter()
            .find(|account| account.region == region && account.epoch == epoch);
        if let Some(account) = by_epoch {
            return Some(account.id.as_str());
        }
        let live_uid = live_uid.filter(|uid| !uid.is_empty())?;
        // 占位身份（`epoch:<uuid>`）不是平台 uid，不能拿来认人。
        store
            .accounts
            .iter()
            .find(|account| {
                account.region == region
                    && account.uid == live_uid
                    && !account.uid.starts_with("epoch:")
            })
            .map(|account| account.id.as_str())
    }

    fn upsert(
        &self,
        store: &mut Store,
        region: Region,
        live: &Live,
        identity: Option<&Identity>,
    ) -> Result<Account, String> {
        let epoch = live.credential.epoch();
        // 同一账号在客户端重新登录会换 `loginEpoch`：先按代际匹配，再按平台用户 id 匹配，
        // 否则重新登录一次就会多出一张只带失效令牌的旧卡片。
        let live_uid = identity
            .map(|value| value.uid.as_str())
            .filter(|uid| !uid.is_empty());
        let existing = store
            .accounts
            .iter()
            .position(|account| account.region == region && account.epoch == epoch)
            .or_else(|| {
                live_uid.and_then(|uid| {
                    store
                        .accounts
                        .iter()
                        .position(|account| account.region == region && account.uid == uid)
                })
            });
        let id = existing.map_or_else(
            || format!("mm-{}", short_hash(&format!("{}:{epoch}", region.text()))),
            |index| store.accounts[index].id.clone(),
        );
        let previous = existing.map(|index| store.accounts[index].clone());
        let name = if previous.as_ref().is_some_and(|account| account.custom_name) {
            previous
                .as_ref()
                .map(|account| account.name.clone())
                .unwrap_or_default()
        } else {
            identity
                .and_then(|value| value.name.clone())
                .or_else(|| previous.as_ref().map(|account| account.name.clone()))
                .unwrap_or_else(|| {
                    let count = store
                        .accounts
                        .iter()
                        .filter(|account| account.region == region)
                        .count();
                    format!(
                        "MiniMax {} 账号 {}",
                        region.text().to_uppercase(),
                        count + 1
                    )
                })
        };
        let payload = self.seal(&live.payload)?;
        let account = Account {
            id,
            uid: identity
                .map(|value| value.uid.clone())
                .or_else(|| previous.as_ref().map(|account| account.uid.clone()))
                .unwrap_or_else(|| format!("epoch:{epoch}")),
            name,
            email: identity
                .and_then(|value| value.email.clone())
                .or_else(|| previous.as_ref().and_then(|account| account.email.clone())),
            region,
            epoch,
            captured_at: previous
                .as_ref()
                .map_or_else(now_ms, |account| account.captured_at),
            expires_at: Some(live.credential.expires_at_ms / 1000),
            needs_login: false,
            custom_name: previous.as_ref().is_some_and(|account| account.custom_name),
            credits: previous
                .as_ref()
                .and_then(|account| account.credits.clone()),
            payload,
        };
        match existing {
            Some(index) => store.accounts[index] = account.clone(),
            None => store.accounts.push(account.clone()),
        }
        Ok(account)
    }

    fn find<'a>(&self, store: &'a Store, id: &str) -> Result<&'a Account, String> {
        store
            .accounts
            .iter()
            .find(|account| account.id == id)
            .ok_or_else(|| "MiniMax 账号不存在，请刷新后重试".to_string())
    }

    pub fn status(&self) -> Result<Value, String> {
        let _guard = self.lock()?;
        let store = self.load()?;
        // 每个区域只读一次现场登录与本地用户资料：后面判断「当前账号」两样都要用。
        // 早先这里把现场 JSON 解析了两遍，等于白做一次。
        let mut regions = Vec::new();
        for region in [Region::Cn, Region::En] {
            let configured = self.paths.auth_file(region).is_file();
            let detected = self.paths.installed(region);
            let live = if configured {
                self.live(region)
            } else {
                Ok(None)
            };
            let (live, recognition_error) = match live {
                Ok(value) => (value, None),
                Err(error) => (None, Some(error)),
            };
            let identity = self.read_identity(region);
            regions.push((
                region,
                configured,
                detected,
                live,
                recognition_error,
                identity,
            ));
        }
        // 桌面端是同一个可执行文件，与区域无关：`running` 探测一次就够
        // （Windows 上一次 tasklist 是几百毫秒，两次纯属浪费）。
        let any_detected = regions.iter().any(|(_, _, detected, _, _, _)| *detected);
        let running = if any_detected {
            self.processes.running(Region::Cn).ok()
        } else {
            Some(false)
        };
        let mut active_by_region = std::collections::HashMap::new();
        let mut clients = Vec::new();
        for (region, configured, detected, live, recognition_error, identity) in regions {
            let active = self
                .active_account_id(
                    &store,
                    region,
                    live.as_ref(),
                    identity.as_ref().map(|value| value.uid.as_str()),
                )
                .map(str::to_string);
            active_by_region.insert(region, active.clone());
            let token_expired = live
                .as_ref()
                .is_some_and(|value| value.credential.expired(now_ms()));
            clients.push(json!({
                "region": region,
                "name": region.label(),
                "detected": detected,
                "configured": configured,
                "running": running,
                "activeAccountId": active,
                "tokenExpired": token_expired,
                "expiresAt": live.as_ref().map(|value| value.credential.expires_at_ms / 1000),
                "identity": identity.as_ref().map(|value| json!({
                    "uid": value.uid,
                    "name": value.name,
                    "email": value.email,
                })),
                "recognitionError": recognition_error,
            }));
        }
        let accounts: Vec<Value> = store
            .accounts
            .iter()
            .map(|account| {
                let active = active_by_region
                    .get(&account.region)
                    .and_then(|value| value.as_deref())
                    == Some(account.id.as_str());
                account.meta(active)
            })
            .collect();
        Ok(json!({
            "accounts": accounts,
            "clients": clients,
            "sharedLogin": true,
        }))
    }

    fn capture(&self, region: Region) -> Result<Value, String> {
        let _guard = self.lock()?;
        let live = self
            .live(region)?
            .ok_or("未检测到 MiniMax Code 登录，请先在客户端登录后再识别")?;
        let identity = self.read_identity(region);
        let mut store = self.load()?;
        let account = self.upsert(&mut store, region, &live, identity.as_ref())?;
        self.save(&store)?;
        Ok(account.meta(true))
    }

    fn backup_live(&self, region: Region, live: &Live) -> Result<PathBuf, String> {
        let dir = self.paths.store.join("backups");
        std::fs::create_dir_all(&dir).map_err(|_| "无法创建 MiniMax 备份目录")?;
        let payload = json!({
            "region": region,
            "auth": live.payload.auth,
            "state": live.payload.state,
        });
        let bytes = serde_json::to_vec(&payload).map_err(|_| "无法编码 MiniMax 备份")?;
        let sealed = self.vault.seal(&bytes)?;
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%9f");
        let path = dir.join(format!(
            "{}-{stamp}-{}.enc",
            region.text(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&path, sealed).map_err(|_| "无法写入 MiniMax 备份")?;
        self.prune_backups(region);
        Ok(path)
    }

    fn prune_backups(&self, region: Region) {
        let dir = self.paths.store.join("backups");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        let prefix = format!("{}-", region.text());
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .map(|entry| entry.path())
            .collect();
        files.sort();
        let excess = files.len().saturating_sub(KEEP_BACKUPS);
        for path in files.into_iter().take(excess) {
            let _ = std::fs::remove_file(path);
        }
    }

    /// 按备份恢复现场；`backup` 为 None 表示切换前本来没有登录态。
    fn restore_live(&self, region: Region, backup: Option<&Path>) -> Result<(), String> {
        match backup {
            Some(path) => {
                let sealed = std::fs::read(path).map_err(|_| "备份文件丢失，无法回滚")?;
                let sealed = String::from_utf8(sealed).map_err(|_| "备份文件损坏，无法回滚")?;
                let bytes = self.vault.open(&sealed)?;
                let value: Value =
                    serde_json::from_slice(&bytes).map_err(|_| "备份内容损坏，无法回滚")?;
                let payload = Payload {
                    auth: value.get("auth").cloned().ok_or("备份缺少凭据")?,
                    state: value.get("state").cloned().ok_or("备份缺少登录状态")?,
                };
                write_bytes(&self.paths.auth_file(region), &payload.auth_bytes()?)?;
                write_bytes(&self.paths.state_file(region), &payload.state_bytes()?)?;
                Ok(())
            }
            None => {
                let _ = std::fs::remove_file(self.paths.auth_file(region));
                let _ = std::fs::remove_file(self.paths.state_file(region));
                Ok(())
            }
        }
    }

    fn switch(
        &self,
        region: Region,
        account_id: &str,
        close_running: bool,
        relaunch: bool,
        dry_run: bool,
    ) -> Result<Value, String> {
        self.switch_checked(region, account_id, close_running, relaunch, dry_run, None)
    }

    fn switch_checked(
        &self,
        region: Region,
        account_id: &str,
        close_running: bool,
        relaunch: bool,
        dry_run: bool,
        expected_epoch: Option<&str>,
    ) -> Result<Value, String> {
        let _guard = self.lock()?;
        let mut store = self.load()?;
        let account = self.find(&store, account_id)?.clone();
        if account.region != region {
            return Err("目标区域与保存的账号不一致，请刷新后重试".into());
        }
        let payload = self.unseal(&account)?;
        let prepared = protocol::prepare(&payload, region, 0)?;
        let credential = prepared.1;
        let token_expired = credential.expired(now_ms());

        let live = self.live(region)?;
        if expected_epoch
            .is_some_and(|epoch| live.as_ref().is_none_or(|v| v.credential.epoch() != epoch))
        {
            return Err("当前账号已变化，已取消自动轮换".into());
        }
        if live
            .as_ref()
            .is_some_and(|value| value.credential.epoch() == account.epoch)
        {
            return Ok(json!({
                "switched": false,
                "alreadyActive": true,
                "accountId": account.id,
                "region": region,
                "message": format!("{} 已经使用该账号", region.label()),
            }));
        }
        let cli_running = self.processes.cli_running()?;
        let running = self.processes.running(region)?;
        if dry_run {
            return Ok(json!({
                "dryRun": true,
                "accountId": account.id,
                "region": region,
                "requiresClose": running || cli_running,
                "running": running,
                "cliRunning": cli_running,
                "tokenExpired": token_expired,
            }));
        }
        if cli_running {
            return Err("mcode CLI 正在运行，与桌面端共用同一份登录态；请先退出 CLI 再切换".into());
        }
        if running && !close_running {
            return Err("MiniMax Code 正在运行，请先保存任务并退出后重试".into());
        }

        let was_running = running;
        let mut warnings: Vec<String> = Vec::new();
        if was_running {
            self.processes.close(region, CLOSE_TIMEOUT)?;
            if self.processes.running(region)? {
                return Err("MiniMax Code 重新启动或拒绝退出，已停止切换".into());
            }
        }
        let restart = |warnings: &mut Vec<String>| {
            if was_running && relaunch && self.processes.launch(&self.paths, region).is_err() {
                warnings.push("账号已切换，但 MiniMax Code 未能重新打开，请手动启动".into());
            }
        };

        // 客户端退出时可能把内存里的新令牌写回；先按最终现场保存，再备份和写入。
        let live = self.live(region)?;
        if let Some(current) = &live {
            if current.credential.epoch() != account.epoch {
                let identity = self.read_identity(region);
                self.upsert(&mut store, region, current, identity.as_ref())?;
                self.save(&store)?;
            }
        }
        let live_generation = live
            .as_ref()
            .map(|value| {
                value.credential.generation.max(
                    value
                        .payload
                        .state
                        .get("generation")
                        .and_then(Value::as_i64)
                        .unwrap_or(0),
                )
            })
            .unwrap_or(0);
        let (payload, credential) = protocol::prepare(&payload, region, live_generation)?;
        let backup = match &live {
            Some(current) => Some(self.backup_live(region, current)?),
            None => None,
        };

        let result = (|| -> Result<(), String> {
            write_bytes(&self.paths.auth_file(region), &payload.auth_bytes()?)?;
            write_bytes(&self.paths.state_file(region), &payload.state_bytes()?)?;
            let auth: Value = serde_json::from_slice(
                &std::fs::read(self.paths.auth_file(region)).map_err(|_| "读回凭据失败")?,
            )
            .map_err(|_| "写入后的凭据不是有效 JSON")?;
            let state: Value = serde_json::from_slice(
                &std::fs::read(self.paths.state_file(region)).map_err(|_| "读回登录状态失败")?,
            )
            .map_err(|_| "写入后的登录状态不是有效 JSON")?;
            protocol::verify_written(&auth, &state, region)?;
            Ok(())
        })();
        if let Err(error) = result {
            let restored = self.restore_live(region, backup.as_deref());
            let tail = if restored.is_ok() {
                "已回滚到切换前状态".to_string()
            } else {
                format!(
                    "回滚失败，备份保留在 {}",
                    backup
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "（无）".into())
                )
            };
            restart(&mut warnings);
            return Err(format!("切换失败：{error}，{tail}"));
        }
        if let Some(saved) = store.accounts.iter_mut().find(|item| item.id == account.id) {
            saved.expires_at = Some(credential.expires_at_ms / 1000);
            saved.needs_login = false;
        }
        self.save(&store)?;
        restart(&mut warnings);
        if credential.expired(now_ms()) {
            warnings.push("该账号的访问令牌已过期，客户端启动后会自行刷新".into());
        }
        Ok(json!({
            "switched": true,
            "accountId": account.id,
            "label": account.name,
            "region": region,
            "closed": was_running,
            "warnings": warnings,
        }))
    }

    fn rename(&self, account_id: &str, name: &str) -> Result<Value, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("名称不能为空".into());
        }
        if name.chars().count() > 80 {
            return Err("名称过长（最多 80 个字符）".into());
        }
        let _guard = self.lock()?;
        let mut store = self.load()?;
        let live_by_region = |region: Region| {
            self.live(region)
                .ok()
                .flatten()
                .map(|live| live.credential.epoch())
        };
        let index = store
            .accounts
            .iter()
            .position(|account| account.id == account_id)
            .ok_or("MiniMax 账号不存在")?;
        store.accounts[index].name = name.to_string();
        store.accounts[index].custom_name = true;
        let account = store.accounts[index].clone();
        self.save(&store)?;
        Ok(account.meta(live_by_region(account.region).as_deref() == Some(account.epoch.as_str())))
    }

    fn delete(&self, account_id: &str) -> Result<Value, String> {
        let _guard = self.lock()?;
        let mut store = self.load()?;
        let before = store.accounts.len();
        store.accounts.retain(|account| account.id != account_id);
        if store.accounts.len() == before {
            return Err("MiniMax 账号不存在".into());
        }
        self.save(&store)?;
        Ok(json!({"deleted": true, "accountId": account_id}))
    }

    fn open(&self, region: Region) -> Result<Value, String> {
        if self.processes.running(region)? {
            return Ok(json!({"opened": false, "running": true}));
        }
        self.processes.launch(&self.paths, region)?;
        Ok(json!({"opened": true}))
    }

    fn mark_needs_login(&self, account_id: &str) {
        let Ok(_guard) = self.lock() else {
            return;
        };
        let Ok(mut store) = self.load() else {
            return;
        };
        if let Some(account) = store.accounts.iter_mut().find(|item| item.id == account_id) {
            account.needs_login = true;
            let _ = self.save(&store);
        }
    }

    fn cache_credits(&self, account_id: &str, credits: Value) -> Result<(), String> {
        let _guard = self.lock_for_read()?;
        let mut store = self.load()?;
        let account = store
            .accounts
            .iter_mut()
            .find(|account| account.id == account_id)
            .ok_or("MiniMax 账号已删除")?;
        account.credits = Some(credits.clone());
        let account = account.clone();
        insights::record(&mut store, &account, &credits);
        self.save(&store)
    }

    /// 取出一个「现在就能用」的访问令牌：现场令牌优先（只读），
    /// 否则用账号包里的那份，必要时先刷新。
    async fn usable_credential(
        &self,
        account_id: &str,
    ) -> Result<(Account, protocol::Credential), String> {
        let _credential_guard = StateLock::try_at(
            &self
                .paths
                .store
                .join(format!("credential-{}.lock", short_hash(account_id))),
        )
        .map_err(|_| "该账号正在查询或更新令牌，请稍后重试")?;
        let (account, payload, live_credential, live_generation) = {
            let _guard = self.lock_for_read()?;
            let store = self.load()?;
            let account = self.find(&store, account_id)?.clone();
            let payload = self.unseal(&account)?;
            let live = self.live(account.region)?;
            let is_live = live
                .as_ref()
                .is_some_and(|value| value.credential.epoch() == account.epoch);
            let live_generation = live
                .as_ref()
                .map(|value| {
                    value.credential.generation.max(
                        value
                            .payload
                            .state
                            .get("generation")
                            .and_then(Value::as_i64)
                            .unwrap_or(0),
                    )
                })
                .unwrap_or(0);
            (
                account,
                payload,
                is_live
                    .then(|| live.map(|value| value.credential))
                    .flatten(),
                live_generation,
            )
        };
        let credential = protocol::parse(&payload, account.region)?;
        let at_ms = now_ms();
        let credential = if let Some(current) = live_credential {
            // 现场令牌由客户端维护，本工具只读取，绝不刷新它。
            // 过期只说明客户端还没刷新，不代表登录失效：不标 `needsLogin`，也不影响切换。
            if current.expired(at_ms) {
                return Err("当前账号的访问令牌已过期，请打开 MiniMax Code 让它刷新后重试".into());
            }
            current
        } else if credential.expired(at_ms + REFRESH_MARGIN_MS) {
            // Keep switching and parallel refreshes out of this refresh transaction.
            let _guard = self.lock_for_read()?;
            let mut store = self.load()?;
            if self.find(&store, account_id)?.payload != account.payload {
                return Err("账号授权已更新，请重新查询".into());
            }
            if self
                .live(account.region)?
                .is_some_and(|v| v.credential.epoch() == account.epoch)
            {
                return Err("该账号已切换为当前账号，请由 MiniMax Code 刷新后重试".into());
            }
            let grant = oauth::refresh(account.region, &credential.refresh_token).await?;
            let (updated, refreshed) = protocol::apply_grant(
                &payload,
                account.region,
                &grant.access_token,
                &grant.refresh_token,
                at_ms + grant.expires_in_secs * 1000,
                live_generation,
            )?;
            let saved = store
                .accounts
                .iter_mut()
                .find(|a| a.id == account_id)
                .ok_or("账号已删除")?;
            saved.payload = self.seal(&updated)?;
            saved.expires_at = Some(refreshed.expires_at_ms / 1000);
            saved.needs_login = false;
            self.save(&store)?;
            refreshed
        } else {
            credential
        };
        Ok((account, credential))
    }

    async fn credit(&self, account_id: &str, timeout_secs: Option<u64>) -> Result<Value, String> {
        let (account, credential) = self.usable_credential(account_id).await?;

        match query_credits(account.region, &credential.access_token, timeout_secs).await {
            Ok(value) => {
                self.cache_credits(account_id, value.clone())?;
                Ok(value)
            }
            Err(error) => {
                if error.contains("重新登录") {
                    self.mark_needs_login(account_id);
                }
                Err(error)
            }
        }
    }

    /// 重新问一次业务接口，把缺失的昵称 / 邮箱 / 真实 uid 补齐。
    ///
    /// 用于历史账号（早期版本登录时没有身份接口）以及登录后接口临时不可用的情况。
    async fn resolve_identity(&self, account_id: &str) -> Result<Value, String> {
        let (account, credential) = self.usable_credential(account_id).await?;
        let (profile, probe) = identity::fetch(account.region, &credential.access_token).await;
        let profile = profile.ok_or_else(|| {
            format!(
                "无法从 MiniMax 业务接口读取账号身份（{}）",
                probe.notes.join("；")
            )
        })?;
        let live_epoch = self
            .live(account.region)
            .ok()
            .flatten()
            .map(|value| value.credential.epoch());
        let _guard = self.lock()?;
        let mut store = self.load()?;
        let stored = store
            .accounts
            .iter_mut()
            .find(|item| item.id == account.id)
            .ok_or("MiniMax 账号已删除")?;
        // uid 只从占位符升级，不覆盖已经拿到的真身份。
        if stored.uid.starts_with("epoch:") {
            if let Some(uid) = profile.uid.clone() {
                stored.uid = uid;
            }
        }
        if !stored.custom_name {
            if let Some(name) = profile.name.clone() {
                stored.name = name;
            }
        }
        if stored.email.is_none() {
            stored.email = profile.email.clone();
        }
        let uid = stored.uid.clone();
        let mut meta = stored.meta(false);
        // 早期版本登录时拿不到身份，只能按 `epoch:` 建卡；同一个平台账号因此可能留下
        // 两张卡片。身份一旦对上就合并：优先保留现场那张（客户端的「当前账号」指向它），
        // 否则保留刚补全身份的这一张。
        if !uid.starts_with("epoch:") {
            let keep_id = live_epoch
                .and_then(|epoch| {
                    store
                        .accounts
                        .iter()
                        .find(|item| {
                            item.region == account.region && item.uid == uid && item.epoch == epoch
                        })
                        .map(|item| item.id.clone())
                })
                .unwrap_or_else(|| account.id.clone());
            let before = store.accounts.len();
            drop_same_uid(&mut store, account.region, &uid, &keep_id);
            if store.accounts.len() != before {
                meta = store
                    .accounts
                    .iter()
                    .find(|item| item.id == keep_id)
                    .map(|item| item.meta(false))
                    .unwrap_or(meta);
                meta["mergedDuplicates"] = json!(before - store.accounts.len());
            }
        }
        self.save(&store)?;
        meta["identityProbe"] = json!(probe.notes);
        Ok(meta)
    }

    pub async fn action(&self, action: &str, input: Value) -> Result<Value, String> {
        let args: Args =
            serde_json::from_value(input).map_err(|_| "MiniMax 参数不合法或包含未知字段")?;
        let region = args.region.unwrap_or(Region::Cn);
        match action {
            "checkin" => {
                self.checkin(
                    args.account_id.as_deref().ok_or("缺少账号")?,
                    args.timeout_seconds,
                )
                .await
            }
            "checkin_status" => {
                self.checkin_status(
                    args.account_id.as_deref().ok_or("缺少账号")?,
                    args.timeout_seconds,
                )
                .await
            }
            "statistics" => self.statistics(region, args.refresh.unwrap_or(false)).await,
            "token_stats" => usage::statistics(&self.paths.home.join(".minimax/v2/sessions")),
            "get_settings" => automation::status(self),
            "save_settings" => automation::save(self, args.settings.ok_or("缺少设置")?),
            "automation_run" => automation::run(self, true, args.region).await,
            "backup_export" => self.export_backup(
                args.passphrase.as_deref().ok_or("缺少备份密码")?,
                args.path.as_deref(),
            ),
            "backup_preview" => self.preview_backup(
                args.data.as_deref().ok_or("缺少备份")?,
                args.passphrase.as_deref().ok_or("缺少备份密码")?,
            ),
            "backup_import" => self.import_backup(
                args.data.as_deref().ok_or("缺少备份")?,
                args.passphrase.as_deref().ok_or("缺少备份密码")?,
                args.snapshot_id.as_deref().ok_or("请先预览备份")?,
            ),
            "cloud_backup" | "cloud_preview" | "cloud_restore" | "cloud_sync" => {
                crate::modules::webdav_sync::minimax_cloud(
                    self,
                    action,
                    args.passphrase.as_deref().ok_or("缺少备份密码")?,
                    args.snapshot_id.as_deref(),
                )
                .await
            }
            "capture" => self.capture(region),
            "switch" => self.switch(
                region,
                args.account_id.as_deref().ok_or("缺少账号")?,
                args.close_running.unwrap_or(false),
                args.relaunch.unwrap_or(false),
                args.dry_run.unwrap_or(false),
            ),
            "credits" => {
                self.credit(
                    args.account_id.as_deref().ok_or("缺少账号")?,
                    args.timeout_seconds,
                )
                .await
            }
            "identity" => {
                self.resolve_identity(args.account_id.as_deref().ok_or("缺少账号")?)
                    .await
            }
            "rename" => self.rename(
                args.account_id.as_deref().ok_or("缺少账号")?,
                args.name.as_deref().ok_or("缺少名称")?,
            ),
            "delete" => self.delete(args.account_id.as_deref().ok_or("缺少账号")?),
            "open" => self.open(region),
            "login_start" => self.login_start(region).await,
            "login_poll" => {
                self.login_poll(args.login_id.as_deref().ok_or("缺少登录会话")?)
                    .await
            }
            "login_cancel" => {
                self.login_cancel(args.login_id.as_deref().ok_or("缺少登录会话")?);
                Ok(json!({"ok": true}))
            }
            _ => Err("未知的 MiniMax 操作".into()),
        }
    }

    /// 发起设备码授权：返回用户码与授权地址，密钥（verifier / device_code）只留在本进程。
    pub async fn login_start(&self, region: Region) -> Result<Value, String> {
        prune_logins();
        let (device, verifier) = oauth::device_start(region).await?;
        let login_id = uuid::Uuid::new_v4().to_string();
        let interval_ms = (device.interval_secs * 1000).max(1000);
        let session = LoginSession {
            region,
            device_code: device.device_code,
            verifier,
            expires_at: chrono::Utc::now().timestamp() + device.expires_in_secs,
            interval_ms,
            // 首次轮询也要隔一个间隔，否则授权服务会直接回 `slow_down`。
            next_poll_at: chrono::Utc::now().timestamp_millis() + interval_ms,
            result: None,
            error: None,
        };
        logins()
            .lock()
            .map_err(|_| "MiniMax 登录会话不可用".to_string())?
            .insert(login_id.clone(), session);
        Ok(json!({
            "loginId": login_id,
            "region": region,
            "userCode": device.user_code,
            "verificationUri": device.verification_uri,
            "verificationUriComplete": device.verification_uri_complete,
            "expiresIn": device.expires_in_secs,
            "intervalMs": interval_ms,
        }))
    }

    /// 轮询一次授权结果；已授权时把凭据封进账号包（不写现场），由用户显式切换。
    pub async fn login_poll(&self, login_id: &str) -> Result<Value, String> {
        let (region, device_code, verifier, finished) = {
            let mut map = logins()
                .lock()
                .map_err(|_| "MiniMax 登录会话不可用".to_string())?;
            let session = map
                .get_mut(login_id)
                .ok_or("MiniMax 登录会话已结束，请重新发起登录")?;
            if let Some(result) = session.result.clone() {
                (
                    session.region,
                    String::new(),
                    String::new(),
                    Some(Ok(result)),
                )
            } else if let Some(error) = session.error.clone() {
                (
                    session.region,
                    String::new(),
                    String::new(),
                    Some(Err(error)),
                )
            } else if chrono::Utc::now().timestamp() > session.expires_at {
                map.remove(login_id);
                return Ok(json!({"status": "expired", "error": "授权码已过期，请重新发起登录"}));
            } else {
                let now = chrono::Utc::now().timestamp_millis();
                if now < session.next_poll_at {
                    return Ok(
                        json!({"status": "pending", "retryAfterMs": session.next_poll_at - now}),
                    );
                }
                session.next_poll_at = now + session.interval_ms;
                (
                    session.region,
                    session.device_code.clone(),
                    session.verifier.clone(),
                    None,
                )
            }
        };
        if let Some(finished) = finished {
            return match finished {
                Ok(account) => Ok(json!({"status": "authorized", "account": account})),
                Err(error) => Ok(json!({"status": "failed", "error": error})),
            };
        }

        match oauth::device_poll(region, &device_code, &verifier).await? {
            oauth::Poll::Pending => Ok(json!({"status": "pending"})),
            oauth::Poll::SlowDown => {
                if let Ok(mut map) = logins().lock() {
                    if let Some(session) = map.get_mut(login_id) {
                        session.interval_ms = (session.interval_ms * 2).min(30_000);
                        session.next_poll_at =
                            chrono::Utc::now().timestamp_millis() + session.interval_ms;
                    }
                }
                Ok(json!({"status": "pending", "slowDown": true}))
            }
            oauth::Poll::Denied(message)
            | oauth::Poll::Expired(message)
            | oauth::Poll::Failed(message) => {
                if let Ok(mut map) = logins().lock() {
                    map.remove(login_id);
                }
                Ok(json!({"status": "failed", "error": message}))
            }
            oauth::Poll::Authorized(grant) => {
                let expires_at_ms =
                    chrono::Utc::now().timestamp_millis() + grant.expires_in_secs * 1000;
                // 授权响应几乎从不带 `sub`；用刚拿到的新鲜令牌问一次业务接口补身份，
                // 否则账号卡片只能显示「账号 N / epoch:<uuid>」。
                let (profile, probe) = identity::fetch(region, &grant.access_token).await;
                let payload = protocol::fresh(
                    region,
                    &self.paths.credential_key(region),
                    &grant.access_token,
                    &grant.refresh_token,
                    expires_at_ms,
                    &uuid::Uuid::new_v4().to_string(),
                );
                let done = self.finish_login(region, payload, grant.subject, profile, probe);
                if let Ok(mut map) = logins().lock() {
                    if let Some(session) = map.get_mut(login_id) {
                        match &done {
                            Ok(account) => session.result = Some(account.clone()),
                            Err(error) => session.error = Some(error.clone()),
                        }
                    }
                }
                match done {
                    Ok(account) => Ok(json!({"status": "authorized", "account": account})),
                    Err(error) => Ok(json!({"status": "failed", "error": error})),
                }
            }
        }
    }

    pub fn login_cancel(&self, login_id: &str) {
        if let Ok(mut map) = logins().lock() {
            map.remove(login_id);
        }
    }

    /// 把一次新登录封装成账号包。
    ///
    /// 身份优先取授权响应的 `sub`，其次取业务接口的 uid；两者都没有才退回
    /// `epoch:<loginEpoch>`（此时昵称/邮箱若能拿到仍然会补上）。
    fn finish_login(
        &self,
        region: Region,
        payload: Payload,
        subject: Option<String>,
        profile: Option<identity::Identity>,
        probe: identity::Probe,
    ) -> Result<Value, String> {
        let credential = protocol::parse(&payload, region)?;
        let profile = profile.unwrap_or_default();
        let uid = subject.or(profile.uid);
        let identity = uid.map(|uid| Identity {
            uid,
            name: profile.name.clone(),
            email: profile.email.clone(),
        });
        let live = Live {
            payload,
            credential,
        };
        let _guard = self.lock()?;
        let mut store = self.load()?;
        let account = self.upsert(&mut store, region, &live, identity.as_ref())?;
        // 没有 uid 时，`upsert` 只能用占位符当身份；昵称和邮箱仍然值得写进去，
        // 否则用户在一堆「账号 N」之间根本分不清谁是谁。
        let account = if identity.is_none() {
            match store.accounts.iter_mut().find(|item| item.id == account.id) {
                Some(stored) => {
                    if !stored.custom_name {
                        if let Some(name) = profile.name.clone() {
                            stored.name = name;
                        }
                    }
                    if stored.email.is_none() {
                        stored.email = profile.email.clone();
                    }
                    stored.meta(false)
                }
                None => account.meta(false),
            }
        } else {
            account.meta(false)
        };
        self.save(&store)?;
        let mut account = account;
        account["identityProbe"] = json!(probe.notes);
        // 新登录如果是客户端当前登录的那个账号，它就不再是「当前」了：客户端槽里
        // 还是旧令牌，记录里是新令牌。界面需要说清楚这一点，否则用户会以为是重复卡。
        let live_uid = self.read_identity(region).map(|live| live.uid);
        let matches_live = live_uid.as_deref() == account["uid"].as_str();
        account["matchesLiveClient"] = json!(matches_live);
        // 与 `status()` 同一套口径：客户端确实登录着这个账号，就该显示为当前，
        // 否则界面会出现「本机当前登录：尚未添加」加一张同名卡片。
        account["active"] = json!(matches_live);
        Ok(account)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    #[serde(default)]
    region: Option<Region>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    login_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    close_running: Option<bool>,
    #[serde(default)]
    relaunch: Option<bool>,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    refresh: Option<bool>,
    settings: Option<Value>,
    passphrase: Option<String>,
    data: Option<String>,
    snapshot_id: Option<String>,
    path: Option<String>,
}

/// 删掉同区域、同 uid、但不是 `keep_id` 的重复卡片，返回删除条数。
fn drop_same_uid(store: &mut Store, region: Region, uid: &str, keep_id: &str) -> usize {
    let before = store.accounts.len();
    store
        .accounts
        .retain(|item| item.id == keep_id || item.region != region || item.uid != uid);
    before - store.accounts.len()
}

/// 查询额度，并按 `--timeout` 给出整体上限（默认沿用 HTTP 客户端的 30 秒）。
async fn query_credits(
    region: Region,
    token: &str,
    timeout_secs: Option<u64>,
) -> Result<Value, String> {
    let Some(secs) = timeout_secs.map(|secs| secs.clamp(5, 600)) else {
        return credits::query(region, token).await;
    };
    match tokio::time::timeout(
        std::time::Duration::from_secs(secs),
        credits::query(region, token),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(format!("积分查询超时（{secs} 秒）")),
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "无法创建 MiniMax 数据目录")?;
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!("{name}.tmp-{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&tmp, bytes).map_err(|_| format!("无法写入 {name}"))?;
    std::fs::rename(&tmp, path)
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
        .map_err(|_| format!("无法替换 {name}"))
}

/// 全局入口：读取状态（后端命令用 `spawn_blocking` 调用）。
pub fn status() -> Result<Value, String> {
    Engine::real().status()
}

pub async fn action(action: &str, input: Value) -> Result<Value, String> {
    let runtime = tokio::runtime::Handle::current();
    let action = action.to_string();
    tokio::task::spawn_blocking(move || runtime.block_on(Engine::real().action(&action, input)))
        .await
        .map_err(|_| "MiniMax 操作异常结束".to_string())?
}
