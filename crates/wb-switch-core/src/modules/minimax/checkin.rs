//! MiniMax's daily panel/claim protocol. Reference evidence is recorded in
//! docs/research/minimax-parity.md; no third-party implementation is vendored.
use super::*;
use std::future::Future;

const STATUS: &str = "/minimax-cloud/api/v1/signin/status";
const CLAIM: &str = "/minimax-cloud/api/v1/signin/claim";

fn number(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}
fn data(body: &Value) -> Result<&Value, String> {
    let code = number(&body["base_resp"]["status_code"]).or_else(|| number(&body["code"]));
    if code.is_some_and(|c| c != 0) {
        return Err(if matches!(code, Some(401 | 403 | 1000048 | 1000004)) {
            "登录已失效，请重新登录".into()
        } else {
            format!("MiniMax 签到请求失败（业务码 {}）", code.unwrap())
        });
    }
    body.get("data")
        .filter(|v| v.is_object())
        .ok_or_else(|| "签到响应缺少有效数据".into())
}
fn panel(body: &Value) -> Result<Value, String> {
    let data = data(body)?;
    let days = data["days"].as_array().ok_or("签到响应缺少每日状态")?;
    let today: Vec<_> = days.iter().filter(|d| d["is_today"] == true).collect();
    if today.len() > 1
        || days
            .iter()
            .filter(|d| number(&d["status"]) == Some(2))
            .count()
            > 1
    {
        return Err("签到状态有歧义，已停止领取".into());
    }
    let Some(day) = today.first() else {
        return Ok(json!({"state":"closed","claimable":false,"todayCheckedIn":false}));
    };
    let status = number(&day["status"])
        .filter(|v| (1..=4).contains(v))
        .ok_or("未知签到状态")?;
    let day_no = number(&day["day_no"])
        .filter(|v| (1..=7).contains(v))
        .ok_or("签到天数无效")?;
    let points = number(&day["points"])
        .filter(|v| *v >= 0)
        .ok_or("签到积分无效")?;
    let bonus = number(&day["bonus_points"]).unwrap_or(0).max(0);
    Ok(
        json!({"state":if status==3{"already"}else if status==2{"available"}else{"closed"},
        "todayCheckedIn":status==3,"claimable":status==2,"dayNo":day_no,"points":points.saturating_add(bonus)}),
    )
}
fn outcome(state: &str, message: &str, points: Option<i64>) -> Value {
    json!({"ok":matches!(state,"claimed"|"already"),"state":state,"message":message,
        "credits":points,"authInvalid":state=="auth","retryable":matches!(state,"failed"|"unconfirmed"|"busy")})
}

async fn claim_with<F, Fut>(mut request: F) -> Result<Value, String>
where
    F: FnMut(bool) -> Fut,
    Fut: Future<Output = Result<Value, String>>,
{
    let before = panel(&request(false).await?)?;
    if before["todayCheckedIn"] == true {
        return Ok(outcome("already", "今日已领取", Some(0)));
    }
    if before["claimable"] != true {
        return Ok(outcome("closed", "官方暂未开放今日领取", None));
    }
    // Exactly one POST. A timeout or malformed receipt is followed only by a GET.
    let receipt = request(true).await;
    let confirmed = request(false).await.ok().and_then(|v| panel(&v).ok());
    let valid_receipt = receipt.as_ref().ok().and_then(|v| data(v).ok());
    let result = valid_receipt.and_then(|v| number(&v["claim_result"]));
    if confirmed
        .as_ref()
        .is_some_and(|v| v["todayCheckedIn"] == true && v["dayNo"] == before["dayNo"])
    {
        if result == Some(2) {
            return Ok(outcome("already", "今日已领取", Some(0)));
        }
        let points = valid_receipt.filter(|_| result == Some(1)).and_then(|v| {
            let p = number(&v["points"])?;
            let b = number(&v["bonus_points"]).unwrap_or(0);
            (p >= 0 && b >= 0).then(|| p.saturating_add(b))
        });
        return Ok(outcome("claimed", "今日领取已确认", points));
    }
    Ok(outcome(
        "unconfirmed",
        "领取尚未确认，请稍后检查；本轮不会重复提交",
        None,
    ))
}
async fn request(region: Region, token: &str, post: bool) -> Result<Value, String> {
    let mut url = reqwest::Url::parse(&format!(
        "{}{}",
        region.origin(),
        if post { CLAIM } else { STATUS }
    ))
    .map_err(|_| "签到地址无效")?;
    let zone = iana_time_zone::get_timezone().map_err(|_| "无法确定本机时区，已停止签到")?;
    url.query_pairs_mut().append_pair("timezone_id", &zone);
    let headers = HashMap::from([
        ("Authorization".into(), format!("Bearer {token}")),
        ("Accept".into(), "application/json".into()),
    ]);
    let (status, _, body) = crate::modules::config::http_request_raw(
        url.as_str(),
        if post { "POST" } else { "GET" },
        post.then(|| json!({})),
        Some(&headers),
        None,
        false,
    )
    .await;
    if matches!(status, 401 | 403) {
        return Err("登录已失效，请重新登录".into());
    }
    if !(200..300).contains(&status) {
        return Err(format!("MiniMax 签到请求失败（HTTP {status}）"));
    }
    if body.len() > 1024 * 1024 {
        return Err("签到响应过大".into());
    }
    serde_json::from_str(&body).map_err(|_| "签到响应无法解析".into())
}
impl Engine {
    pub(super) async fn checkin_status(
        &self,
        id: &str,
        timeout: Option<u64>,
    ) -> Result<Value, String> {
        tokio::time::timeout(
            Duration::from_secs(timeout.unwrap_or(30).clamp(5, 600)),
            async {
                let (account, credential) = self.usable_credential(id).await?;
                panel(&request(account.region, &credential.access_token, false).await?)
            },
        )
        .await
        .map_err(|_| "查询签到状态超时".to_string())?
    }
    pub(super) async fn checkin(
        &self,
        id: &str,
        timeout: Option<u64>,
        source: &str,
    ) -> Result<Value, String> {
        let _claim = StateLock::try_at(
            &self
                .paths
                .store
                .join(format!("claim-{}.lock", short_hash(id))),
        )
        .map_err(|_| "该账号正在领取，请稍后查询结果")?;
        let mut region: Option<Region> = None;
        let result = tokio::time::timeout(
            Duration::from_secs(timeout.unwrap_or(60).clamp(5, 600)),
            async {
                let (account, credential) = self.usable_credential(id).await?;
                region = Some(account.region);
                claim_with(|post| request(account.region, &credential.access_token, post)).await
            },
        )
        .await;
        let mut value = match result {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => outcome(
                if error.contains("重新登录") {
                    "auth"
                } else {
                    "failed"
                },
                &error,
                None,
            ),
            Err(_) => outcome(
                "unconfirmed",
                "签到请求超时，结果尚未确认；稍后重试会先查询状态",
                None,
            ),
        };
        if value["ok"] == true {
            match self.credit(id, timeout).await {
                Ok(credit) => value["balance"] = credit,
                Err(_) => value["warning"] = json!("领取已确认，余额刷新失败，请稍后刷新积分"),
            }
        }
        let state = value
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("failed");
        let message = value.get("message").and_then(Value::as_str).unwrap_or("");
        crate::modules::claim_ledger::record(
            crate::modules::claim_ledger::PLATFORM_MINIMAX,
            region.map(Region::text),
            id,
            crate::modules::claim_ledger::SCOPE_REWARDS,
            source,
            crate::modules::claim_ledger::Outcome::single(state, message),
        );
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn status(s: i64) -> Value {
        json!({"base_resp":{"status_code":0},"data":{"days":[{"is_today":true,"day_no":1,"status":s,"points":400}]}})
    }
    #[tokio::test]
    async fn claims_once_and_requires_status_confirmation() {
        for after in [2, 3] {
            let mut calls = Vec::new();
            let mut replies = vec![
                status(2),
                json!({"data":{"claim_result":1,"points":400}}),
                status(after),
            ]
            .into_iter();
            let result = claim_with(|post| {
                calls.push(post);
                std::future::ready(Ok(replies.next().unwrap()))
            })
            .await
            .unwrap();
            assert_eq!(calls, vec![false, true, false]);
            assert_eq!(result["ok"], after == 3);
        }
    }
    #[tokio::test]
    async fn already_and_closed_do_not_send_claims() {
        for s in [1, 3, 4] {
            let mut calls = 0;
            let r = claim_with(|post| {
                assert!(!post);
                calls += 1;
                std::future::ready(Ok(status(s)))
            })
            .await
            .unwrap();
            assert_eq!(calls, 1);
            assert_eq!(r["ok"], s == 3);
        }
    }
    #[tokio::test]
    async fn lost_receipt_is_checked_without_replaying_post() {
        let mut replies =
            vec![Ok(status(2)), Err("network failure".into()), Ok(status(3))].into_iter();
        let result = claim_with(|_| std::future::ready(replies.next().unwrap()))
            .await
            .unwrap();
        assert_eq!(result["state"], "claimed");
        assert!(result["credits"].is_null());
    }
    #[test]
    fn rejects_ambiguous_panels_and_business_errors() {
        let mut p = status(2);
        let row = p["data"]["days"][0].clone();
        p["data"]["days"].as_array_mut().unwrap().push(row);
        assert!(panel(&p).is_err());
        assert!(panel(&json!({"base_resp":{"status_code":401},"data":{"days":[]}})).is_err());
        assert!(panel(&json!({"data":{}})).is_err());
    }
}
