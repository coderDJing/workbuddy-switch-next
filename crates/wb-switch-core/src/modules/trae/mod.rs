//! TRAE desktop accounts. Credentials are independent of other platforms;
//! restoring an account touches only client authentication keys.
pub mod automation;
pub(crate) mod backup;
mod credits;
mod insights;
mod paths;
mod proc;
mod protocol;
mod vault;
use crate::modules::state_lock::StateLock;
use paths::Paths;
pub use paths::{Client, Region};
use proc::{NativeProcesses, Processes};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use vault::{SystemVault, Vault};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Account {
    id: String,
    uid: String,
    name: String,
    email: Option<String>,
    variant: Region,
    client: Client,
    captured_at: i64,
    expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    credits: Option<Value>,
    payload: String,
}
impl Account {
    fn meta(&self) -> Value {
        json!({"id":self.id,"uid":self.uid,"name":self.name,"email":self.email,
            "variant":self.variant,"client":self.client,"capturedAt":self.captured_at,
            "expiresAt":self.expires_at,"needsLogin":self.expires_at.is_some_and(|t|t<=chrono::Utc::now().timestamp()),"credits":self.credits})
    }
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    version: u32,
    accounts: Vec<Account>,
    #[serde(default)]
    snapshots: Vec<insights::Snapshot>,
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
        StateLock::try_at(&self.paths.store.join("switch.lock"))
            .map_err(|_| "TRAE 账号正在更新，请稍后重试".into())
    }
    fn load(&self) -> Result<Store, String> {
        let path = self.paths.store.join("accounts.json");
        if !path.exists() {
            return Ok(Store {
                version: 1,
                ..Store::default()
            });
        }
        let store: Store = serde_json::from_slice(&read_limited(&path)?)
            .map_err(|_| "TRAE 账号库损坏，已停止操作；请从备份恢复".to_string())?;
        if store.version != 1 {
            return Err("TRAE 账号库版本不受支持".into());
        }
        let mut ids = std::collections::HashSet::new();
        if store.accounts.iter().any(|a| !ids.insert(&a.id)) {
            return Err("TRAE 账号库存在重复 ID".into());
        }
        Ok(store)
    }
    fn save(&self, store: &Store) -> Result<(), String> {
        let text = serde_json::to_string_pretty(store).map_err(|_| "无法编码 TRAE 账号库")?;
        if text.len() > 16 * 1024 * 1024 {
            return Err("TRAE 账号库超过读取上限，未覆盖原文件".into());
        }
        write_private(&self.paths.store.join("accounts.json"), &text)
            .map_err(|_| "无法保存 TRAE 账号库".into())
    }
    fn storage(&self, region: Region, client: Client) -> Result<Value, String> {
        serde_json::from_slice(&read_limited(&self.paths.storage(region, client))?)
            .map_err(|_| "TRAE storage.json 已损坏，未修改".into())
    }
    fn upsert(
        &self,
        store: &mut Store,
        region: Region,
        client: Client,
        keys: &Map<String, Value>,
    ) -> Result<String, String> {
        let auth = protocol::identity(keys)?;
        let id = format!(
            "trae-{:x}",
            Sha256::digest(format!("{}:{}:{}", region.text(), client.text(), auth.uid).as_bytes())
        );
        let payload = self
            .vault
            .seal(&serde_json::to_vec(keys).map_err(|_| "无法编码 TRAE 账号")?)?;
        if let Some(a) = store.accounts.iter_mut().find(|a| a.id == id) {
            a.payload = payload;
            a.expires_at = auth.expires;
            a.captured_at = chrono::Utc::now().timestamp();
        } else {
            store.accounts.push(Account {
                id: id.clone(),
                uid: auth.uid,
                name: auth.name,
                email: auth.email,
                variant: region,
                client,
                captured_at: chrono::Utc::now().timestamp(),
                expires_at: auth.expires,
                credits: None,
                payload,
            });
        }
        Ok(id)
    }
    fn keys(&self, a: &Account) -> Result<Map<String, Value>, String> {
        let keys: Map<String, Value> = serde_json::from_slice(&self.vault.open(&a.payload)?)
            .map_err(|_| "TRAE 账号快照损坏")?;
        if keys.keys().any(|k| !protocol::owned_key(k)) || protocol::identity(&keys)?.uid != a.uid {
            return Err("TRAE 快照身份校验失败".into());
        }
        Ok(keys)
    }
    pub fn status(&self) -> Result<Value, String> {
        let _guard = self.lock()?;
        let store = self.load()?;
        let mut clients = Vec::new();
        for variant in [Region::Cn, Region::Ai] {
            {
                let client = Client::Desktop;
                let path = self.paths.storage(variant, client);
                let detected = path.exists();
                let current = if detected {
                    Some(
                        self.storage(variant, client)
                            .and_then(|r| protocol::auth_keys(&r))
                            .and_then(|k| protocol::identity(&k)),
                    )
                } else {
                    None
                };
                let uid = current
                    .as_ref()
                    .and_then(|v| v.as_ref().ok())
                    .map(|a| a.uid.as_str());
                let active = store.accounts.iter().find(|a| {
                    a.variant == variant && a.client == client && Some(a.uid.as_str()) == uid
                });
                let running = if detected || self.paths.executable(variant, client).is_some() {
                    self.processes.running(variant, client).ok()
                } else {
                    Some(false)
                };
                clients.push(json!({"variant":variant,"client":client,"name":client.app_name(variant),"detected":detected,"running":running,
                "configured":uid.is_some(),"activeAccountId":active.map(|a|&a.id),"recognitionError":current.and_then(|v|v.err())}));
            }
        }
        Ok(
            json!({"accounts":store.accounts.iter().map(Account::meta).collect::<Vec<_>>(),"clients":clients,"nativeCliSupported":false}),
        )
    }
    fn capture(&self, args: &Args) -> Result<Value, String> {
        let _guard = self.lock()?;
        let mut store = self.load()?;
        let keys = protocol::auth_keys(&self.storage(args.variant, args.client)?)?;
        let id = self.upsert(&mut store, args.variant, args.client, &keys)?;
        self.save(&store)?;
        Ok(store.accounts.iter().find(|a| a.id == id).unwrap().meta())
    }
    fn switch(&self, args: &Args) -> Result<Value, String> {
        let _guard = self.lock()?;
        let mut store = self.load()?;
        let account = find(&store, args)?.clone();
        if account.variant != args.variant || account.client != args.client {
            return Err(
                "目标区域或客户端与保存的账号快照不一致；请先在对应客户端登录并保存".into(),
            );
        }
        let keys = self.keys(&account)?;
        let auth = protocol::identity(&keys)?;
        if auth
            .expires
            .is_some_and(|t| t <= chrono::Utc::now().timestamp())
        {
            return Err(
                "TRAE 登录已过期，请在官方客户端重新登录并保存；本工具不伪造刷新凭据".into(),
            );
        }
        let running = self.processes.running(args.variant, args.client)?;
        if args.dry_run {
            return Ok(
                json!({"dryRun":true,"account":account.meta(),"running":running,"requiresClose":running,"preservesProjects":true}),
            );
        }
        if running && !args.close_running {
            return Err("TRAE 正在运行，请先保存任务并退出，或显式允许关闭".into());
        }
        // Validate both the destination file and the vault BEFORE closing an app.
        let before = self.storage(args.variant, args.client)?;
        let previous = protocol::auth_keys(&before)?;
        if !previous.is_empty() {
            protocol::identity(&previous)?;
        }
        protocol::patch(&before, &keys)?;
        if running {
            self.processes.close(args.variant, args.client)?;
        }
        let result = (|| -> Result<Value, String> {
            if self.processes.running(args.variant, args.client)? {
                return Err("TRAE 重新启动，已停止切换".into());
            }
            let path = self.paths.storage(args.variant, args.client);
            let original = read_limited(&path)?;
            let root: Value =
                serde_json::from_slice(&original).map_err(|_| "TRAE 登录文件已变化，停止切换")?;
            let current = protocol::auth_keys(&root)?;
            if let Some(expected) = &args.expected_current_uid {
                if protocol::identity(&current).map(|a| a.uid).ok().as_ref() != Some(expected) {
                    return Err("当前账号已变化，自动轮换已停止".into());
                }
            }
            if !current.is_empty() {
                self.upsert(&mut store, args.variant, args.client, &current)?;
                self.save(&store)?;
            }
            let updated = protocol::patch(&root, &keys)?;
            let backup = self.vault.seal(&original)?;
            write_private(&self.paths.store.join("last-switch-backup.enc"), &backup)
                .map_err(|_| "无法写入加密回滚备份")?;
            if self.processes.running(args.variant, args.client)?
                || read_limited(&path)? != original
            {
                return Err("TRAE 进程或登录文件已变化，未执行写入".into());
            }
            replace_and_verify(
                &path,
                &original,
                &serde_json::to_string_pretty(&updated).map_err(|_| "无法编码 TRAE 文件")?,
                || {
                    let verified = self.storage(args.variant, args.client)?;
                    if verified != updated {
                        return Err("TRAE 登录写入校验失败".into());
                    }
                    Ok(())
                },
            )?;
            Ok(
                json!({"switched":true,"accountId":account.id,"variant":args.variant,"client":args.client,"preservesProjects":true}),
            )
        })();
        let rollback_failed = result
            .as_ref()
            .err()
            .is_some_and(|e| e.contains("回滚失败"));
        if args.relaunch
            && !rollback_failed
            && self
                .processes
                .launch(&self.paths, args.variant, args.client)
                .is_err()
        {
            return Err(match result {
                Ok(_) => "TRAE 账号已切换，但启动失败，请手动启动".into(),
                Err(error) => format!("{error}；客户端未能重新启动，请手动启动"),
            });
        }
        result
    }
    async fn credit(&self, args: &Args) -> Result<Value, String> {
        // The official usage query cannot rotate credentials. Keep slow network
        // reads outside the mutation lock so adding/renaming/switching stays usable.
        let (account, auth) = {
            let _guard = self.lock()?;
            let store = self.load()?;
            let account = find(&store, args)?.clone();
            let auth = protocol::identity(&self.keys(&account)?)?;
            (account, auth)
        };
        let credits =
            credits::query(account.variant, &auth, args.timeout_seconds.unwrap_or(30)).await?;
        self.cache_credit(&account, &credits)?;
        Ok(credits)
    }
    fn cache_credit(&self, account: &Account, credits: &Value) -> Result<(), String> {
        let _guard = self.lock()?;
        let mut store = self.load()?;
        let saved = store
            .accounts
            .iter_mut()
            .find(|a| a.id == account.id)
            .ok_or("账号已删除，未重新创建旧账号")?;
        if saved.payload != account.payload {
            return Err("账号登录信息已更新，请重试额度查询".into());
        }
        saved.credits = Some(credits.clone());
        insights::record(&mut store, account, credits);
        self.save(&store)?;
        Ok(())
    }
    pub async fn action(&self, action: &str, input: Value) -> Result<Value, String> {
        let args: Args =
            serde_json::from_value(input).map_err(|_| "TRAE 参数不合法或包含未知字段")?;
        match action {
            "statistics" => self.statistics(args.variant, args.refresh).await,
            "get_settings" => automation::status(self),
            "save_settings" => automation::save(self, args.settings.ok_or("缺少设置")?),
            "automation_run" => automation::run(self, true, Some(args.variant)).await,
            "backup_export" => self.export_backup(
                args.passphrase.as_deref().ok_or("请输入加密密码")?,
                args.path.as_deref(),
            ),
            "backup_preview" => self.preview_backup(
                args.data.as_deref().ok_or("请选择备份文件")?,
                args.passphrase.as_deref().ok_or("请输入加密密码")?,
            ),
            "backup_import" => self.import_backup(
                args.data.as_deref().ok_or("请选择备份文件")?,
                args.passphrase.as_deref().ok_or("请输入加密密码")?,
                args.snapshot_id.as_deref().ok_or("请先预览备份")?,
            ),
            "cloud_backup" | "cloud_preview" | "cloud_restore" | "cloud_sync" => {
                crate::modules::webdav_sync::trae_cloud(
                    self,
                    action,
                    args.passphrase.as_deref().ok_or("请输入加密密码")?,
                    args.snapshot_id.as_deref(),
                )
                .await
            }
            "capture" => self.capture(&args),
            "switch" => self.switch(&args),
            "credits" => self.credit(&args).await,
            "open" => {
                self.processes
                    .launch(&self.paths, args.variant, args.client)?;
                Ok(json!({"opened":true}))
            }
            "rename" | "delete" => {
                let _guard = self.lock()?;
                let mut store = self.load()?;
                let id = find(&store, &args)?.id.clone();
                if action == "delete" {
                    store.accounts.retain(|a| a.id != id);
                } else {
                    let name = args
                        .name
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty() && s.chars().count() <= 80)
                        .ok_or("名称须为 1–80 个字符")?;
                    store.accounts.iter_mut().find(|a| a.id == id).unwrap().name = name.into();
                }
                self.save(&store)?;
                Ok(json!({"ok":true,"accountId":id}))
            }
            _ => Err("不支持的 TRAE 操作；原生 TRAE CLI 不在本轮接入范围".into()),
        }
    }
}
fn write_private(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let temp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}
fn replace_and_verify(
    path: &std::path::Path,
    original: &[u8],
    updated: &str,
    verify: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let original = std::str::from_utf8(original).map_err(|_| "回滚备份编码异常")?;
    let result = write_private(path, updated)
        .map_err(|_| "无法写入 TRAE 登录文件".to_string())
        .and_then(|_| verify());
    if result.is_err() && write_private(path, original).is_err() {
        return Err(
            "TRAE 切换失败且回滚失败；加密备份保留在 ~/.wb-switch/trae/last-switch-backup.enc"
                .into(),
        );
    }
    result
}
fn read_limited(path: &std::path::Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path)
        .map_err(|_| "无法读取 TRAE 文件，请先在对应客户端登录并启动一次")?;
    let mut data = Vec::new();
    file.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|_| "无法读取 TRAE 文件")?;
    if data.len() > 16 * 1024 * 1024 {
        return Err("TRAE 文件超过安全读取上限".into());
    }
    Ok(data)
}
fn find<'a>(store: &'a Store, args: &Args) -> Result<&'a Account, String> {
    store
        .accounts
        .iter()
        .find(|a| Some(a.id.as_str()) == args.account_id.as_deref())
        .ok_or_else(|| "未找到 TRAE 账号，请先保存当前登录账号".into())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    #[serde(default)]
    refresh: bool,
    settings: Option<Value>,
    passphrase: Option<String>,
    data: Option<String>,
    path: Option<String>,
    snapshot_id: Option<String>,
    #[serde(skip)]
    expected_current_uid: Option<String>,
    #[serde(default = "default_region")]
    variant: Region,
    #[serde(default = "default_client")]
    client: Client,
    account_id: Option<String>,
    name: Option<String>,
    timeout_seconds: Option<u64>,
    #[serde(default)]
    close_running: bool,
    #[serde(default)]
    relaunch: bool,
    #[serde(default)]
    dry_run: bool,
}
fn default_region() -> Region {
    Region::Cn
}
fn default_client() -> Client {
    Client::Desktop
}
pub fn status() -> Result<Value, String> {
    Engine::real().status()
}
pub async fn action(action: &str, input: Value) -> Result<Value, String> {
    let action = action.to_owned();
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || runtime.block_on(Engine::real().action(&action, input)))
        .await
        .map_err(|_| "TRAE 任务异常结束".to_string())?
}
#[cfg(test)]
mod tests;
