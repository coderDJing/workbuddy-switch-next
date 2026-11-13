use super::*;
const ORIGIN: &str = "https://www.doubao.com";
const QUOTA: &str = "/alice/commerce/sale/subscription/quota/summary/";
const DETAIL: &str = "/alice/ug/campaign/v3/detail/";
/// 非 v3 活动组用列表接口取详情（与官方前端一致）。
const DETAIL_LEGACY: &str = "/alice/ug/campaign/list/";
const CLAIM: &str = "/alice/ug/campaign/landing/checkin";
const LAUNCH: &str = "/alice/user/launch";
/// 已知的 v3 活动组；官方换组名时以 launch 返回值为准，这里只作兜底。
const SCENE: &str = "pc_incentive_task_mvp_v3";
const QUERY:&str="version_code=20800&language=zh&device_platform=web&aid=497858&real_aid=497858&pc_version=2.33.5&samantha_web=1&web_platform=desktop&use-olympus-account=1";
/// 只有官方业务码 710012001 才能证明已保存的登录态失效。
/// HTTP 401 也可能来自网关、代理或临时拒绝，不能据此要求用户重新登录。
pub(super) const LOGIN_EXPIRED: &str = "豆包登录态已失效，请在官方客户端重新登录并识别本机账号";
pub(super) const SESSION_UPDATED: &str = "豆包登录态已在官方客户端更新，请重新识别本机账号后再查询";
pub(super) const HTTP_UNAUTHORIZED: &str =
    "豆包拒绝本次查询（HTTP 401），请稍后重试；持续失败请重新登录并识别本机账号";
pub(super) fn auth_error(e: &str) -> bool {
    e == LOGIN_EXPIRED
}
pub(super) fn code_error(code: i64) -> String {
    match code {
        710012001 => LOGIN_EXPIRED.into(),
        710022004 => "豆包需要验证，请在官方客户端完成验证后重试".into(),
        710010702 | 712010702 => {
            "豆包暂未接受此领取请求，请在官方客户端确认活动资格或完成今日签到".into()
        }
        _ => format!("豆包接口暂不可用（状态码 {code}）"),
    }
}
async fn request(
    id: &Identity,
    path: &str,
    body: Value,
    timeout: Option<u64>,
) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout.unwrap_or(20).clamp(5, 60)))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法创建豆包连接")?;
    let url = if path == QUOTA {
        format!("{ORIGIN}{path}")
    } else {
        format!("{ORIGIN}{path}?{QUERY}")
    };
    let response=client.post(url).header("Cookie",format!("sessionid={}; sessionid_ss={}",id.session,id.session)).header("Origin",ORIGIN).header("Referer","https://www.doubao.com/pc/agent-explore").header("User-Agent","Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36 SamanthaDoubao/2.33.5").json(&body).send().await.map_err(|_|"豆包请求超时或连接失败，请稍后查询状态")?;
    if response.status().as_u16() == 401 {
        return Err(HTTP_UNAUTHORIZED.into());
    }
    if !response.status().is_success() {
        return Err(format!(
            "豆包服务暂不可用（HTTP {}）",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|v| v > 2 * 1024 * 1024)
    {
        return Err("豆包响应超过读取上限".into());
    }
    let bytes = response.bytes().await.map_err(|_| "无法读取豆包响应")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("豆包响应超过读取上限".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "豆包响应格式暂不受支持")?;
    let code = value["code"]
        .as_i64()
        .or_else(|| value["code"].as_str()?.parse().ok())
        .ok_or("豆包响应缺少业务状态")?;
    if code != 0 {
        return Err(code_error(code));
    }
    Ok(value)
}
/// 从 `POST /alice/user/launch` 的响应里取活动组；官方换组名时只有这里能跟上。
pub(super) fn parse_group(value: &Value) -> Option<String> {
    let code = value["data"]["ug_campaign_info"]["campaign_group_code"]
        .as_str()?
        .trim();
    (!code.is_empty()).then(|| code.to_string())
}

/// 详情接口随活动组变化：v3 组走 detail，其它组走 list。
pub(super) fn detail_path(group: &str) -> &'static str {
    if group == SCENE {
        DETAIL
    } else {
        DETAIL_LEGACY
    }
}

/// 领取回执里的 `data.biz_error.code`：顶层 code 为 0 也可能带业务错误。
pub(super) fn receipt_biz_code(value: &Value) -> Option<i64> {
    let biz = &value["data"]["biz_error"];
    if biz.is_null() {
        return None;
    }
    let code = biz["code"]
        .as_i64()
        .or_else(|| biz["code"].as_str()?.parse().ok())?;
    (code != 0).then_some(code)
}

/// 官方明确拒绝（资格/前置条件未满足）：今天没有可领的，重试也没用。
fn rejected(code: i64) -> bool {
    matches!(code, 710010702 | 712010702)
}

fn timestamp(v: &Value) -> Option<i64> {
    let n = v.as_i64().or_else(|| v.as_str()?.parse().ok())?;
    if n <= 0 {
        None
    } else {
        Some(if n > 1000000000000 { n / 1000 } else { n })
    }
}
pub(super) fn parse_credits(value: &Value) -> Result<Value, String> {
    let data = value["data"]
        .as_object()
        .ok_or("豆包额度响应格式暂不受支持")?;
    if !data.contains_key("window_limit_section")
        && !data.contains_key("current_subscription")
        && !data.contains_key("enterprise_window_limit_section")
    {
        return Err("豆包尚未返回可识别的额度结构".into());
    }
    let mut windows = vec![];
    for section in ["window_limit_section", "enterprise_window_limit_section"] {
        for group in value["data"][section]["window_limit_groups"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let group_name = group["feature_group_name"].as_str().unwrap_or("额度");
            for w in group["window_limits"].as_array().into_iter().flatten() {
                let used = w["used_percent"]
                    .as_f64()
                    .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 100.0);
                let kind = w["window_type"].as_i64().unwrap_or(0);
                let label = match kind {
                    1 => "当前时段",
                    2 => "近 7 天",
                    _ => "额度窗口",
                };
                windows.push(json!({"key":format!("{section}:{group_name}:{kind}"),"name":format!("{group_name} · {label}"),"usedPercent":used,"remainingPercent":used.map(|v|100.0-v),"exhausted":w["usage_exhausted"].as_bool().unwrap_or(false)||used==Some(100.0),"resetsAt":timestamp(&w["end_time"])}));
            }
        }
    }
    let remaining = windows
        .iter()
        .filter_map(|w| w["remainingPercent"].as_f64())
        .reduce(f64::min);
    let subscription = &value["data"]["current_subscription"];
    Ok(
        json!({"available":remaining.is_some(),"unit":"percent","remaining":remaining,"windows":windows,"planName":subscription["display"]["short_name"].as_str().or_else(||subscription["display"]["product_name"].as_str()),"planActive":value["data"]["member_info"]["hasActiveSubscription"].as_bool().unwrap_or(false),"planExpiresAt":timestamp(&subscription["end_time"]),"checkedAt":now()}),
    )
}
pub(super) async fn credits(id: &Identity, timeout: Option<u64>) -> Result<Value, String> {
    parse_credits(&request(id, QUOTA, json!({}), timeout).await?)
}
pub(super) fn parse_detail(v: &Value) -> Result<Value, String> {
    let tasks = v["data"]["tasks"]
        .as_array()
        .ok_or("豆包签到响应缺少任务状态")?;
    let task = tasks.iter().find(|t| t["task_kind"] == "daily_signin");
    let Some(task) = task else {
        return Ok(
            json!({"ok":false,"state":"closed","message":"该账号暂无每日签到活动","credits":null,"checkedAt":now()}),
        );
    };
    let daily = &task["daily_checkin"];
    let checked = daily["checked_in_today"]
        .as_bool()
        .ok_or("豆包未明确返回今日签到状态")?;
    Ok(
        json!({"ok":checked,"state":if checked{"already"}else{"available"},"message":if checked{"今日已领取"}else{"今日可领取"},"credits":null,"reward":daily["reward"]["reward_amount"].as_f64(),"continuousDays":daily["continuous_days"].as_i64(),"lastCheckinDate":daily["last_checkin_date"].as_str(),"earnedTotal":v["data"]["progress"]["total_amount"].as_f64(),"checkedAt":now()}),
    )
}
/// 解析活动组：官方换组名（`_v3` → `_v4`）时只有 launch 能跟上，失败则退回已知组。
async fn campaign_group(id: &Identity, timeout: Option<u64>) -> String {
    request(id, LAUNCH, json!({}), timeout)
        .await
        .ok()
        .and_then(|value| parse_group(&value))
        .unwrap_or_else(|| SCENE.to_string())
}

/// 签到状态回查：官方落库有延迟，按 0 / 1.5 / 3 秒重试三次，
/// 避免把「其实已经领到」记成待确认。
async fn readback(
    id: &Identity,
    path: &'static str,
    group: &str,
    timeout: Option<u64>,
) -> Option<Value> {
    let mut last = None;
    for delay in [0u64, 1500, 3000] {
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        let Ok(value) = request(id, path, json!({"campaign_group_code": group}), timeout).await
        else {
            continue;
        };
        let Ok(parsed) = parse_detail(&value) else {
            continue;
        };
        if parsed["state"] == "already" {
            return Some(parsed);
        }
        last = Some(parsed);
    }
    last
}

pub(super) async fn checkin(
    id: &Identity,
    submit: bool,
    timeout: Option<u64>,
) -> Result<Value, String> {
    let group = campaign_group(id, timeout).await;
    let path = detail_path(&group);
    let before =
        parse_detail(&request(id, path, json!({"campaign_group_code": group}), timeout).await?)?;
    if !submit || before["state"] != "available" {
        return Ok(before);
    }
    // One submission only. Even a lost response is followed by reads, never another claim.
    let claimed = request(id, CLAIM, json!({"scene": group}), timeout).await;
    // 先回查：回执里的 biz_error 可能只是重复提交，而状态已经落库。
    let after = readback(id, path, &group, timeout).await;
    if let Some(mut after) = after {
        if after["state"] == "already" {
            let idempotent = claimed
                .as_ref()
                .is_ok_and(|v| v["data"]["idempotent"] == true);
            after["state"] = json!(if idempotent { "already" } else { "claimed" });
            after["message"] = json!(if idempotent {
                "今日已领取"
            } else {
                "今日签到已确认"
            });
            after["credits"] = claimed
                .as_ref()
                .ok()
                .and_then(|v| v["data"]["reward_amount"].as_f64())
                .map_or(Value::Null, |v| json!(v));
            return Ok(after);
        }
    }
    // 回执带业务错误：被官方拒绝时今天没有可领的，其它情况按错误上报（可重试）。
    if let Some(code) = claimed.as_ref().ok().and_then(receipt_biz_code) {
        return Ok(json!({
            "ok": false,
            "state": if rejected(code) { "blocked" } else { "error" },
            "message": code_error(code),
            "credits": Value::Null,
            "checkedAt": now()
        }));
    }
    Ok(
        json!({"ok":false,"state":"unconfirmed","message":claimed.err().unwrap_or_else(||"领取回执尚未确认，请稍后刷新签到状态；本次不会重复提交".into()),"credits":null,"checkedAt":now()}),
    )
}
