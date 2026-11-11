//! 非现场账号副本的令牌刷新，以及「工具内网页登录」用的设备码授权。
//!
//! 官方刷新请求（`packages/oauth-core/src/oauth-client.ts`）：
//! `grant_type=refresh_token`、`client_id=mcode-public`、`scope=agent.default`、
//! `audience=agent-backend`，发往 `{account_origin}/oauth2/token`。
//!
//! 刷新会轮换 refresh token，因此本工具只刷新账号包里那份「当前不在现场」的登录，
//! 绝不刷新正在被客户端使用的令牌，避免与客户端互相作废。
//!
//! 设备码授权（RFC 8628 + PKCE S256）实测于 2026-11-11：
//! `POST {account_origin}/oauth2/device/code` 返回 `device_code` / `user_code` /
//! `verification_uri(_complete)` / `expires_in=300` / `interval=3`；轮询同一 `/oauth2/token`
//! 端点返回 `authorization_pending`、`slow_down`、`expired_token` 等标准错误。
use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::paths::Region;

const CLIENT_ID: &str = "mcode-public";
const SCOPE: &str = "agent.default";
const AUDIENCE: &str = "agent-backend";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

pub struct Grant {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in_secs: i64,
    /// 授权响应里能拿到的账号标识（`sub` / `user_id`），拿不到就是 `None`。
    pub subject: Option<String>,
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "无法创建授权客户端".to_string())
}

fn error_code(body: &Value) -> String {
    body.get("error")
        .and_then(Value::as_str)
        .or_else(|| body.get("error_code").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

fn grant_from(body: &Value, fallback_refresh: Option<&str>) -> Result<Grant, String> {
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or("授权服务没有返回访问令牌")?
        .to_string();
    let refresh_token = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .or_else(|| fallback_refresh.map(str::to_string))
        .ok_or("授权服务没有返回刷新令牌")?;
    let expires_in_secs = body
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or("授权服务没有返回有效期")?;
    let subject = ["sub", "user_id", "userId", "account_id"]
        .iter()
        .find_map(|key| body.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Ok(Grant {
        access_token,
        refresh_token,
        expires_in_secs,
        subject,
    })
}

pub async fn refresh(region: Region, refresh_token: &str) -> Result<Grant, String> {
    let response = client()?
        .post(format!("{}/oauth2/token", region.account_origin()))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", CLIENT_ID),
            ("scope", SCOPE),
            ("audience", AUDIENCE),
        ])
        .send()
        .await
        .map_err(|_| "无法连接 MiniMax 授权服务，请检查网络".to_string())?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

    if !status.is_success() {
        return Err(if error_code(&body).eq_ignore_ascii_case("invalid_grant") {
            "账号的刷新令牌已失效，请在 MiniMax Code 客户端重新登录后识别保存".into()
        } else {
            format!("刷新令牌失败（HTTP {}）", status.as_u16())
        });
    }
    grant_from(&body, Some(refresh_token))
}

/// PKCE 对：`verifier` 留在进程内，`challenge` 发给授权服务。
pub fn pkce() -> (String, String) {
    // 两个 UUID v4 拼成 32 字节随机数，Base64URL 后正好 43 个字符（RFC 7636 下限）。
    let mut seed = [0u8; 32];
    seed[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    seed[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    let verifier = URL_SAFE_NO_PAD.encode(seed);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in_secs: i64,
    pub interval_secs: i64,
}

pub async fn device_start(region: Region) -> Result<(DeviceCode, String), String> {
    let (verifier, challenge) = pkce();
    let response = client()?
        .post(format!("{}/oauth2/device/code", region.account_origin()))
        .form(&[
            ("client_id", CLIENT_ID),
            ("scope", SCOPE),
            ("audience", AUDIENCE),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ])
        .send()
        .await
        .map_err(|_| "无法连接 MiniMax 授权服务，请检查网络".to_string())?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(format!(
            "申请授权码失败（HTTP {}）：{}",
            status.as_u16(),
            body.get("error_description")
                .or_else(|| body.get("error"))
                .and_then(Value::as_str)
                .unwrap_or("授权服务返回无法识别的内容")
                .chars()
                .take(120)
                .collect::<String>()
        ));
    }
    let field = |key: &str| {
        body.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let device_code = field("device_code").ok_or("授权服务没有返回设备码")?;
    let user_code = field("user_code").ok_or("授权服务没有返回用户码")?;
    let verification_uri = field("verification_uri")
        .or_else(|| field("verification_url"))
        .ok_or("授权服务没有返回授权地址")?;
    Ok((
        DeviceCode {
            device_code,
            user_code,
            verification_uri,
            verification_uri_complete: field("verification_uri_complete"),
            expires_in_secs: body
                .get("expires_in")
                .and_then(Value::as_i64)
                .filter(|value| *value > 0)
                .unwrap_or(300),
            interval_secs: body
                .get("interval")
                .and_then(Value::as_i64)
                .filter(|value| *value > 0)
                .unwrap_or(5),
        },
        verifier,
    ))
}

pub enum Poll {
    Pending,
    SlowDown,
    Authorized(Grant),
    Denied(String),
    Expired(String),
    Failed(String),
}

pub async fn device_poll(
    region: Region,
    device_code: &str,
    verifier: &str,
) -> Result<Poll, String> {
    let response = client()?
        .post(format!("{}/oauth2/token", region.account_origin()))
        .form(&[
            ("grant_type", DEVICE_GRANT),
            ("client_id", CLIENT_ID),
            ("device_code", device_code),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .map_err(|_| "无法连接 MiniMax 授权服务，请检查网络".to_string())?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if status.is_success() {
        return grant_from(&body, None).map(Poll::Authorized);
    }
    let code = error_code(&body).to_ascii_lowercase();
    let description = body
        .get("error_description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect::<String>();
    Ok(match code.as_str() {
        "authorization_pending" | "pending" => Poll::Pending,
        "slow_down" => Poll::SlowDown,
        "access_denied" | "denied" => Poll::Denied(if description.is_empty() {
            "授权被拒绝".into()
        } else {
            description
        }),
        "expired_token" | "expired" | "code_expired" => Poll::Expired(if description.is_empty() {
            "授权码已过期，请重新发起登录".into()
        } else {
            description
        }),
        _ => Poll::Failed(if description.is_empty() {
            format!("授权失败（HTTP {}）", status.as_u16())
        } else {
            description
        }),
    })
}
