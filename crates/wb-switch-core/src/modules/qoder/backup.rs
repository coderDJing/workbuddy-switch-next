//! Portable Qoder backups contain credentials; never log or include them in previews.
use super::{
    authorized::{self, AuthorizedAccount},
    paths::{credentials, QoderRoots, QoderTarget},
    store,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};

#[derive(Serialize, Deserialize)]
struct Backup {
    format: String,
    version: u32,
    accounts: Vec<Entry>,
}
#[derive(Serialize, Deserialize)]
struct Entry {
    label: String,
    credentials: Vec<Credential>,
}
#[derive(Serialize, Deserialize)]
struct Credential {
    target: QoderTarget,
    account: AuthorizedAccount,
}

fn parse(text: &str) -> Result<Backup, String> {
    if text.len() > 8 * 1024 * 1024 {
        return Err("Qoder 备份文件过大".into());
    }
    let backup: Backup =
        serde_json::from_str(text).map_err(|_| "Qoder 备份文件格式无效".to_string())?;
    if backup.format != "wb-switch-qoder"
        || backup.version != 1
        || backup.accounts.is_empty()
        || backup.accounts.len() > 1000
    {
        return Err("不是受支持的 Qoder 账号备份".into());
    }
    for entry in &backup.accounts {
        if entry.label.trim().is_empty()
            || entry.label.chars().count() > 128
            || entry.credentials.is_empty()
            || entry.credentials.len() > 2
        {
            return Err("Qoder 账号信息无效".into());
        }
        let mut targets = HashSet::new();
        let mut expected = None;
        for credential in &entry.credentials {
            if !targets.insert(credential.target)
                || credential.account.access_token.trim().is_empty()
            {
                return Err("Qoder 账号凭据无效".into());
            }
            let id = account_identity(credential.target, &credential.account)?;
            if expected.as_ref().is_some_and(|key| key != &id) {
                return Err("Qoder 账号身份或区域不一致".into());
            }
            expected = Some(id);
        }
    }
    Ok(backup)
}

fn account_identity(target: QoderTarget, account: &AuthorizedAccount) -> Result<String, String> {
    authorized::identity_from_json(target, &json!({"user": account.user}))
        .ok_or_else(|| "Qoder 账号缺少有效身份".into())
}

pub(super) fn profile_account(profile: &Value) -> Option<AuthorizedAccount> {
    let get = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            profile
                .get(*key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
    };
    let date = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            let value = profile.get(*key)?;
            if let Some(s) = value.as_str() {
                return chrono::DateTime::parse_from_rfc3339(s)
                    .ok()
                    .map(|t| t.to_rfc3339());
            }
            let n = value.as_i64()?;
            chrono::DateTime::from_timestamp(if n > 100_000_000_000 { n / 1000 } else { n }, 0)
                .map(|t| t.to_rfc3339())
        })
    };
    let mut user = profile
        .get("user")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let obj = user.as_object_mut()?;
    if !obj.contains_key("id") {
        obj.insert(
            "id".into(),
            profile
                .get("uid")
                .or_else(|| profile.get("user_id"))?
                .clone(),
        );
    }
    for (from, to) in [
        ("name", "name"),
        ("avatar_url", "avatarUrl"),
        ("data_policy_agreed", "data_policy_agreed"),
        ("is_data_policy_modifiable", "is_data_policy_modifiable"),
        ("user_type", "user_type"),
        ("is_personal_version", "is_personal_version"),
    ] {
        if let Some(value) = profile.get(from) {
            obj.entry(to).or_insert_with(|| value.clone());
        }
    }
    Some(AuthorizedAccount {
        access_token: get(&[
            "token",
            "security_oauth_token",
            "access_token",
            "accessToken",
        ])?,
        refresh_token: get(&["refreshToken", "refresh_token"]),
        expires_at: date(&["expiresAt", "expire_time", "expires_at"]),
        refresh_expires_at: date(&["refreshTokenExpiresAt", "refresh_token_expire_time"]),
        user,
    })
}

fn bundle_account(
    root: &Path,
    roots: &QoderRoots,
    meta: &store::BundleMeta,
) -> Result<AuthorizedAccount, String> {
    let directory = store::bundle_dir(root, &meta.id)?;
    #[cfg(windows)]
    if meta.files.len() == 1 && meta.files[0].name == super::vault::FILE {
        let bytes = std::fs::read(directory.join(super::vault::FILE))
            .map_err(|_| "无法读取 Qoder 账号凭据")?;
        if store::sha256_hex(&bytes) != meta.files[0].sha256 {
            return Err("Qoder 账号凭据校验失败".into());
        }
        return Ok(super::vault::open(&bytes)?.0);
    }
    let specs = credentials(roots, meta.target);
    let mut names = HashSet::new();
    let mut payload = Vec::new();
    for f in &meta.files {
        if !specs.iter().any(|s| s.name == f.name) || !names.insert(&f.name) {
            return Err("Qoder 账号凭据文件无效".into());
        }
        let bytes =
            std::fs::read(directory.join(&f.name)).map_err(|_| "无法读取 Qoder 账号凭据")?;
        if store::sha256_hex(&bytes) != f.sha256 {
            return Err("Qoder 账号凭据校验失败".into());
        }
        payload.push((f.name.clone(), bytes));
    }
    let profile = authorized::snapshot_profile(&directory, meta.target, &payload)
        .ok_or("无法解密 Qoder 账号，请重新登录后导出")?;
    profile_account(&profile)
        .ok_or_else(|| "Qoder 账号缺少可迁移的登录信息，请重新登录后导出".into())
}

fn live_account(
    roots: &QoderRoots,
    target: QoderTarget,
    identity: &str,
) -> Option<AuthorizedAccount> {
    let mut payload = Vec::new();
    for spec in credentials(roots, target) {
        if let Ok(bytes) = std::fs::read(spec.path) {
            payload.push((spec.name.to_string(), bytes));
        }
    }
    let profile = authorized::snapshot_profile(&roots.desktop_dir_for(target), target, &payload)?;
    let account = profile_account(&profile)?;
    (account_identity(target, &account).ok().as_deref() == Some(identity)).then_some(account)
}

fn export_with(root: &Path, roots: &QoderRoots, ids: &[String]) -> Result<Backup, String> {
    if ids.is_empty() || ids.len() > 2000 {
        return Err("请选择要导出的 Qoder 账号".into());
    }
    let _lock = crate::modules::state_lock::StateLock::wait_at(
        &root.join("switch.lock"),
        Duration::from_secs(2),
    )
    .map_err(|_| "Qoder 账号正忙，请稍后重试")?;
    let mut entries: Vec<Entry> = Vec::new();
    let mut groups = HashMap::new();
    let mut selected = HashSet::new();
    for id in ids {
        if !selected.insert(id) {
            continue;
        }
        let meta = store::load(root, id)?;
        let saved = bundle_account(root, roots, &meta)?;
        let identity = account_identity(meta.target, &saved)?;
        if meta
            .identity_key
            .as_ref()
            .is_some_and(|key| key != &identity)
        {
            return Err("Qoder 账号身份不一致，请重新添加".into());
        }
        let account = live_account(roots, meta.target, &identity).unwrap_or(saved);
        let index = *groups.entry(identity).or_insert_with(|| {
            entries.push(Entry {
                label: meta.label.clone(),
                credentials: Vec::new(),
            });
            entries.len() - 1
        });
        if entries[index]
            .credentials
            .iter()
            .any(|c| c.target == meta.target)
        {
            return Err("选择中包含重复的 Qoder 客户端账号".into());
        }
        entries[index].credentials.push(Credential {
            target: meta.target,
            account,
        });
    }
    Ok(Backup {
        format: "wb-switch-qoder".into(),
        version: 1,
        accounts: entries,
    })
}

pub fn export(ids: &[String]) -> Result<Value, String> {
    let backup = export_with(&store::default_store(), &QoderRoots::real(), ids)?;
    serde_json::to_value(backup).map_err(|_| "无法生成 Qoder 备份".into())
}
pub fn export_to_path(ids: &[String], path: &str) -> Result<Value, String> {
    let p = Path::new(path);
    if !p.is_absolute()
        || !p
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        return Err("请选择 JSON 文件保存路径".into());
    }
    let backup = export(ids)?;
    let text = serde_json::to_string_pretty(&backup).map_err(|_| "无法生成 Qoder 备份")?;
    std::fs::write(p, text).map_err(|_| "无法保存 Qoder 备份文件")?;
    Ok(json!({"path":path}))
}
pub fn preview(text: &str) -> Result<Value, String> {
    let backup = parse(text)?;
    Ok(
        json!({"accounts":backup.accounts.iter().enumerate().map(|(index, entry)| json!({"index":index,"label":entry.label,"global":entry.credentials[0].target.is_global(),"targets":entry.credentials.iter().map(|c| c.target).collect::<Vec<_>>() })).collect::<Vec<_>>()}),
    )
}
pub fn import(text: &str, indexes: &[usize]) -> Result<Value, String> {
    import_with(&store::default_store(), &QoderRoots::real(), text, indexes)
}
fn import_with(
    root: &Path,
    roots: &QoderRoots,
    text: &str,
    indexes: &[usize],
) -> Result<Value, String> {
    let backup = parse(text)?;
    if indexes.is_empty() || indexes.iter().any(|i| *i >= backup.accounts.len()) {
        return Err("请选择有效的 Qoder 账号".into());
    }
    let _lock = crate::modules::state_lock::StateLock::wait_at(
        &root.join("switch.lock"),
        Duration::from_secs(2),
    )
    .map_err(|_| "Qoder 账号正忙，请稍后重试")?;
    let mut imported = 0;
    let mut skipped = 0;
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    for index in indexes {
        if !seen.insert(index) {
            continue;
        }
        let entry = backup.accounts.get(*index).ok_or("Qoder 备份选择无效")?;
        let device = uuid::Uuid::new_v4().to_string();
        let mut saved = 0;
        let mut destinations: Vec<_> = entry
            .credentials
            .iter()
            .map(|c| (c.target, &c.account))
            .collect();
        if entry.credentials.len() == 1 {
            let first = &entry.credentials[0];
            let other = match first.target {
                QoderTarget::Desktop => QoderTarget::Cli,
                QoderTarget::Cli => QoderTarget::Desktop,
                QoderTarget::GlobalDesktop => QoderTarget::GlobalCli,
                QoderTarget::GlobalCli => QoderTarget::GlobalDesktop,
            };
            if !other.is_desktop()
                || (first
                    .account
                    .refresh_token
                    .as_deref()
                    .is_some_and(|v| !v.is_empty())
                    && first.account.expires_at.is_some())
            {
                destinations.push((other, &first.account));
            } else {
                warnings.push(format!(
                    "{}：缺少桌面端所需的授权信息，请重新登录添加",
                    entry.label
                ));
            }
        }
        let mut already_present = 0;
        for (target, account) in destinations {
            let identity = account_identity(target, account)?;
            let same = store::list(root, Some(target))
                .into_iter()
                .rev()
                .find(|m| m.identity_key.as_deref() == Some(identity.as_str()))
                .and_then(|meta| bundle_account(root, roots, &meta).ok())
                .is_some_and(|existing| {
                    serde_json::to_value(existing).ok() == serde_json::to_value(account).ok()
                });
            if same {
                if super::auto::allow(root, Some(&identity), Some(target)).is_err() {
                    warnings.push(format!("{}：无法更新自动识别设置", entry.label));
                }
                already_present += 1;
                continue;
            }
            #[cfg(windows)]
            let prepared =
                super::vault::seal(account, &device).map(|v| vec![(super::vault::FILE.into(), v)]);
            #[cfg(not(windows))]
            let prepared = authorized::build_snapshot(roots, target, account, &device);
            match prepared.and_then(|payload| {
                store::save_authorized(root, target, &entry.label, &identity, payload)
            }) {
                Ok(_) => {
                    if super::auto::allow(root, Some(&identity), Some(target)).is_err() {
                        warnings.push(format!(
                            "{}：账号已保存，但无法更新自动识别设置",
                            entry.label
                        ));
                    }
                    saved += 1;
                }
                Err(_) => warnings.push(format!(
                    "{}：无法保存{}，请检查客户端加密存储或重新登录",
                    entry.label,
                    target.label()
                )),
            }
        }
        if saved > 0 {
            imported += 1;
        } else if already_present > 0 {
            skipped += 1;
        } else {
            warnings.push(format!("{}：未能导入", entry.label));
        }
    }
    Ok(json!({"imported":imported,"skipped":skipped,"warnings":warnings}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn account(uid: &str, token: &str) -> AuthorizedAccount {
        AuthorizedAccount {
            access_token: token.into(),
            refresh_token: Some("test-refresh".into()),
            expires_at: Some("2099-01-01T00:00:00Z".into()),
            refresh_expires_at: None,
            user: json!({"id":uid,"name":"Synthetic"}),
        }
    }
    #[cfg(windows)]
    fn roots(dir: &Path) -> QoderRoots {
        QoderRoots {
            home: dir.join("home"),
            app_data: dir.join("appdata"),
        }
    }
    #[test]
    fn preview_validates_identity_and_does_not_expose_tokens() {
        let backup = Backup {
            format: "wb-switch-qoder".into(),
            version: 1,
            accounts: vec![Entry {
                label: "Test".into(),
                credentials: vec![Credential {
                    target: QoderTarget::Cli,
                    account: account("uid", "test-secret"),
                }],
            }],
        };
        let text = serde_json::to_string(&backup).unwrap();
        let shown = preview(&text).unwrap().to_string();
        assert!(!shown.contains("test-secret"));
        assert!(!shown.contains("uid"));
        assert!(shown.contains("Test"));
        let mut bad: Value = serde_json::from_str(&text).unwrap();
        bad["accounts"][0]["credentials"][0]["account"]["access_token"] = json!("");
        assert!(preview(&bad.to_string()).is_err());
        assert!(preview("[]").is_err());
    }
    #[test]
    fn profile_converts_cli_and_desktop_credentials() {
        for value in [
            json!({"uid":"id","access_token":"token","refresh_token":"refresh","expire_time":4070908800_i64}),
            json!({"user":{"id":"id"},"token":"token","refreshToken":"refresh","expiresAt":"2099-01-01T00:00:00Z"}),
        ] {
            let result = profile_account(&value).unwrap();
            assert_eq!(result.user["id"], "id");
            assert_eq!(result.access_token, "token");
            assert_eq!(result.refresh_token.as_deref(), Some("refresh"));
            assert!(result.expires_at.is_some());
        }
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn backup_round_trip_restores_both_regions_without_touching_live_clients() {
        let temp =
            std::env::temp_dir().join(format!("wb-qoder-backup-test-{}", uuid::Uuid::new_v4()));
        let source = temp.join("source");
        let destination = temp.join("destination");
        let source_roots = roots(&source);
        let destination_roots = roots(&destination);
        let source_store = source.join("store");
        let destination_store = destination.join("store");
        let mut ids = Vec::new();
        for (desktop, cli, uid) in [
            (QoderTarget::Desktop, QoderTarget::Cli, "cn-user"),
            (
                QoderTarget::GlobalDesktop,
                QoderTarget::GlobalCli,
                "global-user",
            ),
        ] {
            let credential = account(uid, "secret-token");
            let identity = account_identity(desktop, &credential).unwrap();
            let encrypted =
                super::super::vault::seal(&credential, "original-machine-identifier").unwrap();
            for target in [desktop, cli] {
                let meta = store::save_authorized(
                    &source_store,
                    target,
                    "Test",
                    &identity,
                    vec![(super::super::vault::FILE.into(), encrypted.clone())],
                )
                .unwrap();
                ids.push(meta.id);
            }
        }
        let backup = export_with(&source_store, &source_roots, &ids).unwrap();
        assert_eq!(backup.accounts.len(), 2);
        let text = serde_json::to_string(&backup).unwrap();
        let result = import_with(&destination_store, &destination_roots, &text, &[0, 1]).unwrap();
        assert_eq!(result["imported"], 2);
        assert_eq!(result["warnings"].as_array().unwrap().len(), 0);
        assert_eq!(store::list(&destination_store, None).len(), 4);
        let repeated = import_with(&destination_store, &destination_roots, &text, &[0, 1]).unwrap();
        assert_eq!(repeated["imported"], 0);
        assert_eq!(repeated["skipped"], 2);
        assert_eq!(store::list(&destination_store, None).len(), 4);
        assert!(!destination_roots.home.exists());
        assert!(!destination_roots.app_data.exists());
        // A portable account must also be usable for credit queries before any
        // Qoder client is installed or switched on the destination machine.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(error) => panic!("mock credit server did not receive request: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut buffer = [0; 8192];
            let n = stream.read(&mut buffer).unwrap();
            assert!(String::from_utf8_lossy(&buffer[..n])
                .to_ascii_lowercase()
                .contains("bearer secret-token"));
            let body = r#"{"userQuota":{"remaining":12,"total":20,"used":8}}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        });
        let imported_id = store::list(&destination_store, None)[0].id.clone();
        let credit = super::super::credits::query_with(
            &destination_store,
            &destination_roots,
            &[imported_id],
            None,
            Some(&endpoint),
        )
        .await;
        server.join().unwrap();
        assert_eq!(credit["ok"], true);
        assert_eq!(credit["remaining"], 12.0);
        for meta in store::list(&destination_store, None) {
            let encrypted = std::fs::read(
                store::bundle_dir(&destination_store, &meta.id)
                    .unwrap()
                    .join(super::super::vault::FILE),
            )
            .unwrap();
            let (imported, machine) = super::super::vault::open(&encrypted).unwrap();
            let rebuilt =
                authorized::build_snapshot(&destination_roots, meta.target, &imported, &machine)
                    .unwrap();
            let desktop_root = destination_roots.desktop_dir_for(meta.target);
            if meta.target.is_desktop() {
                std::fs::create_dir_all(&desktop_root).unwrap();
                let local_state = rebuilt
                    .iter()
                    .find(|(name, _)| name == "Local State")
                    .unwrap();
                std::fs::write(desktop_root.join("Local State"), &local_state.1).unwrap();
            }
            let profile =
                authorized::snapshot_profile(&desktop_root, meta.target, &rebuilt).unwrap();
            assert_eq!(
                profile_account(&profile).unwrap().access_token,
                "secret-token"
            );
        }
        let reexport = export_with(
            &destination_store,
            &destination_roots,
            &store::list(&destination_store, None)
                .iter()
                .map(|m| m.id.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(reexport.accounts.len(), 2);
        assert_eq!(
            reexport.accounts[0].credentials[0].account.access_token,
            "secret-token"
        );
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "requires explicitly authorized local Qoder accounts"]
    async fn real_accounts_export_and_import_into_isolated_store() {
        let source = store::default_store();
        let source_roots = QoderRoots::real();
        let mut latest = HashMap::new();
        for meta in store::list(&source, None) {
            let key = (
                meta.identity_key.clone().unwrap_or_else(|| meta.id.clone()),
                meta.target,
            );
            latest.insert(key, meta.id);
        }
        assert!(!latest.is_empty(), "no saved Qoder accounts");
        let backup = export_with(
            &source,
            &source_roots,
            &latest.into_values().collect::<Vec<_>>(),
        )
        .expect("real Qoder account export failed");
        let text = serde_json::to_string(&backup).unwrap();
        let target =
            std::env::temp_dir().join(format!("wb-qoder-migration-test-{}", uuid::Uuid::new_v4()));
        let target_roots = roots(&target);
        let result = import_with(
            &target.join("store"),
            &target_roots,
            &text,
            &(0..backup.accounts.len()).collect::<Vec<_>>(),
        )
        .expect("isolated Qoder import failed");
        assert_eq!(
            result["imported"].as_u64(),
            Some(backup.accounts.len() as u64)
        );
        assert_eq!(result["warnings"].as_array().unwrap().len(), 0);
        assert!(!target_roots.home.exists());
        assert!(!target_roots.app_data.exists());
        for meta in store::list(&target.join("store"), None) {
            let encrypted = std::fs::read(
                store::bundle_dir(&target.join("store"), &meta.id)
                    .unwrap()
                    .join(super::super::vault::FILE),
            )
            .unwrap();
            let (imported, machine) = super::super::vault::open(&encrypted).unwrap();
            let rebuilt =
                authorized::build_snapshot(&target_roots, meta.target, &imported, &machine)
                    .expect("could not rebuild imported Qoder credentials");
            let desktop_root = target_roots.desktop_dir_for(meta.target);
            if meta.target.is_desktop() {
                std::fs::create_dir_all(&desktop_root).unwrap();
                let state = rebuilt
                    .iter()
                    .find(|(name, _)| name == "Local State")
                    .unwrap();
                std::fs::write(desktop_root.join("Local State"), &state.1).unwrap();
            }
            let profile = authorized::snapshot_profile(&desktop_root, meta.target, &rebuilt)
                .expect("could not decode rebuilt Qoder credentials");
            assert!(profile_account(&profile)
                .is_some_and(|restored| restored.access_token == imported.access_token));
        }
        let mut queried = 0;
        for entry in &backup.accounts {
            let identity =
                account_identity(entry.credentials[0].target, &entry.credentials[0].account)
                    .unwrap();
            let original: Vec<_> = store::list(&source, None)
                .into_iter()
                .filter(|meta| meta.identity_key.as_deref() == Some(identity.as_str()))
                .map(|meta| meta.id)
                .collect();
            let restored: Vec<_> = store::list(&target.join("store"), None)
                .into_iter()
                .filter(|meta| meta.identity_key.as_deref() == Some(identity.as_str()))
                .map(|meta| meta.id)
                .collect();
            let before =
                super::super::credits::query_with(&source, &source_roots, &original, None, None)
                    .await;
            let after = super::super::credits::query_with(
                &target.join("store"),
                &target_roots,
                &restored,
                None,
                None,
            )
            .await;
            if before["ok"] == true {
                assert_eq!(
                    after["ok"], true,
                    "isolated imported account could not query credits"
                );
                queried += 1;
            }
        }
        println!("Portable real-account credit probe: successful accounts={queried}");
        assert!(queried > 0, "no original account credits could be queried");
        std::fs::remove_dir_all(target).unwrap();
    }
}
