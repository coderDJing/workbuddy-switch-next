//! 账号身份补全（uid / 昵称 / 邮箱）。
//!
//! 设备码授权拿到的令牌是「新登录」的，但授权响应里通常不含 `sub`；
//! 客户端本地的 `auth.json` 里也没有邮箱和 uid。因此登录成功后
//! 用新鲜令牌问一次业务接口，把身份补齐，避免账号卡片退化成
//! 「MiniMax CN 账号 2 / epoch:<uuid>」这种不可辨认的占位符。
//!
//! 端点与调用方式取自官方桌面端产物（`subscription-quota.js` / `transport.js`）：
//! - `GET  /v1/api/user/info`：`data.userInfo.realUserID`，以及昵称/邮箱；
//! - `POST /matrix/api/v1/user/get_user_extra_info`：个人 workspace 归属。
//!
//! 两个请求都必须带 [`super::sign`] 的签名头和一组固定查询参数，否则统一返回
//! `statusInfo.code = 2`。任何一步失败都只是「拿不到身份」，绝不影响登录本身。
use serde_json::{json, Value};

use super::paths::Region;
use super::sign;
use crate::modules::config::http_request;

const UID_KEYS: &[&str] = &[
    "realUserID",
    "real_user_id",
    "user_id",
    "userId",
    "uid",
    "account_id",
    "accountId",
    "subject",
    "sub",
];
const NAME_KEYS: &[&str] = &[
    "subUserName",
    "sub_user_name",
    "userName",
    "user_name",
    "nickname",
    "nick_name",
    "nickName",
    "display_name",
    "displayName",
    "name",
];
const EMAIL_KEYS: &[&str] = &["email", "userEmail", "user_email", "mail", "emailAddress"];

/// 每条端点的探测记录；只含字段名与业务码，不含凭据或字段值。
#[derive(Default)]
pub struct Probe {
    pub notes: Vec<String>,
}

#[derive(Default, Clone)]
pub struct Identity {
    pub uid: Option<String>,
    pub name: Option<String>,
    pub email: Option<String>,
}

impl Identity {
    pub fn is_empty(&self) -> bool {
        self.uid.is_none() && self.name.is_none() && self.email.is_none()
    }
}

fn text(value: &Value) -> Option<String> {
    let raw = match value {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        _ => return None,
    };
    if raw.is_empty() || raw.eq_ignore_ascii_case("null") || raw == "0" {
        return None;
    }
    Some(raw)
}

/// 在对象里按优先级查找键；`depth` 限制嵌套层数，避免在超大响应里乱翻。
fn find(value: &Value, keys: &[&str], depth: u8) -> Option<String> {
    let map = value.as_object()?;
    for key in keys {
        if let Some(found) = map.get(*key).and_then(text) {
            return Some(found);
        }
    }
    if depth == 0 {
        return None;
    }
    map.values()
        .find_map(|nested| find(nested, keys, depth - 1))
}

/// 响应看起来像「成功」吗？依次看 `statusInfo.code`（matrix 信封）、
/// `base_resp.status_code`、顶层 `code`。
fn succeeded(body: &Value) -> bool {
    if body.is_null() {
        return false;
    }
    for pointer in ["/statusInfo/code", "/base_resp/status_code"] {
        if let Some(code) = body.pointer(pointer).and_then(Value::as_i64) {
            return code == 0;
        }
    }
    if let Some(code) = body.get("code").and_then(Value::as_i64) {
        // 传输层失败是 -1，HTTP 错误是三位状态码；0/200 才算成功。
        return code == 0 || code == 200;
    }
    true
}

fn describe(value: &Value) -> String {
    let Some(map) = value.as_object() else {
        return if value.is_null() {
            "no-json".to_string()
        } else {
            format!("type={}", value_type(value))
        };
    };
    if map.is_empty() {
        return "empty-object".to_string();
    }
    let mut parts = Vec::new();
    for key in ["code", "status_code", "message", "msg"] {
        match map.get(key) {
            Some(Value::Number(number)) => parts.push(format!("{key}={number}")),
            Some(Value::String(text)) => parts.push(format!(
                "{key}={}",
                text.chars().take(40).collect::<String>()
            )),
            _ => {}
        }
    }
    let keys: Vec<String> = map
        .iter()
        .take(6)
        .map(|(key, nested)| match nested.as_object() {
            Some(nested) if !nested.is_empty() => {
                let inner: Vec<String> = nested
                    .iter()
                    .take(5)
                    .map(|(field, value)| match value {
                        Value::Number(number) => format!("{field}={number}"),
                        _ => field.clone(),
                    })
                    .collect();
                format!("{key}({})", inner.join(","))
            }
            _ => key.clone(),
        })
        .collect();
    parts.push(format!("keys={}", keys.join(",")));
    parts.join(" ")
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// 复刻客户端 `transport.matrix`：固定查询参数 + 签名头 + Bearer。
async fn matrix(
    region: Region,
    token: &str,
    user_id: Option<&str>,
    path: &str,
    body: Option<Value>,
) -> Value {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let language = if region == Region::Cn { "zh" } else { "en" };
    let offset_minutes = chrono::Local::now().offset().local_minus_utc() / 60;
    let query = format!(
        "device_platform=web&biz_id=3&app_id=3001&version_code=22201&unix={now_ms}\
         &timezone_offset={offset_minutes}&sys_language={language}&lang={language}\
         &device_id=0&user_id={}&client=desktop&region={}&is_desktop=1",
        user_id.unwrap_or("0"),
        region.text()
    );
    let (payload, body_value) = match body {
        Some(value) => (value.to_string(), Some(value)),
        None => (String::new(), None),
    };
    let (yy, stamp, signature) = sign::headers(&format!("{path}?{query}"), &payload, now_ms);
    let mut headers = std::collections::HashMap::new();
    headers.insert("Authorization".to_string(), format!("Bearer {token}"));
    headers.insert("Accept".to_string(), "application/json".to_string());
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    headers.insert("yy".to_string(), yy);
    headers.insert("x-timestamp".to_string(), stamp);
    headers.insert("x-signature".to_string(), signature);
    let url = format!("{}{path}?{query}", region.origin());
    let method = if body_value.is_some() { "POST" } else { "GET" };
    http_request(&url, method, body_value, Some(&headers)).await
}

/// 访问令牌若是 JWT，本地解出 `sub` 等声明即可，无需任何网络请求。
fn from_jwt(token: &str) -> Option<Identity> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let mut parts = token.split('.');
    let payload = parts.next()?;
    parts.next()?;
    let raw = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: Value = serde_json::from_slice(&raw).ok()?;
    let identity = Identity {
        uid: find(&claims, UID_KEYS, 0),
        name: find(&claims, NAME_KEYS, 0),
        email: find(&claims, EMAIL_KEYS, 0),
    };
    (!identity.is_empty()).then_some(identity)
}

/// 用访问令牌查一次身份。失败返回 `None`，永不返回错误字符串——
/// 调用方把它当成「可选增强」，不该因身份查不到就判定登录失败。
pub async fn fetch(region: Region, token: &str) -> (Option<Identity>, Probe) {
    let mut probe = Probe::default();
    if let Some(identity) = from_jwt(token) {
        probe.notes.push("jwt: ok".to_string());
        return (Some(identity), probe);
    }
    probe.notes.push("jwt: not-a-jwt".to_string());

    let mut identity = Identity::default();
    let info = matrix(region, token, None, "/v1/api/user/info", None).await;
    if succeeded(&info) {
        let data = info.get("data").unwrap_or(&info);
        let user = data
            .get("userInfo")
            .or_else(|| data.get("user_info"))
            .unwrap_or(data);
        identity.uid = find(user, UID_KEYS, 0);
        identity.name = find(user, NAME_KEYS, 0);
        identity.email = find(user, EMAIL_KEYS, 0);
        probe.notes.push(if identity.is_empty() {
            format!("user/info: no identity, {}", describe(&info))
        } else {
            "user/info: ok".to_string()
        });
    } else {
        probe.notes.push(format!("user/info: {}", describe(&info)));
    }

    if identity.uid.is_some() {
        return (Some(identity), probe);
    }
    // 兜底：个人 workspace 归属里也可能带 uid / 昵称。
    let extra = matrix(
        region,
        token,
        identity.uid.as_deref(),
        "/matrix/api/v1/user/get_user_extra_info",
        Some(json!({})),
    )
    .await;
    if succeeded(&extra) {
        identity.uid = find(&extra, UID_KEYS, 3);
        identity.name = find(&extra, NAME_KEYS, 3);
        identity.email = find(&extra, EMAIL_KEYS, 3);
        probe.notes.push(if identity.is_empty() {
            format!("extra_info: no identity, {}", describe(&extra))
        } else {
            "extra_info: ok".to_string()
        });
    } else {
        probe
            .notes
            .push(format!("extra_info: {}", describe(&extra)));
    }
    ((!identity.is_empty()).then_some(identity), probe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_fields_under_nested_wrappers() {
        let body = json!({
            "base_resp": {"status_code": 0},
            "user_info": {"user_id": 576404468350857218i64, "nickname": "coder", "email": "a@b.c"}
        });
        assert_eq!(
            find(&body, UID_KEYS, 3).as_deref(),
            Some("576404468350857218")
        );
        assert_eq!(find(&body, NAME_KEYS, 3).as_deref(), Some("coder"));
        assert_eq!(find(&body, EMAIL_KEYS, 3).as_deref(), Some("a@b.c"));
    }

    #[test]
    fn reads_the_client_user_info_shape() {
        let body = json!({
            "statusInfo": {"code": 0},
            "data": {"userInfo": {"realUserID": "576404468350857218", "userName": "coder", "userEmail": "a@b.c"}}
        });
        assert!(succeeded(&body));
        let data = body.get("data").unwrap();
        let user = data
            .get("userInfo")
            .or_else(|| data.get("user_info"))
            .unwrap();
        assert_eq!(
            find(user, UID_KEYS, 0).as_deref(),
            Some("576404468350857218")
        );
        assert_eq!(find(user, NAME_KEYS, 0).as_deref(), Some("coder"));
        assert_eq!(find(user, EMAIL_KEYS, 0).as_deref(), Some("a@b.c"));
    }

    #[test]
    fn prefers_the_most_specific_name_key() {
        let body = json!({"name": "display", "nickname": "nick"});
        assert_eq!(find(&body, NAME_KEYS, 3).as_deref(), Some("nick"));
    }

    #[test]
    fn rejects_error_and_empty_bodies() {
        assert!(!succeeded(&Value::Null));
        assert!(!succeeded(&json!({"code": -1, "message": "connect"})));
        assert!(!succeeded(&json!({"base_resp": {"status_code": 401}})));
        assert!(!succeeded(&json!({"statusInfo": {"code": 2}})));
        assert!(succeeded(&json!({"base_resp": {"status_code": 0}})));
        assert!(succeeded(&json!({"statusInfo": {"code": 0}})));
        assert!(succeeded(&json!({"code": 200})));
        assert!(succeeded(&json!({"user_id": "1"})));
    }

    #[test]
    fn placeholder_uids_are_not_identity() {
        let body = json!({"realUserID": "", "userName": "  ", "userEmail": "null"});
        assert!(Identity {
            uid: find(&body, UID_KEYS, 0),
            name: find(&body, NAME_KEYS, 0),
            email: find(&body, EMAIL_KEYS, 0),
        }
        .is_empty());
        // `user_id=0` 是客户端对「还不知道自己是谁」的占位值。
        assert!(find(&json!({"user_id": "0"}), UID_KEYS, 0).is_none());
    }
}
