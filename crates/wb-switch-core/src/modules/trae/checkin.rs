//! 国内版每日签到。协议对齐 L0NE-6/Trae-AutoCheckin `d289f0d`：
//! 只用客户端已经注册的设备号，只发送小写 `x-device-id`，请求体固定
//! `req_source=1`。9074 不连发。不生成设备号，也不刷新令牌。
use super::protocol::Auth;
use serde_json::{json, Map, Value};

const ORIGIN: &str = "https://api.trae.cn";
const STATUS_PATH: &str = "/trae/api/v2/ug/checkin_credits/status";
const CLAIM_PATH: &str = "/trae/api/v2/ug/checkin_credits/claim";
/// 官方桌面端版本号。设备头要与客户端一致，缺失会被服务端判成 9074。
const IDE_VERSION: &str = "1.107.1";

/// 签到结果。`ok` 为真表示今天已经签到或刚刚签到成功。
#[derive(Clone, Debug)]
pub(super) struct Outcome {
    pub ok: bool,
    pub auth_invalid: bool,
    /// 限流或到账未确认，稍后可以重试。
    pub retryable: bool,
    pub message: String,
    pub credits: Option<f64>,
    pub state: &'static str,
}

impl Outcome {
    fn to_value(&self) -> Value {
        json!({"ok":self.ok,"message":self.message,"authInvalid":self.auth_invalid,
            "retryable":self.retryable,"credits":self.credits,"state":self.state})
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Ready,
    Already,
    Busy,
    Auth,
    Closed,
    Failed,
}

/// 设备号是客户端注册过的十进制串，12–20 位。生成的号会被服务端拒成 9074，
/// 所以这里只认快照里客户端自己写下的那一个。
fn valid_device_id(value: &str) -> bool {
    let trimmed = value.trim();
    (12..=20).contains(&trimmed.len()) && trimmed.bytes().all(|byte| byte.is_ascii_digit())
}

/// 取账号快照里客户端注册过的设备号。找不到就不签，不用生成的号冒充。
pub(super) fn device_id(keys: &Map<String, Value>) -> Option<String> {
    keys.keys().find_map(|key| {
        let id = key.strip_prefix("iCubeAuthInfo://icube-dc:")?;
        valid_device_id(id).then(|| id.to_string())
    })
}

fn device_type() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "Darwin"
    } else {
        "Linux"
    }
}

/// 与上游一致的兜底版本串：取不到系统版本时用固定值，不伪造具体构建号。
fn os_version() -> &'static str {
    if cfg!(target_os = "macos") {
        "23.0.0"
    } else {
        "10.0.19045"
    }
}

/// 业务码判定。签到接口用 0 表示成功，9004/9095 表示今日已签，9074 表示限流。
fn code_state(payload: &Value) -> State {
    match payload.get("code").and_then(Value::as_i64) {
        Some(1001 | 1002) => State::Auth,
        Some(9004 | 9095) => State::Already,
        Some(9074) => State::Busy,
        Some(code) if code != 0 => State::Failed,
        _ => State::Ready,
    }
}

fn checked_in(payload: &Value) -> bool {
    payload.get("checked_in").and_then(Value::as_bool) == Some(true)
        || payload.get("did_checked_in").and_then(Value::as_bool) == Some(true)
}

/// 状态接口的判定。服务端没有开放签到时不再发领取请求，避免白白消耗限流额度。
fn status_state(payload: &Value) -> State {
    match code_state(payload) {
        State::Ready => {}
        other => return other,
    }
    if checked_in(payload) {
        return State::Already;
    }
    match payload.get("enable") {
        None | Some(Value::Bool(false)) => State::Closed,
        _ => State::Ready,
    }
}

/// 本次可展示的积分：credits 优先，否则正的 extra_credits，两者不相加。
fn reward_credits(payload: &Value) -> Option<f64> {
    let credits = payload
        .get("credits")
        .and_then(Value::as_f64)
        .filter(|value| *value > 0.0);
    let extra = payload
        .get("extra_credits")
        .and_then(Value::as_f64)
        .filter(|value| *value > 0.0);
    credits.or(extra)
}

fn client(timeout: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout.clamp(1, 120)))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法创建 TRAE 签到请求".into())
}

fn request(
    client: &reqwest::Client,
    auth: &Auth,
    device_id: &str,
    path: &str,
) -> reqwest::RequestBuilder {
    client
        .post(format!("{ORIGIN}{path}"))
        .header("Authorization", format!("Cloud-IDE-JWT {}", auth.token))
        .header("X-User-Region", "CN")
        .header("User-Agent", format!("Trae/{IDE_VERSION}"))
        // 只发小写一个设备号，大小写各发一个会被判成异常客户端。
        .header("x-device-id", device_id)
        .header("x-device-type", device_type())
        .header("x-os-version", os_version())
        .header("x-app-version", IDE_VERSION)
        .json(&json!({"req_source": 1}))
}

/// 失败分类。鉴权失效要提示重新登录，限流只是排队，等下一轮再试。
enum Failure {
    Auth,
    Busy,
    Other(String),
}

async fn post(auth: &Auth, device_id: &str, path: &str, timeout: u64) -> Result<Value, Failure> {
    let client = client(timeout).map_err(Failure::Other)?;
    let response = request(&client, auth, device_id, path)
        .send()
        .await
        .map_err(|_| Failure::Other("TRAE 签到网络失败".into()))?;
    let status = response.status().as_u16();
    if matches!(status, 401 | 403) {
        return Err(Failure::Auth);
    }
    if status == 429 || (500..600).contains(&status) {
        return Err(Failure::Busy);
    }
    if !response.status().is_success() {
        return Err(Failure::Other(format!("TRAE 签到失败（HTTP {status}）")));
    }
    response
        .json()
        .await
        .map_err(|_| Failure::Other("TRAE 签到响应格式不支持".into()))
}

fn outcome(state: &'static str, ok: bool, message: &str, credits: Option<f64>) -> Outcome {
    Outcome {
        ok,
        auth_invalid: state == "auth",
        retryable: matches!(state, "busy" | "unconfirmed"),
        message: message.to_string(),
        credits,
        state,
    }
}

/// 领取一次每日积分。每轮最多发一次 claim；9074 与未确认都留给下一轮。
pub(super) async fn claim(auth: &Auth, device_id: &str, timeout: u64) -> Result<Outcome, String> {
    if auth
        .expires
        .is_some_and(|value| value <= chrono::Utc::now().timestamp())
    {
        return Err("TRAE 登录已过期，请在官方客户端重新登录并保存".into());
    }
    let status = post(auth, device_id, STATUS_PATH, timeout)
        .await
        .map_err(failure_message)?;
    match status_state(&status) {
        State::Already => {
            return Ok(outcome(
                "already",
                true,
                "今日已签到",
                reward_credits(&status),
            ))
        }
        State::Busy => return Ok(outcome("busy", false, "签到人数过多，请稍后再试", None)),
        State::Auth => {
            return Ok(outcome(
                "auth",
                false,
                "登录状态失效，请在 TRAE 客户端重新登录并保存",
                None,
            ))
        }
        State::Closed => return Ok(outcome("closed", false, "官方尚未开放今日签到", None)),
        State::Failed => {
            return Ok(outcome(
                "failed",
                false,
                "签到状态查询失败，请稍后重试",
                None,
            ))
        }
        State::Ready => {}
    }
    let claimed = post(auth, device_id, CLAIM_PATH, timeout)
        .await
        .map_err(failure_message)?;
    match code_state(&claimed) {
        State::Already => {
            return Ok(outcome(
                "already",
                true,
                "今日已签到",
                reward_credits(&status),
            ))
        }
        State::Busy => return Ok(outcome("busy", false, "签到人数过多，请稍后再试", None)),
        State::Auth => {
            return Ok(outcome(
                "auth",
                false,
                "登录状态失效，请在 TRAE 客户端重新登录并保存",
                None,
            ))
        }
        State::Closed | State::Failed => {
            return Ok(outcome("failed", false, "签到失败，请稍后重试", None))
        }
        State::Ready => {}
    }
    // 领取接口可能返回假成功：用同一个设备号回查，确认今天真的签到了。
    let verify = post(auth, device_id, STATUS_PATH, timeout)
        .await
        .map_err(failure_message)?;
    if status_state(&verify) != State::Already {
        return Ok(outcome(
            "unconfirmed",
            false,
            "领取结果未确认，请稍后重试",
            None,
        ));
    }
    let points = claimed.pointer("/data/points").and_then(Value::as_f64);
    Ok(outcome(
        "claimed",
        true,
        "签到成功",
        points.or_else(|| reward_credits(&verify)),
    ))
}

fn failure_message(failure: Failure) -> String {
    match failure {
        Failure::Auth => "TRAE 登录失效或无权签到，请在官方客户端重新登录并保存".into(),
        Failure::Busy => "TRAE 签到服务繁忙，请稍后重试".into(),
        Failure::Other(message) => message,
    }
}

pub(super) fn to_value(result: &Outcome) -> Value {
    result.to_value()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_accepts_only_registered_numeric_suffix() {
        let keys = serde_json::from_value(json!({
            "iCubeAuthInfo://icube.cloudide": {},
            "iCubeAuthInfo://icube-dc:1711320556112436": {},
            "iCubeAuthInfo://icube-dc:not-a-device": {},
            "iCubeAuthInfo://icube-dc:123": {}
        }))
        .unwrap();
        assert_eq!(device_id(&keys).as_deref(), Some("1711320556112436"));
        let keys = serde_json::from_value(json!({
            "iCubeAuthInfo://icube-dc:17113205561124365": {}
        }))
        .unwrap();
        // 21 位不是客户端注册过的号段，不能当成设备号。
        assert_eq!(device_id(&keys), None);
    }

    #[test]
    fn status_codes_match_the_reference_client() {
        assert_eq!(code_state(&json!({"code":9095})), State::Already);
        assert_eq!(code_state(&json!({"code":9004})), State::Already);
        assert_eq!(code_state(&json!({"code":9074})), State::Busy);
        assert_eq!(code_state(&json!({"code":1001})), State::Auth);
        assert_eq!(code_state(&json!({"code":1002})), State::Auth);
        assert_eq!(code_state(&json!({"code":500})), State::Failed);
        assert_eq!(code_state(&json!({"code":0})), State::Ready);
    }

    #[test]
    fn status_requires_an_open_checkin_window() {
        assert_eq!(
            status_state(&json!({"code":0,"checked_in":true,"enable":true})),
            State::Already
        );
        assert_eq!(
            status_state(&json!({"code":0,"checked_in":false,"enable":true})),
            State::Ready
        );
        assert_eq!(
            status_state(&json!({"code":0,"checked_in":false,"enable":false})),
            State::Closed
        );
        // 与上游一致：缺少 enable 视为未开放，不再发领取请求。
        assert_eq!(
            status_state(&json!({"code":0,"checked_in":false})),
            State::Closed
        );
    }

    #[test]
    fn credits_prefer_credits_and_never_add_both() {
        assert_eq!(
            reward_credits(&json!({"credits":80,"extra_credits":75})),
            Some(80.0)
        );
        assert_eq!(
            reward_credits(&json!({"credits":0,"extra_credits":75})),
            Some(75.0)
        );
        assert_eq!(
            reward_credits(&json!({"credits":-10,"extra_credits":80})),
            Some(80.0)
        );
        assert_eq!(reward_credits(&json!({"credits":"80"})), None);
        assert_eq!(reward_credits(&json!({})), None);
    }

    #[test]
    fn outcome_flags_auth_and_retryable_states() {
        let auth = outcome("auth", false, "登录状态失效", None);
        assert!(auth.auth_invalid);
        assert!(!auth.retryable);
        let busy = outcome("busy", false, "签到人数过多", None);
        assert!(busy.retryable);
        assert!(!busy.auth_invalid);
    }
}
