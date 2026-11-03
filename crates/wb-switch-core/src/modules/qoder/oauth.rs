//! Qoder device authorization. Secrets stay in this process and snapshots are
//! encrypted using the selected client's own storage before being persisted.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{
    authorized::{self, AuthorizedAccount},
    paths::{QoderRoots, QoderTarget},
    store,
};

const LOGIN_LIFETIME: i64 = 300;

struct Session {
    target: QoderTarget,
    nonce: String,
    verifier: String,
    machine_id: String,
    expires: i64,
    polling: bool,
    token: Option<Value>,
    result: Option<Value>,
    error: Option<String>,
}

static SESSIONS: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct Realm {
    website: &'static str,
    openapi: &'static str,
    client_id: &'static str,
    user_agent: &'static str,
    redirect: Option<&'static str>,
}

fn realm(target: QoderTarget) -> Realm {
    if target.is_global() {
        Realm {
            website: "https://qoder.com",
            openapi: "https://openapi.qoder.sh",
            client_id: "732aef47-9cf2-46a2-95fe-4cebb5d0d1fa",
            user_agent: "Qoder/1.1.64",
            redirect: Some("qoder-app://"),
        }
    } else {
        Realm {
            website: "https://qoder.cn",
            openapi: "https://openapi.qoder.com.cn",
            client_id: "732aef47-9cf2-46a2-95fe-4cebb5d0d1fa",
            user_agent: "QoderWork/1.1.64",
            redirect: None,
        }
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn expiration(token: &Value, absolute: &str, relative: &str) -> Option<String> {
    let datetime = field(token, absolute)
        .and_then(|value| {
            if let Some(raw) = value.as_str() {
                chrono::DateTime::parse_from_rfc3339(raw)
                    .ok()
                    .map(|v| v.with_timezone(&chrono::Utc))
            } else {
                value.as_i64().and_then(|raw| {
                    let millis = if raw > 1_000_000_000_000 {
                        raw
                    } else {
                        raw.checked_mul(1000)?
                    };
                    chrono::DateTime::from_timestamp_millis(millis)
                })
            }
        })
        .or_else(|| {
            field(token, relative)
                .and_then(Value::as_i64)
                .and_then(|raw| {
                    if raw <= 0 {
                        return None;
                    }
                    let seconds = if raw > 86_400 { raw / 1000 } else { raw };
                    chrono::DateTime::from_timestamp(now().checked_add(seconds)?, 0)
                })
        });
    datetime.map(|time| time.to_rfc3339())
}

fn machine_id(target: QoderTarget, roots: &QoderRoots) -> Result<String, String> {
    let cli = roots.cli_dir_for(target).join(".auth").join("machine_id");
    let desktop = roots.desktop_dir_for(target).join("auth.machine-id");
    let paths = if target.is_desktop() {
        [&desktop, &cli]
    } else {
        [&cli, &desktop]
    };
    for path in paths {
        if let Ok(value) = std::fs::read_to_string(path) {
            let value = value.trim();
            if !value.is_empty() && value.len() <= 256 {
                return Ok(value.to_string());
            }
        }
    }
    // This ID is used only for authorization and does not modify the client.
    let dir = store::default_store();
    std::fs::create_dir_all(&dir).map_err(|_| "无法创建 Qoder 数据目录".to_string())?;
    let path = dir.join(if target.is_global() {
        "oauth-device-global"
    } else {
        "oauth-device-cn"
    });
    if let Ok(value) = std::fs::read_to_string(&path) {
        let value = value.trim();
        if uuid::Uuid::parse_str(value).is_ok() {
            return Ok(value.to_string());
        }
    }
    let value = uuid::Uuid::new_v4().to_string();
    crate::modules::config::atomic_write(&path, &value)
        .map_err(|_| "无法保存 Qoder 设备标识".to_string())?;
    Ok(value)
}

fn authorize_url(
    target: QoderTarget,
    machine: &str,
    nonce: &str,
    verifier: &str,
) -> Result<reqwest::Url, String> {
    let spec = realm(target);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = reqwest::Url::parse(&format!("{}/device/selectAccounts", spec.website))
        .map_err(|_| "无效的 Qoder 授权地址".to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("challenge", &challenge)
            .append_pair("challenge_method", "S256")
            .append_pair("nonce", nonce)
            .append_pair("client_id", spec.client_id)
            .append_pair("machine_id", machine);
        if let Some(redirect) = spec.redirect {
            query.append_pair("redirect_uri", redirect);
        }
    }
    let mut signin = reqwest::Url::parse(&format!("{}/users/sign-in", spec.website))
        .map_err(|_| "无效的 Qoder 登录地址".to_string())?;
    signin
        .query_pairs_mut()
        .append_pair("biz_variant", "qoder")
        .append_pair("oauth_callback", url.as_str());
    Ok(signin)
}

pub fn start(target: QoderTarget) -> Result<Value, String> {
    let roots = QoderRoots::real();
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let nonce = if target.is_global() {
        uuid::Uuid::new_v4().simple().to_string()
    } else {
        uuid::Uuid::new_v4().to_string()
    };
    let machine = machine_id(target, &roots)?;
    let url = authorize_url(target, &machine, &nonce, &verifier)?;
    let id = uuid::Uuid::new_v4().to_string();
    let mut map = sessions()
        .lock()
        .map_err(|_| "Qoder 登录服务暂不可用".to_string())?;
    map.retain(|_, s| s.expires > now());
    map.insert(
        id.clone(),
        Session {
            target,
            nonce,
            verifier,
            machine_id: machine,
            expires: now() + LOGIN_LIFETIME,
            polling: false,
            token: None,
            result: None,
            error: None,
        },
    );
    Ok(json!({"loginId": id, "verificationUri": url.as_str(), "expiresIn": LOGIN_LIFETIME}))
}

pub fn cancel(id: &str) {
    if let Ok(mut map) = sessions().lock() {
        map.remove(id);
    }
}

struct PollGuard(String);
impl Drop for PollGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = sessions().lock() {
            if let Some(s) = map.get_mut(&self.0) {
                s.polling = false;
            }
        }
    }
}

fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value
        .get(key)
        .or_else(|| value.get("data").and_then(|v| v.get(key)))
}

fn fail(id: &str, message: &str) -> Value {
    if let Ok(mut map) = sessions().lock() {
        if let Some(s) = map.get_mut(id) {
            s.error = Some(message.to_string());
            s.token = None;
        }
    }
    json!({"done": true, "error": message})
}

pub async fn poll(id: &str) -> Value {
    poll_with(id, &QoderRoots::real(), &store::default_store(), None).await
}

async fn poll_with(
    id: &str,
    roots: &QoderRoots,
    store_dir: &std::path::Path,
    endpoint: Option<&str>,
) -> Value {
    let (target, nonce, verifier, machine, token) = {
        let Ok(mut map) = sessions().lock() else {
            return json!({"done":true,"error":"Qoder 登录服务暂不可用"});
        };
        let Some(s) = map.get_mut(id) else {
            return json!({"done":true,"error":"登录已取消或不存在"});
        };
        if let Some(result) = &s.result {
            return json!({"done":true,"result":result});
        }
        if let Some(error) = &s.error {
            return json!({"done":true,"error":error});
        }
        if now() > s.expires {
            s.token = None;
            s.error = Some("登录已超时".into());
            return json!({"done":true,"error":"登录已超时"});
        }
        if s.polling {
            return json!({"done":false});
        }
        s.polling = true;
        (
            s.target,
            s.nonce.clone(),
            s.verifier.clone(),
            s.machine_id.clone(),
            s.token.clone(),
        )
    };
    let _guard = PollGuard(id.to_string());
    let spec = realm(target);
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(15));
    if endpoint.is_some() {
        builder = builder.no_proxy();
    }
    let client = match builder.build() {
        Ok(client) => client,
        Err(_) => return fail(id, "无法初始化 Qoder 网络连接"),
    };
    let token = match token {
        Some(value) => value,
        None => {
            let reply = client
                .get(format!(
                    "{}/api/v1/deviceToken/poll",
                    endpoint.unwrap_or(spec.openapi)
                ))
                .query(&[
                    ("nonce", nonce.as_str()),
                    ("verifier", verifier.as_str()),
                    ("challenge_method", "S256"),
                ])
                .header("User-Agent", spec.user_agent)
                .send()
                .await;
            let Ok(reply) = reply else {
                return json!({"done":false});
            };
            if matches!(reply.status().as_u16(), 202 | 404) {
                return json!({"done":false});
            }
            if !reply.status().is_success() {
                return fail(id, "Qoder 授权请求失败，请重试");
            }
            let Ok(value) = reply.json::<Value>().await else {
                return fail(id, "Qoder 授权响应格式不正确");
            };
            if field(&value, "token")
                .or_else(|| field(&value, "device_token"))
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                return json!({"done":false});
            }
            if let Ok(mut map) = sessions().lock() {
                let Some(s) = map.get_mut(id) else {
                    return json!({"done":true,"error":"登录已取消"});
                };
                s.token = Some(value.clone());
            }
            value
        }
    };
    let access = field(&token, "token")
        .or_else(|| field(&token, "device_token"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let reply = client
        .get(format!(
            "{}/api/v1/userinfo",
            endpoint.unwrap_or(spec.openapi)
        ))
        .header("User-Agent", spec.user_agent)
        .bearer_auth(access)
        .send()
        .await;
    let Ok(reply) = reply else {
        return json!({"done":false});
    };
    if !reply.status().is_success() {
        return fail(id, "无法验证 Qoder 账号身份，请重试");
    }
    let Ok(user) = reply.json::<Value>().await else {
        return fail(id, "Qoder 账号身份格式不正确");
    };
    let mut user = user
        .get("data")
        .filter(|v| v.is_object())
        .unwrap_or(&user)
        .clone();
    let Some(uid) = user
        .get("id")
        .and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        })
        .filter(|s| !s.is_empty())
    else {
        return fail(id, "Qoder 未返回账号身份，无法保存");
    };
    if let Some(object) = user.as_object_mut() {
        object.insert("id".into(), Value::String(uid.clone()));
    }
    let label = user
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&uid)
        .chars()
        .take(128)
        .collect::<String>();
    let expires_at = expiration(&token, "expires_at", "expires_in");
    let refresh_expires_at = expiration(
        &token,
        "refresh_token_expires_at",
        "refresh_token_expires_in",
    );
    let account = AuthorizedAccount {
        access_token: access.to_string(),
        refresh_token: field(&token, "refresh_token")
            .and_then(Value::as_str)
            .map(str::to_string),
        expires_at,
        refresh_expires_at,
        user,
    };
    let targets = if target.is_global() {
        [QoderTarget::GlobalDesktop, QoderTarget::GlobalCli]
    } else {
        [QoderTarget::Desktop, QoderTarget::Cli]
    };
    let key = authorized::identity_key(target, &uid);
    let mut prepared = Vec::new();
    let mut warnings = Vec::new();
    #[cfg(windows)]
    {
        let encrypted = match super::vault::seal(&account, &machine) {
            Ok(value) => value,
            Err(_) => return fail(id, "无法加密保存 Qoder 账号"),
        };
        for client in targets {
            prepared.push((client, vec![(super::vault::FILE.into(), encrypted.clone())]));
        }
    }
    #[cfg(not(windows))]
    for client in targets {
        match authorized::build_snapshot(roots, client, &account, &machine) {
            Ok(payload) => prepared.push((client, payload)),
            Err(_) if client.is_desktop() => warnings.push(format!(
                "{} 尚未准备好；请先在该客户端登录一次以初始化加密存储",
                client.label()
            )),
            Err(_) => warnings.push(format!("{} 的本机加密存储不可用", client.label())),
        }
    }
    if prepared.is_empty() {
        return fail(id, "无法为 Qoder 客户端生成加密账号，请检查本机客户端存储");
    }
    let Ok(_store_lock) = crate::modules::state_lock::StateLock::wait_at(
        &store_dir.join("switch.lock"),
        Duration::from_secs(2),
    ) else {
        return json!({"done":false});
    };
    // Serialize completion with cancellation so a cancelled dialog cannot save later.
    let Ok(mut map) = sessions().lock() else {
        return json!({"done":true,"error":"Qoder 登录服务暂不可用"});
    };
    let Some(s) = map.get_mut(id) else {
        return json!({"done":true,"error":"登录已取消"});
    };
    for client in targets {
        if store::live_primary_sha(roots, client).is_some()
            && store::current_id(store_dir, roots, client).is_none()
            && !super::auto::is_current_suppressed(store_dir, roots, client)
            && store::capture(store_dir, roots, client, None).is_err()
        {
            warnings.push(format!("未能自动导入 {} 的当前登录", client.label()));
        }
    }
    let mut clients = Vec::new();
    for (client, payload) in prepared {
        match store::save_authorized(store_dir, client, &label, &key, payload) {
            Ok(_) => {
                if super::auto::allow(store_dir, Some(&key), Some(client)).is_err() {
                    warnings.push(format!("{} 已保存，但无法更新自动识别设置", client.label()));
                }
                clients.push(client)
            }
            Err(_) => warnings.push(format!("无法保存 {} 的登录状态", client.label())),
        }
    }
    if clients.is_empty() {
        s.error = Some("无法保存 Qoder 账号".into());
        s.token = None;
        json!({"done":true,"error":"无法保存 Qoder 账号"})
    } else {
        let value = json!({"label":label, "clients":clients, "warnings":warnings});
        s.result = Some(value.clone());
        s.token = None;
        json!({"done":true,"result":value})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_expiration_accepts_rfc3339_seconds_and_milliseconds() {
        let expected = "2030-01-01T00:00:00+00:00";
        assert_eq!(
            expiration(
                &json!({"expires_at":"2030-01-01T00:00:00Z"}),
                "expires_at",
                "expires_in"
            )
            .as_deref(),
            Some(expected)
        );
        assert_eq!(
            expiration(
                &json!({"expires_at":1893456000}),
                "expires_at",
                "expires_in"
            )
            .as_deref(),
            Some(expected)
        );
        assert_eq!(
            expiration(
                &json!({"expires_at":1893456000000i64}),
                "expires_at",
                "expires_in"
            )
            .as_deref(),
            Some(expected)
        );
        let current = now();
        let sec = expiration(&json!({"expires_in":3600}), "expires_at", "expires_in").unwrap();
        let ms = expiration(&json!({"expires_in":3600000}), "expires_at", "expires_in").unwrap();
        for time in [sec, ms] {
            let timestamp = chrono::DateTime::parse_from_rfc3339(&time)
                .unwrap()
                .timestamp();
            assert!((current + 3600..=current + 3602).contains(&timestamp));
        }
    }

    #[test]
    fn device_authorization_uses_selected_client_machine_when_available() {
        let root = std::env::temp_dir().join(format!("wb-qoder-device-{}", uuid::Uuid::new_v4()));
        let roots = QoderRoots {
            home: root.join("home"),
            app_data: root.join("app"),
        };
        let desktop = roots.desktop_dir_for(QoderTarget::Desktop);
        let cli = roots.cli_dir_for(QoderTarget::Cli).join(".auth");
        std::fs::create_dir_all(&desktop).unwrap();
        std::fs::create_dir_all(&cli).unwrap();
        std::fs::write(desktop.join("auth.machine-id"), "desktop-device").unwrap();
        std::fs::write(cli.join("machine_id"), "cli-device").unwrap();
        assert_eq!(
            machine_id(QoderTarget::Desktop, &roots).unwrap(),
            "desktop-device"
        );
        assert_eq!(machine_id(QoderTarget::Cli, &roots).unwrap(), "cli-device");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    fn add_fake_desktop(roots: &QoderRoots) {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use windows::{
            core::PCWSTR,
            Win32::{
                Foundation::{LocalFree, HLOCAL},
                Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB},
            },
        };
        let key = [29u8; 32];
        let input = CRYPT_INTEGER_BLOB {
            cbData: 32,
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
        let dir = roots.desktop_dir_for(QoderTarget::Desktop);
        std::fs::create_dir_all(&dir).unwrap();
        let encrypted_key = STANDARD.encode([b"DPAPI".as_slice(), protected.as_slice()].concat());
        std::fs::write(
            dir.join("Local State"),
            serde_json::to_vec(&json!({"os_crypt":{"encrypted_key":encrypted_key}})).unwrap(),
        )
        .unwrap();
        let descriptor = authorized::safe_storage_target(QoderTarget::Desktop);
        let encrypted = crate::modules::vscode_cn_inject::encrypt_secret_payload(
            br#"{"token":"previous-token","user":{"id":"previous"}}"#,
            None,
            &dir,
            &descriptor,
        )
        .unwrap();
        std::fs::write(dir.join("auth.v1.dat"), encrypted).unwrap();
    }

    #[test]
    fn authorization_urls_are_region_scoped_and_hide_the_verifier() {
        for (target, host, redirect) in [
            (QoderTarget::Desktop, "qoder.cn", false),
            (QoderTarget::Cli, "qoder.cn", false),
            (QoderTarget::GlobalDesktop, "qoder.com", true),
            (QoderTarget::GlobalCli, "qoder.com", true),
        ] {
            let url =
                authorize_url(target, "test-machine", "test-nonce", "private-verifier").unwrap();
            assert_eq!(url.host_str(), Some(host));
            assert_eq!(url.path(), "/users/sign-in");
            let outer: HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(outer.get("biz_variant").map(String::as_str), Some("qoder"));
            let callback = reqwest::Url::parse(outer.get("oauth_callback").unwrap()).unwrap();
            assert_eq!(callback.host_str(), Some(host));
            assert_eq!(callback.path(), "/device/selectAccounts");
            let query: HashMap<_, _> = callback.query_pairs().into_owned().collect();
            assert_eq!(
                query.get("client_id").map(String::as_str),
                Some("732aef47-9cf2-46a2-95fe-4cebb5d0d1fa")
            );
            assert_eq!(
                query.get("challenge_method").map(String::as_str),
                Some("S256")
            );
            assert_eq!(query.contains_key("redirect_uri"), redirect);
            assert_ne!(
                query.get("challenge").map(String::as_str),
                Some("private-verifier")
            );
            assert!(!url.as_str().contains("private-verifier"));
        }
    }

    #[tokio::test]
    async fn device_authorization_saves_encrypted_snapshot_without_changing_the_client() {
        use std::io::{Read, Write};
        let dir =
            std::env::temp_dir().join(format!("wb-qoder-oauth-test-{}", uuid::Uuid::new_v4()));
        let roots = QoderRoots {
            home: dir.join("home"),
            app_data: dir.join("app"),
        };
        let auth_dir = roots.cli_dir_for(QoderTarget::Cli).join(".auth");
        std::fs::create_dir_all(&auth_dir).unwrap();
        let original = br#"{"token":"previous-token","uid":"previous"}"#;
        std::fs::write(auth_dir.join("user"), original).unwrap();
        std::fs::write(auth_dir.join("machine_id"), b"0123456789abcdef-machine").unwrap();
        #[cfg(windows)]
        add_fake_desktop(&roots);

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for (path, body) in [
                (
                    "/api/v1/deviceToken/poll",
                    r#"{"token":"test-access-secret","refresh_token":"test-refresh-secret","expires_in":3600000}"#,
                ),
                ("/api/v1/userinfo", r#"{"id":"new-id","name":"New User"}"#),
            ] {
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
                        Err(error) => panic!("mock server did not receive request: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut request = [0u8; 8192];
                let n = stream.read(&mut request).unwrap();
                assert!(String::from_utf8_lossy(&request[..n]).starts_with(&format!("GET {path}")));
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let id = uuid::Uuid::new_v4().to_string();
        sessions().lock().unwrap().insert(
            id.clone(),
            Session {
                target: QoderTarget::Cli,
                nonce: "test-nonce".into(),
                verifier: "test-verifier".into(),
                machine_id: "0123456789abcdef-machine".into(),
                expires: now() + 60,
                polling: false,
                token: None,
                result: None,
                error: None,
            },
        );
        let result = poll_with(&id, &roots, &dir.join("store"), Some(&endpoint)).await;
        server.join().unwrap();
        assert_eq!(result["done"], true);
        assert_eq!(result["result"]["label"], "New User");
        let result_text = result.to_string();
        assert!(!result_text.contains("test-access-secret"));
        assert!(!result_text.contains("test-refresh-secret"));
        assert_eq!(std::fs::read(auth_dir.join("user")).unwrap(), original);
        let bundles = store::list(&dir.join("store"), None);
        assert_eq!(bundles.len(), if cfg!(windows) { 4 } else { 2 });
        let new_identity = authorized::identity_key(QoderTarget::Cli, "new-id");
        let bundle_id = &bundles
            .iter()
            .find(|item| {
                item.target == QoderTarget::Cli
                    && item.identity_key.as_deref() == Some(new_identity.as_str())
            })
            .unwrap()
            .id;
        #[cfg(windows)]
        {
            struct NoRunningClient;
            impl super::super::proc::QoderProcessOps for NoRunningClient {
                fn running_pids(&self, _: QoderTarget) -> Vec<u32> {
                    vec![]
                }
                fn running_exe(&self, _: QoderTarget) -> Option<std::path::PathBuf> {
                    None
                }
                fn close(&self, _: QoderTarget, _: Duration) -> Result<(), String> {
                    panic!("client is not running")
                }
                fn launch(
                    &self,
                    _: QoderTarget,
                    _: Option<std::path::PathBuf>,
                ) -> Result<(), String> {
                    panic!("client is not running")
                }
                fn hosted_by(&self, _: QoderTarget) -> bool {
                    false
                }
            }
            let new_desktop = bundles
                .iter()
                .find(|item| {
                    item.target == QoderTarget::Desktop
                        && item.identity_key.as_deref() == Some(new_identity.as_str())
                })
                .unwrap();
            let old_identity = authorized::identity_key(QoderTarget::Desktop, "previous");
            let old_desktop = bundles
                .iter()
                .find(|item| {
                    item.target == QoderTarget::Desktop
                        && item.identity_key.as_deref() == Some(old_identity.as_str())
                })
                .unwrap();
            let auth = roots
                .desktop_dir_for(QoderTarget::Desktop)
                .join("auth.v1.dat");
            let before = std::fs::read(&auth).unwrap();
            let opts = super::super::SwitchOptions {
                close_running: true,
                relaunch: true,
            };
            super::super::switch::execute(
                &dir.join("store"),
                &roots,
                &NoRunningClient,
                &new_desktop.id,
                opts,
                &|_| {},
            )
            .unwrap();
            let descriptor = authorized::safe_storage_target(QoderTarget::Desktop);
            let decoded = crate::modules::vscode_cn_inject::decrypt_secret_payload(
                &std::fs::read(&auth).unwrap(),
                &roots.desktop_dir_for(QoderTarget::Desktop),
                &descriptor,
            )
            .unwrap();
            let current: Value = serde_json::from_slice(&decoded).unwrap();
            assert_eq!(current["token"], "test-access-secret");
            super::super::switch::execute(
                &dir.join("store"),
                &roots,
                &NoRunningClient,
                &old_desktop.id,
                opts,
                &|_| {},
            )
            .unwrap();
            assert_eq!(std::fs::read(&auth).unwrap(), before);
        }
        let saved = std::fs::read(dir.join("store").join("bundles").join(bundle_id).join(
            if cfg!(windows) {
                "authorized.dpapi"
            } else {
                "user"
            },
        ))
        .unwrap();
        assert!(!saved
            .windows(b"test-access-secret".len())
            .any(|w| w == b"test-access-secret"));
        cancel(&id);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
