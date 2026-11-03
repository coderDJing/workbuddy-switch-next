//! Preparation of Qoder client credentials obtained through device authorization.
//!
//! Build an encrypted snapshot in the account store; do not change the running
//! client's files here. Switching continues through the existing transaction.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{Map, Value};

use super::paths::{credentials, Criticality, QoderRoots, QoderTarget};
use crate::modules::vscode_cn_inject::{
    decrypt_secret_payload, encrypt_secret_payload, VscodeSafeStorageTarget,
};

/// An authorization result must never be serialized to the UI or diagnostics.
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct AuthorizedAccount {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
    #[serde(default)]
    pub refresh_expires_at: Option<String>,
    pub user: Value,
}

fn unused_data_dir() -> Option<PathBuf> {
    None
}

pub(super) fn safe_storage_target(target: QoderTarget) -> VscodeSafeStorageTarget {
    let global = target.is_global();
    VscodeSafeStorageTarget {
        data_dir_resolver: unused_data_dir,
        display_name: if global { "Qoder" } else { "Qoder CN" },
        secret_item_prefix_extension_id: "",
        secret_key: "",
        macos_keychain_service: if global {
            "Qoder Safe Storage"
        } else {
            "Qoder CN App Safe Storage"
        },
        linux_secret_tool_app_names: if global {
            &["Qoder", "qoder"]
        } else {
            &["Qoder CN App", "Qoder CN", "qoder-cn"]
        },
    }
}

pub(super) fn identity_key(target: QoderTarget, uid: &str) -> String {
    super::store::sha256_hex(
        format!(
            "{}\0{}",
            if target.is_global() { "global" } else { "cn" },
            uid.trim()
        )
        .as_bytes(),
    )
}

static LIVE_IDENTITIES: OnceLock<Mutex<std::collections::HashMap<(QoderTarget, String), String>>> =
    OnceLock::new();

// User-authorized desktop profiles stay in memory only. Background credit
// refreshes can reuse them without repeatedly invoking the system keychain.
#[cfg(not(windows))]
static DESKTOP_PROFILES: OnceLock<Mutex<std::collections::HashMap<(QoderTarget, String), Value>>> =
    OnceLock::new();

#[cfg(not(windows))]
fn remember_desktop_profile(target: QoderTarget, primary: &[u8], profile: &Value) {
    if !target.is_desktop() {
        return;
    }
    let cache = DESKTOP_PROFILES.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        if cache.len() >= 128 {
            cache.clear();
        }
        cache.insert((target, super::store::sha256_hex(primary)), profile.clone());
    }
}

#[cfg(not(windows))]
pub(super) fn cached_desktop_profile(target: QoderTarget, primary: &[u8]) -> Option<Value> {
    DESKTOP_PROFILES
        .get()?
        .lock()
        .ok()?
        .get(&(target, super::store::sha256_hex(primary)))
        .cloned()
}

#[cfg(not(windows))]
pub(super) fn forget_desktop_profile(target: QoderTarget, primary_sha: Option<&str>) {
    let Some(sha) = primary_sha else {
        return;
    };
    if let Some(cache) = DESKTOP_PROFILES.get() {
        if let Ok(mut cache) = cache.lock() {
            cache.remove(&(target, sha.to_string()));
        }
    }
}

pub(super) fn identity_from_json(target: QoderTarget, value: &Value) -> Option<String> {
    let identity = value
        .get("user")
        .and_then(|v| v.get("id").or_else(|| v.get("user_id")))
        .or_else(|| value.get("uid"))
        .or_else(|| value.get("user_id"))?;
    let uid = identity
        .as_str()
        .map(str::to_string)
        .or_else(|| identity.as_i64().map(|id| id.to_string()))?;
    if uid.trim().is_empty() {
        return None;
    }
    Some(identity_key(target, &uid))
}

fn remember_live_identity(target: QoderTarget, primary: &[u8], profile: &Value) {
    let Some(identity) = identity_from_json(target, profile) else {
        return;
    };
    let cache = LIVE_IDENTITIES.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        cache.retain(|(cached_target, _), _| *cached_target != target);
        cache.insert((target, super::store::sha256_hex(primary)), identity);
    }
}

pub(super) fn cached_live_identity(target: QoderTarget, primary_sha: &str) -> Option<String> {
    LIVE_IDENTITIES
        .get()?
        .lock()
        .ok()?
        .get(&(target, primary_sha.to_string()))
        .cloned()
}

/// Read only the stable account identity from a saved client snapshot. Failure
/// leaves it ungrouped; display names must never be used as account identifiers.
pub(super) fn snapshot_identity(
    roots: &QoderRoots,
    target: QoderTarget,
    payload: &[(String, Vec<u8>)],
) -> Option<String> {
    let primary = &payload.first()?.1;
    if let Some(identity) = cached_live_identity(target, &super::store::sha256_hex(primary)) {
        return Some(identity);
    }
    let value = snapshot_profile(&roots.desktop_dir_for(target), target, payload)?;
    remember_live_identity(target, primary, &value);
    identity_from_json(target, &value)
}

/// Decode only in memory. `desktop_root` may point to a saved snapshot so its
/// own Local State is used rather than the current client's key.
pub(super) fn snapshot_profile(
    desktop_root: &std::path::Path,
    target: QoderTarget,
    payload: &[(String, Vec<u8>)],
) -> Option<Value> {
    let primary = &payload.first()?.1;
    #[cfg(not(windows))]
    if target.is_desktop() {
        if let Some(profile) = cached_desktop_profile(target, primary) {
            return Some(profile);
        }
    }
    let plain = if target.is_desktop() {
        decrypt_secret_payload(primary, desktop_root, &safe_storage_target(target)).ok()?
    } else if primary.first() == Some(&b'{') {
        primary.clone()
    } else {
        let machine = payload
            .iter()
            .find(|(name, _)| name == "machine_id")?
            .1
            .as_slice();
        let machine = std::str::from_utf8(machine).ok()?.trim().as_bytes();
        let key = machine.get(..16)?;
        let mut encrypted = STANDARD.decode(primary).ok()?;
        cbc::Decryptor::<aes::Aes128>::new_from_slices(key, key)
            .ok()?
            .decrypt_padded_mut::<Pkcs7>(&mut encrypted)
            .ok()?
            .to_vec()
    };
    let profile = serde_json::from_slice(&plain).ok()?;
    #[cfg(not(windows))]
    remember_desktop_profile(target, primary, &profile);
    Some(profile)
}

/// Bootstrap a Windows desktop profile without modifying the live directory.
/// Field requirements come from Qoder CN 0.4.8's desktop credential reader.
#[cfg(windows)]
fn build_fresh_desktop(
    roots: &QoderRoots,
    target: QoderTarget,
    account: &AuthorizedAccount,
    device_id: &str,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    use crate::modules::vscode_cn_inject::{
        decrypt_windows_gcm_v10, encrypt_windows_gcm_v10, get_windows_encryption_key,
    };
    use aes_gcm::aead::{rand_core::RngCore, OsRng};
    if account.refresh_token.as_deref().is_none_or(str::is_empty) || account.expires_at.is_none() {
        return Err("Qoder 授权缺少刷新令牌或到期时间，账号已保存，请重新登录后重试".into());
    }
    let root = roots.desktop_dir_for(target);
    let state_path = root.join("Local State");
    let original_state = match std::fs::read(&state_path) {
        Ok(state) => Some(state),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("无法读取 Qoder 桌面端加密存储".into()),
    };
    let mut state_json: Value = match original_state.as_ref() {
        Some(bytes) => serde_json::from_slice(bytes).map_err(|_| "Qoder 桌面端加密存储格式无效")?,
        None => serde_json::json!({}),
    };
    let state_object = state_json
        .as_object_mut()
        .ok_or("Qoder 桌面端加密存储格式无效")?;
    let os_crypt = state_object
        .entry("os_crypt")
        .or_insert_with(|| serde_json::json!({}));
    let crypt_object = os_crypt
        .as_object_mut()
        .ok_or("Qoder 桌面端加密存储格式无效")?;
    let (state, key) = if crypt_object.contains_key("encrypted_key") {
        let key =
            get_windows_encryption_key(&root).map_err(|_| "无法读取 Qoder 桌面端的加密密钥")?;
        (original_state.ok_or("Qoder 桌面端加密存储格式无效")?, key)
    } else {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        let protected = super::vault::protect_bytes(&key)?;
        let wrapped = STANDARD.encode([b"DPAPI".as_slice(), &protected].concat());
        crypt_object.insert("encrypted_key".into(), Value::String(wrapped));
        let state = serde_json::to_vec(&state_json).map_err(|_| "无法创建 Qoder 加密存储")?;
        (state, key.to_vec())
    };
    if key.len() != 32 {
        return Err("Qoder 桌面端加密密钥无效".into());
    }
    let mut profile = serde_json::json!({"schemaVersion":1});
    update_profile(&mut profile, account, true)?;
    let plain = serde_json::to_vec(&profile).map_err(|_| "无法生成 Qoder 登录数据")?;
    let encrypted = encrypt_windows_gcm_v10(&key, &plain).map_err(|_| "无法加密 Qoder 登录数据")?;
    if decrypt_windows_gcm_v10(&key, &encrypted).map_err(|_| "无法验证 Qoder 登录数据")? != plain
    {
        return Err("Qoder 登录数据校验失败".into());
    }
    let mut payload = vec![
        ("auth.v1.dat".into(), encrypted),
        ("Local State".into(), state),
    ];
    for spec in credentials(roots, target).iter().skip(2) {
        if spec.criticality == Criticality::Paired {
            continue;
        }
        match std::fs::read(&spec.path) {
            Ok(value) if !value.is_empty() => payload.push((spec.name.into(), value)),
            Err(error)
                if spec.name == "auth.machine-id"
                    && error.kind() == std::io::ErrorKind::NotFound =>
            {
                if device_id.trim().is_empty() {
                    return Err("Qoder 设备标识无效".into());
                }
                payload.push((spec.name.into(), device_id.as_bytes().to_vec()));
            }
            _ => {}
        }
    }
    Ok(payload)
}

/// Produce a snapshot for a single client without modifying that client's files.
pub(super) fn build_snapshot(
    roots: &QoderRoots,
    target: QoderTarget,
    account: &AuthorizedAccount,
    device_id: &str,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let specs = credentials(roots, target);
    let primary = &specs[0];
    let original = match std::fs::read(&primary.path) {
        Ok(value) => value,
        #[cfg(windows)]
        Err(error) if target.is_desktop() && error.kind() == std::io::ErrorKind::NotFound => {
            return build_fresh_desktop(roots, target, account, device_id);
        }
        Err(error) if !target.is_desktop() && error.kind() == std::io::ErrorKind::NotFound => {
            Vec::new()
        }
        Err(_) if target.is_desktop() => {
            return Err(format!(
                "无法读取 {} 的登录文件，账号已保存",
                target.label()
            ))
        }
        Err(_) => return Err(format!("无法读取 {} 的登录文件", target.label())),
    };
    let mut cli_machine = None;
    let encoded = if target.is_desktop() {
        let root = roots.desktop_dir_for(target);
        let descriptor = safe_storage_target(target);
        let plaintext = decrypt_secret_payload(&original, &root, &descriptor)
            .map_err(|_| format!("无法读取 {} 的本机加密存储", target.label()))?;
        let mut profile: Value = serde_json::from_slice(&plaintext)
            .map_err(|_| "Qoder 桌面登录文件格式不受支持".to_string())?;
        remember_live_identity(target, &original, &profile);
        update_profile(&mut profile, account, true)?;
        let plaintext =
            serde_json::to_vec(&profile).map_err(|_| "无法生成 Qoder 登录数据".to_string())?;
        let prefix = std::str::from_utf8(original.get(..3).unwrap_or_default()).ok();
        let encrypted = encrypt_secret_payload(&plaintext, prefix, &root, &descriptor)
            .map_err(|_| "无法加密 Qoder 登录数据".to_string())?;
        let roundtrip = decrypt_secret_payload(&encrypted, &root, &descriptor)
            .map_err(|_| "无法验证 Qoder 加密数据".to_string())?;
        if roundtrip != plaintext {
            return Err("Qoder 加密数据校验失败".to_string());
        }
        #[cfg(not(windows))]
        remember_desktop_profile(target, &encrypted, &profile);
        encrypted
    } else {
        let machine_path = roots.cli_dir_for(target).join(".auth").join("machine_id");
        let machine = match std::fs::read_to_string(&machine_path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && original.is_empty() => {
                device_id.to_string()
            }
            Err(_) => return Err(format!("无法读取 {} 的设备标识", target.label())),
        };
        let key = machine
            .trim()
            .as_bytes()
            .get(..16)
            .ok_or_else(|| "Qoder CLI 设备标识无效".to_string())?;
        let plaintext = if original.is_empty() {
            b"{}".to_vec()
        } else if original.first() == Some(&b'{') {
            original.clone()
        } else {
            let mut bytes = STANDARD
                .decode(&original)
                .map_err(|_| "Qoder CLI 凭据格式不受支持".to_string())?;
            cbc::Decryptor::<aes::Aes128>::new_from_slices(key, key)
                .map_err(|_| "Qoder CLI 加密密钥无效".to_string())?
                .decrypt_padded_mut::<Pkcs7>(&mut bytes)
                .map_err(|_| "无法读取 Qoder CLI 加密存储".to_string())?
                .to_vec()
        };
        let mut profile: Value = serde_json::from_slice(&plaintext)
            .map_err(|_| "Qoder CLI 登录文件格式不受支持".to_string())?;
        if !original.is_empty() {
            remember_live_identity(target, &original, &profile);
        }
        update_profile(&mut profile, account, false)?;
        let plaintext =
            serde_json::to_vec(&profile).map_err(|_| "无法生成 Qoder CLI 登录数据".to_string())?;
        let mut buffer = plaintext.clone();
        buffer.resize(plaintext.len() + 16, 0);
        let ciphertext = cbc::Encryptor::<aes::Aes128>::new_from_slices(key, key)
            .map_err(|_| "Qoder CLI 加密密钥无效".to_string())?
            .encrypt_padded_mut::<Pkcs7>(&mut buffer, plaintext.len())
            .map_err(|_| "无法加密 Qoder CLI 登录数据".to_string())?
            .to_vec();
        let mut encrypted_copy = ciphertext.clone();
        let verified = cbc::Decryptor::<aes::Aes128>::new_from_slices(key, key)
            .map_err(|_| "Qoder CLI 加密密钥无效".to_string())?
            .decrypt_padded_mut::<Pkcs7>(&mut encrypted_copy)
            .map_err(|_| "无法验证 Qoder CLI 加密数据".to_string())?;
        if verified != plaintext {
            return Err("Qoder CLI 加密数据校验失败".to_string());
        }
        cli_machine = Some(machine.trim().as_bytes().to_vec());
        STANDARD.encode(ciphertext).into_bytes()
    };
    let mut payload = vec![(primary.name.to_string(), encoded)];
    for spec in specs.iter().skip(1) {
        if spec.name == "machine_id" {
            if let Some(machine) = cli_machine.take() {
                payload.push((spec.name.to_string(), machine));
            }
            continue;
        }
        // Paired user data belongs to the previous account; do not carry it over.
        if spec.criticality == Criticality::Paired {
            continue;
        }
        match std::fs::read(&spec.path) {
            Ok(bytes) if !bytes.is_empty() => payload.push((spec.name.to_string(), bytes)),
            _ if spec.criticality == Criticality::Required => {
                return Err(format!("{} 缺少本机加密存储", target.label()))
            }
            _ => {}
        }
    }
    Ok(payload)
}

/// Apply only account-specific fields; never retain another account's refresh
/// token or personal profile when the new authorization did not provide them.
pub(super) fn update_profile(
    profile: &mut Value,
    account: &AuthorizedAccount,
    desktop: bool,
) -> Result<(), String> {
    let object = profile
        .as_object_mut()
        .ok_or_else(|| "Qoder 登录文件不是 JSON 对象".to_string())?;
    if !desktop {
        // Current Qoder CLI regenerates encrypt_user_info and key at startup.
        // Never carry the previous account's identity, policy or auth fields.
        let user = account
            .user
            .as_object()
            .filter(|u| u.contains_key("id"))
            .ok_or_else(|| "Qoder 授权未返回账号身份".to_string())?;
        let mut next = Map::new();
        next.insert("uid".into(), user["id"].clone());
        for (source, destination) in [
            ("name", "name"),
            ("avatarUrl", "avatar_url"),
            ("data_policy_agreed", "data_policy_agreed"),
            ("is_data_policy_modifiable", "is_data_policy_modifiable"),
            ("user_type", "user_type"),
            ("is_personal_version", "is_personal_version"),
        ] {
            if let Some(value) = user.get(source) {
                next.insert(destination.into(), value.clone());
            }
        }
        next.insert(
            "security_oauth_token".into(),
            Value::String(account.access_token.clone()),
        );
        next.insert(
            "access_token".into(),
            Value::String(account.access_token.clone()),
        );
        if let Some(refresh) = account.refresh_token.as_ref().filter(|s| !s.is_empty()) {
            next.insert("refresh_token".into(), Value::String(refresh.clone()));
        }
        for (value, key) in [
            (&account.expires_at, "expire_time"),
            (&account.refresh_expires_at, "refresh_token_expire_time"),
        ] {
            if let Some(time) = value
                .as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            {
                next.insert(key.into(), Value::from(time.timestamp()));
            }
        }
        next.insert("login_method".into(), Value::String("browser".into()));
        next.insert(
            "login_timestamp".into(),
            Value::from(chrono::Utc::now().timestamp()),
        );
        next.insert("encrypt_user_info".into(), Value::String(String::new()));
        next.insert("key".into(), Value::String(String::new()));
        *object = next;
        return Ok(());
    }
    // Both values may describe the previous account. The desktop reader does
    // not require them, so do not attach them to a newly authorized identity.
    object.remove("authenticationContext");
    object.remove("profileOverlay");
    let token_key = if desktop {
        "token"
    } else {
        ["token", "accessToken", "access_token"]
            .into_iter()
            .find(|key| object.contains_key(*key))
            .unwrap_or("token")
    };
    for key in ["token", "accessToken", "access_token"] {
        object.remove(key);
    }
    object.insert(
        token_key.into(),
        Value::String(account.access_token.clone()),
    );
    let refresh_key = if !desktop && object.contains_key("refresh_token") {
        "refresh_token"
    } else {
        "refreshToken"
    };
    for key in ["refreshToken", "refresh_token", "refreshTokenExpiresAt"] {
        object.remove(key);
    }
    if let Some(refresh) = account.refresh_token.as_ref().filter(|s| !s.is_empty()) {
        object.insert(refresh_key.into(), Value::String(refresh.clone()));
    }
    if let Some(expires) = &account.refresh_expires_at {
        object.insert(
            "refreshTokenExpiresAt".into(),
            Value::String(expires.clone()),
        );
    }
    if let Some(expires_at) = &account.expires_at {
        object.insert("expiresAt".into(), Value::String(expires_at.clone()));
    } else {
        object.remove("expiresAt");
    }
    let user = account
        .user
        .as_object()
        .filter(|u| u.contains_key("id"))
        .ok_or_else(|| "Qoder 授权未返回账号身份".to_string())?;
    let mut new_user = Map::new();
    for key in ["id", "name", "email", "phone", "avatarUrl"] {
        if let Some(value) = user.get(key) {
            new_user.insert(key.into(), value.clone());
        }
    }
    if desktop || object.get("user").is_some_and(Value::is_object) {
        object.remove("uid");
        object.remove("user_id");
        object.insert("user".into(), Value::Object(new_user));
    } else {
        object.remove("user_id");
        object.insert("uid".into(), new_user.remove("id").unwrap());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn authorization_does_not_inherit_another_accounts_tokens_or_identity() {
        let mut old = json!({"token":"old", "refreshToken":"old-refresh", "refreshTokenExpiresAt":"old", "authenticationContext":"old-context", "profileOverlay":{"about":"old-profile"}, "user":{"id":"old", "email":"old@example.test"}, "device":"keep"});
        let account = AuthorizedAccount {
            access_token: "new".into(),
            refresh_token: None,
            expires_at: None,
            refresh_expires_at: None,
            user: json!({"id":"new", "name":"new"}),
        };
        update_profile(&mut old, &account, true).unwrap();
        assert_eq!(old["token"], "new");
        assert_eq!(old["user"]["id"], "new");
        assert!(old.get("refreshToken").is_none());
        assert!(old.get("refreshTokenExpiresAt").is_none());
        assert!(old.get("authenticationContext").is_none());
        assert!(old.get("profileOverlay").is_none());
        assert!(old["user"].get("email").is_none());
        assert_eq!(old["device"], "keep");
    }

    #[test]
    fn identity_matches_between_clients_but_never_across_regions() {
        assert_eq!(
            identity_key(QoderTarget::Desktop, "account-1"),
            identity_key(QoderTarget::Cli, "account-1")
        );
        assert_eq!(
            identity_key(QoderTarget::GlobalDesktop, "account-1"),
            identity_key(QoderTarget::GlobalCli, "account-1")
        );
        assert_ne!(
            identity_key(QoderTarget::Desktop, "account-1"),
            identity_key(QoderTarget::GlobalDesktop, "account-1")
        );
        assert_ne!(
            identity_key(QoderTarget::Desktop, "account-1"),
            identity_key(QoderTarget::Desktop, "account-2")
        );
    }

    #[test]
    fn cli_snapshot_is_encrypted_and_does_not_modify_live_credentials() {
        let temp = std::env::temp_dir().join(format!("wb-qoder-auth-{}", uuid::Uuid::new_v4()));
        let roots = QoderRoots {
            home: temp.join("home"),
            app_data: temp.join("app"),
        };
        let auth_dir = roots.cli_dir_for(QoderTarget::Cli).join(".auth");
        std::fs::create_dir_all(&auth_dir).unwrap();
        let before = br#"{"security_oauth_token":"old-secret","access_token":"old-secret","refresh_token":"old-refresh","uid":"old","name":"Old","encrypt_user_info":"old-user-data","key":"old-key","startup_validation":{"checked_at_ms":1}}"#;
        std::fs::write(auth_dir.join("user"), before).unwrap();
        std::fs::write(auth_dir.join("machine_id"), b"0123456789abcdef-machine").unwrap();
        let account = AuthorizedAccount {
            access_token: "new-secret".into(),
            refresh_token: Some("new-refresh".into()),
            expires_at: None,
            refresh_expires_at: None,
            user: json!({"id":"new-user", "name":"New"}),
        };
        let payload = build_snapshot(
            &roots,
            QoderTarget::Cli,
            &account,
            "0123456789abcdef-machine",
        )
        .unwrap();
        assert_eq!(std::fs::read(auth_dir.join("user")).unwrap(), before);
        assert_eq!(payload[0].0, "user");
        assert!(!payload[0]
            .1
            .windows(b"new-secret".len())
            .any(|w| w == b"new-secret"));
        assert!(payload.iter().any(|(name, _)| name == "machine_id"));
        let mut encrypted = STANDARD.decode(&payload[0].1).unwrap();
        let plaintext = cbc::Decryptor::<aes::Aes128>::new_from_slices(
            b"0123456789abcdef",
            b"0123456789abcdef",
        )
        .unwrap()
        .decrypt_padded_mut::<Pkcs7>(&mut encrypted)
        .unwrap();
        let result: Value = serde_json::from_slice(plaintext).unwrap();
        assert_eq!(result["access_token"], "new-secret");
        assert_eq!(result["security_oauth_token"], "new-secret");
        assert_eq!(result["uid"], "new-user");
        assert_eq!(result["name"], "New");
        assert_eq!(result["refresh_token"], "new-refresh");
        assert_eq!(result["encrypt_user_info"], "");
        assert_eq!(result["key"], "");
        assert!(result.get("startup_validation").is_none());
        std::fs::remove_dir_all(&temp).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn fresh_desktop_reuses_existing_local_state_and_refuses_corruption() {
        let temp = std::env::temp_dir().join(format!("wb-qoder-fresh-{}", uuid::Uuid::new_v4()));
        let roots = QoderRoots {
            home: temp.join("home"),
            app_data: temp.join("app"),
        };
        let target = QoderTarget::Desktop;
        let account = AuthorizedAccount {
            access_token: "test-access".into(),
            refresh_token: Some("test-refresh".into()),
            expires_at: Some("2099-01-01T00:00:00Z".into()),
            refresh_expires_at: None,
            user: json!({"id":"test-id"}),
        };
        let payload = build_snapshot(&roots, target, &account, "0123456789abcdef-device").unwrap();
        assert!(!roots.desktop_dir_for(target).exists());
        let state = &payload
            .iter()
            .find(|(name, _)| name == "Local State")
            .unwrap()
            .1;
        let root = roots.desktop_dir_for(target);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Local State"), state).unwrap();
        let again = build_snapshot(&roots, target, &account, "0123456789abcdef-device").unwrap();
        assert_eq!(
            &again
                .iter()
                .find(|(name, _)| name == "Local State")
                .unwrap()
                .1,
            state
        );
        assert!(!root.join("auth.v1.dat").exists());
        let existing = br#"{"unrelated":{"setting":42}}"#;
        std::fs::write(root.join("Local State"), existing).unwrap();
        let merged = build_snapshot(&roots, target, &account, "0123456789abcdef-device").unwrap();
        let merged_state = &merged
            .iter()
            .find(|(name, _)| name == "Local State")
            .unwrap()
            .1;
        let state_json: Value = serde_json::from_slice(merged_state).unwrap();
        assert_eq!(state_json["unrelated"]["setting"], 42);
        assert!(state_json["os_crypt"]["encrypted_key"].is_string());
        assert_eq!(std::fs::read(root.join("Local State")).unwrap(), existing);
        std::fs::write(root.join("Local State"), b"invalid").unwrap();
        assert!(build_snapshot(&roots, target, &account, "0123456789abcdef-device").is_err());
        assert_eq!(std::fs::read(root.join("Local State")).unwrap(), b"invalid");
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn desktop_snapshot_uses_the_installed_clients_encryption_key() {
        use windows::{
            core::PCWSTR,
            Win32::{
                Foundation::{LocalFree, HLOCAL},
                Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB},
            },
        };
        let temp =
            std::env::temp_dir().join(format!("wb-qoder-desktop-auth-{}", uuid::Uuid::new_v4()));
        let roots = QoderRoots {
            home: temp.join("home"),
            app_data: temp.join("app"),
        };
        let key = [42u8; 32];
        let input = CRYPT_INTEGER_BLOB {
            cbData: key.len() as u32,
            pbData: key.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(&input, PCWSTR::null(), None, None, None, 0, &mut output).unwrap();
        }
        let protected =
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
        unsafe {
            let _ = LocalFree(HLOCAL(output.pbData as _));
        }
        let key_blob = STANDARD.encode([b"DPAPI".as_slice(), protected.as_slice()].concat());
        for target in [QoderTarget::Desktop, QoderTarget::GlobalDesktop] {
            let dir = roots.desktop_dir_for(target);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("Local State"),
                serde_json::to_vec(&json!({"os_crypt":{"encrypted_key":key_blob}})).unwrap(),
            )
            .unwrap();
            let descriptor = safe_storage_target(target);
            let before = encrypt_secret_payload(
                br#"{"token":"old","user":{"id":"old"},"device":"keep"}"#,
                None,
                &dir,
                &descriptor,
            )
            .unwrap();
            std::fs::write(dir.join("auth.v1.dat"), &before).unwrap();
            let account = AuthorizedAccount {
                access_token: "new-secret".into(),
                refresh_token: Some("new-refresh".into()),
                expires_at: None,
                refresh_expires_at: None,
                user: json!({"id":"new-user", "name":"New"}),
            };
            let payload = build_snapshot(&roots, target, &account, "unused-device").unwrap();
            assert_eq!(std::fs::read(dir.join("auth.v1.dat")).unwrap(), before);
            let decoded = decrypt_secret_payload(&payload[0].1, &dir, &descriptor).unwrap();
            let result: Value = serde_json::from_slice(&decoded).unwrap();
            assert_eq!(result["token"], "new-secret");
            assert_eq!(result["user"]["id"], "new-user");
            assert_eq!(result["device"], "keep");
            assert!(payload.iter().any(|(name, _)| name == "Local State"));
        }
        std::fs::remove_dir_all(&temp).unwrap();
    }
}
