//! MiniMax Code 账号额度与会员信息。
//!
//! 接口与字段来自官方客户端（`packages/tui/src/account/matrix-account-client.ts`）
//! 以及第三方实测（`VICqiufeeng/dsh-minimax-connect`，MIT）：
//! - `GET  {origin}/minimax-cloud/api/v1/credit/details`：积分明细行
//!   `remaining_amount` / `granted_amount` / `consumed_amount` / `expire_at_ms`；
//! - `POST {origin}/matrix/api/v1/commerce/get_membership_info`：套餐、总剩余、
//!   `op_credit_summary.total_remaining_amount`、到期时间。
//!
//! 请求失败时不回显响应正文与凭据，只保留可读原因与业务码。
use std::collections::HashMap;

use serde_json::{json, Value};

use super::paths::Region;
use crate::modules::config::http_request;

const DETAILS_PATH: &str = "/minimax-cloud/api/v1/credit/details";
const MEMBERSHIP_PATH: &str = "/matrix/api/v1/commerce/get_membership_info";

/// 业务上表示「需要重新登录」的返回码。
const NEED_LOGIN_CODES: &[i64] = &[401, 403, 1000048, 1000004];

fn headers(token: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("Authorization".to_string(), format!("Bearer {token}"));
    map.insert("Accept".to_string(), "application/json".to_string());
    map
}

fn base_status(body: &Value) -> (Option<i64>, Option<String>) {
    let base = body.get("base_resp");
    let code = base
        .and_then(|value| value.get("status_code"))
        .and_then(Value::as_i64)
        .or_else(|| body.get("code").and_then(Value::as_i64));
    let message = base
        .and_then(|value| value.get("status_msg"))
        .and_then(Value::as_str)
        .or_else(|| body.get("message").and_then(Value::as_str))
        .map(|text| text.chars().take(120).collect::<String>());
    (code, message)
}

fn failure(body: &Value, route: &str) -> String {
    let (code, message) = base_status(body);
    if code.is_some_and(|code| NEED_LOGIN_CODES.contains(&code)) {
        return "登录已失效，请在 MiniMax Code 客户端重新登录后重新识别保存".into();
    }
    match (code, message) {
        (Some(code), Some(message)) if !message.is_empty() => {
            format!("{route} 失败（{code}）：{message}")
        }
        (Some(code), None) => format!("{route} 失败（{code}）"),
        (None, Some(message)) if !message.is_empty() => format!("{route} 失败：{message}"),
        _ => format!("{route} 返回无法识别的内容"),
    }
}

fn amount(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
        .filter(|number| number.is_finite())
}

fn seconds_from_ms(value: Option<&Value>) -> Option<i64> {
    let raw = amount(value)?;
    if raw <= 0.0 {
        return None;
    }
    Some(if raw > 1e11 {
        (raw / 1000.0) as i64
    } else {
        raw as i64
    })
}

fn details_view(body: &Value) -> Value {
    let rows = body
        .get("details")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut remaining = 0.0;
    let mut granted = 0.0;
    let mut consumed = 0.0;
    let mut expiry: Option<i64> = None;
    let mut packs = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let row_remaining = amount(row.get("remaining_amount")).unwrap_or(0.0);
        let row_granted = amount(row.get("granted_amount")).unwrap_or(0.0);
        let row_consumed = amount(row.get("consumed_amount")).unwrap_or(0.0);
        remaining += row_remaining;
        granted += row_granted;
        consumed += row_consumed;
        let expires_at = seconds_from_ms(row.get("expire_at_ms"));
        if let Some(at) = expires_at {
            expiry = Some(expiry.map_or(at, |current: i64| current.min(at)));
        }
        if row_remaining > 0.0 {
            packs.push(json!({
                "name": format!("积分包 {}", index + 1),
                "remaining": row_remaining,
                "total": row_granted.max(row_remaining),
                "used": row_consumed,
                "expiresAt": expires_at,
            }));
        }
    }
    json!({
        "rows": rows.len(),
        "remaining": remaining,
        "total": granted,
        "used": consumed,
        "expiresAt": expiry,
        "packs": packs,
    })
}

fn membership_view(body: &Value) -> Value {
    let summary = body.get("op_credit_summary");
    let remaining = amount(summary.and_then(|value| value.get("total_remaining_amount")))
        .or_else(|| amount(body.get("total_remains_credit")));
    json!({
        "planName": body.get("plan_name").and_then(Value::as_str).filter(|text| !text.is_empty()),
        "hasTokenPlan": body.get("has_token_plan").and_then(Value::as_bool).unwrap_or(false),
        "expiresAt": seconds_from_ms(body.get("expires_at")),
        "remaining": remaining,
        "freeRemaining": amount(summary.and_then(|value| value.get("free_remaining_amount"))),
        "purchasedRemaining": amount(summary.and_then(|value| value.get("purchased_remaining_amount"))),
        "dailyGiftRemaining": amount(body.get("daily_login_gift_credit_remaining")),
        "pendingCredit": amount(body.get("pending_credit")),
    })
}

/// 查询一个账号的额度。`token` 必须是仍然有效的访问令牌。
pub async fn query(region: Region, token: &str) -> Result<Value, String> {
    let origin = region.origin();
    let headers = headers(token);
    let details = http_request(
        &format!("{origin}{DETAILS_PATH}"),
        "GET",
        None,
        Some(&headers),
    )
    .await;
    let membership = http_request(
        &format!("{origin}{MEMBERSHIP_PATH}"),
        "POST",
        Some(json!({})),
        Some(&headers),
    )
    .await;

    let mut errors = Vec::new();
    let details_ok = details
        .get("base_resp")
        .and_then(|value| value.get("status_code"))
        .and_then(Value::as_i64)
        .map_or_else(
            || details.get("details").and_then(Value::as_array).is_some(),
            |code| code == 0,
        );
    if !details_ok {
        errors.push(failure(&details, "积分明细"));
    }
    let membership_ok = membership
        .get("base_resp")
        .and_then(|value| value.get("status_code"))
        .and_then(Value::as_i64)
        .map_or_else(
            || {
                membership.get("plan_name").is_some()
                    || membership.get("total_remains_credit").is_some()
            },
            |code| code == 0,
        );
    if !membership_ok {
        errors.push(failure(&membership, "套餐信息"));
    }
    if !details_ok && !membership_ok {
        return Err(errors.join("；"));
    }

    let points = details_view(&details);
    let plan = membership_view(&membership);
    let remaining = plan
        .get("remaining")
        .and_then(Value::as_f64)
        .or_else(|| points.get("remaining").and_then(Value::as_f64));
    let total = points
        .get("total")
        .and_then(Value::as_f64)
        .filter(|v| *v > 0.0);
    Ok(json!({
        "available": remaining.is_some(),
        "unit": "credits",
        "unlimited": false,
        "remaining": remaining,
        "total": total,
        "used": points.get("used").and_then(Value::as_f64),
        "checkedAt": chrono::Utc::now().timestamp(),
        "expiresAt": points.get("expiresAt").cloned().unwrap_or(Value::Null),
        "planName": plan.get("planName").cloned().unwrap_or(Value::Null),
        "planExpiresAt": plan.get("expiresAt").cloned().unwrap_or(Value::Null),
        "hasTokenPlan": plan.get("hasTokenPlan").cloned().unwrap_or(json!(false)),
        "dailyGiftRemaining": plan.get("dailyGiftRemaining").cloned().unwrap_or(Value::Null),
        "packs": points.get("packs").cloned().unwrap_or_else(|| json!([])),
        "error": if errors.is_empty() { Value::Null } else { json!(errors.join("；")) },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_aggregate_rows_and_keep_earliest_expiry() {
        let body = json!({
            "details": [
                {"remaining_amount": 10, "granted_amount": 20, "consumed_amount": 10, "expire_at_ms": 1_900_000_000_000i64},
                {"remaining_amount": 5.5, "granted_amount": 5.5, "consumed_amount": 0, "expire_at_ms": 1_800_000_000_000i64},
                {"remaining_amount": 0, "granted_amount": 8, "consumed_amount": 8}
            ]
        });
        let view = details_view(&body);
        assert_eq!(view["rows"], 3);
        assert_eq!(view["remaining"], 15.5);
        assert_eq!(view["total"], 33.5);
        assert_eq!(view["used"], 18.0);
        assert_eq!(view["expiresAt"], 1_800_000_000i64);
        assert_eq!(view["packs"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn membership_reads_token_plan_summary_and_expiry() {
        let body = json!({
            "plan_name": "M Plan 标准版",
            "has_token_plan": true,
            "expires_at": 1_900_000_000i64,
            "total_remains_credit": 99,
            "op_credit_summary": {"total_remaining_amount": 42, "free_remaining_amount": 2},
        });
        let view = membership_view(&body);
        assert_eq!(view["remaining"], 42.0);
        assert_eq!(view["planName"], "M Plan 标准版");
        assert_eq!(view["hasTokenPlan"], true);
        assert_eq!(view["expiresAt"], 1_900_000_000i64);
    }

    #[test]
    fn failures_never_echo_credentials() {
        let body = json!({"base_resp": {"status_code": 401, "status_msg": "unauthorized"}});
        assert!(failure(&body, "积分明细").contains("重新登录"));
        let obscure = json!({"message": "gateway"});
        let text = failure(&obscure, "套餐信息");
        assert!(text.contains("套餐信息"));
        assert!(!text.contains("mmort_"));
    }
}
