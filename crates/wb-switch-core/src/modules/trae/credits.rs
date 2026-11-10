use super::{protocol::Auth, Region};
use serde_json::{json, Value};

pub(super) async fn query(region: Region, auth: &Auth, timeout: u64) -> Result<Value, String> {
    if auth
        .expires
        .is_some_and(|v| v <= chrono::Utc::now().timestamp())
    {
        return Err("TRAE 登录已过期，请在官方客户端重新登录并保存".into());
    }
    // Fixed official origins only. Never send credentials to an imported host.
    let origin = match region {
        Region::Cn => "https://api.trae.cn",
        Region::Ai => match auth.region.as_deref() {
            Some("US" | "us" | "US-East" | "us-east") => "https://grow-normal.traeapi.us",
            Some("SG" | "sg" | "Singapore") => "https://growsg-normal.trae.ai",
            Some("I18N" | "i18n" | "ROW" | "row") => "https://grow-normal.trae.ai",
            _ => return Err("无法确定 TRAE 国际版额度区域，请在客户端重新登录并保存".into()),
        },
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout.clamp(1, 3600)))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法创建 TRAE 额度请求")?;
    // 国内版菜单余额来自官方 v2。v1 会省略计费标志并只返回全 0 的旧字段。
    // req_source 1 对应桌面 IDE，与官方 main 进程一致。
    let (path, body) = match region {
        Region::Cn => (
            "/trae/api/v2/pay/ide_user_ent_usage",
            json!({"require_usage": true, "req_source": 1}),
        ),
        Region::Ai => (
            "/trae/api/v1/pay/ide_user_ent_usage",
            json!({"require_usage": true}),
        ),
    };
    let response = client
        .post(format!("{origin}{path}"))
        .header("Authorization", format!("Cloud-IDE-JWT {}", auth.token))
        .json(&body)
        .send()
        .await
        .map_err(|_| "TRAE 额度查询网络失败")?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err("TRAE 登录失效或无权查询额度，请在官方客户端重新登录并保存".into());
    }
    if !response.status().is_success() {
        return Err(format!(
            "TRAE 额度查询失败（HTTP {}）",
            response.status().as_u16()
        ));
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|_| "TRAE 额度响应格式不支持")?;
    summarize(&payload)
}

fn pack_label(pack: &Value) -> String {
    let name = pack
        .get("display_desc")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if (1..=40).contains(&name.chars().count())
        && !name.chars().any(|c| c.is_control() || c == '<' || c == '>')
    {
        name.to_string()
    } else {
        "积分包".to_string()
    }
}

fn future_end(pack: &Value) -> Option<i64> {
    pack.pointer("/entitlement_base_info/end_time")
        .or_else(|| pack.get("expire_time"))
        .and_then(Value::as_i64)
        .filter(|at| *at > chrono::Utc::now().timestamp())
}
pub(super) fn summarize(payload: &Value) -> Result<Value, String> {
    if payload
        .get("code")
        .and_then(Value::as_i64)
        .is_some_and(|c| c != 0)
    {
        return Err("TRAE 官方额度接口返回失败状态".into());
    }
    let data = payload.get("data").unwrap_or(payload);
    let billing = data.get("is_credits_billing").and_then(Value::as_bool);
    let dollar_billing = billing != Some(true) && data["is_dollar_usage_billing"] == true;
    let packs = data
        .get("user_entitlement_pack_list")
        .and_then(Value::as_array)
        .ok_or("TRAE 额度格式变化，未将未知余额当作零")?;
    let mut total = 0.0;
    let mut used = 0.0;
    let mut remaining = 0.0;
    let mut known = true;
    let mut unlimited = false;
    let mut count = 0;
    let mut expires_at: Option<i64> = None;
    let mut period_end: Option<i64> = None;
    let mut listed = Vec::new();
    for p in packs {
        if p.pointer("/entitlement_base_info/product_type")
            .and_then(Value::as_i64)
            == Some(3)
        {
            continue;
        }
        // 官方菜单把 credits_limit > 0 的包全部相加，status 0 的未消耗包也在其中。
        // 仍排除隐藏、上一周期、已过期和加速包，避免把不可用额度算进去。
        if p["is_hide"] == true
            || p["is_last_period"] == true
            || p.pointer("/entitlement_base_info/end_time")
                .and_then(Value::as_i64)
                .is_some_and(|v| v > 0 && v < chrono::Utc::now().timestamp())
        {
            continue;
        }
        // 积分包缺 credits_amount 时官方按 0 计算，不能让这一包把其余余额变成未知。
        // 没有 credits_limit 的包直接跳过。美元额度仍要求上下限字段齐全。
        if !dollar_billing {
            let cap = p
                .pointer("/entitlement_base_info/quota/credits_limit")
                .and_then(Value::as_f64);
            let spent = p.pointer("/usage/credits_amount").and_then(Value::as_f64);
            match cap {
                None => continue,
                Some(-1.0) => {
                    unlimited = true;
                    count += 1;
                }
                Some(v) if v >= 0.0 && spent.is_none_or(|s| s >= 0.0) => {
                    let spent = spent.unwrap_or(0.0);
                    count += 1;
                    total += v;
                    used += spent;
                    let piece = (v - spent).max(0.0);
                    remaining += piece;
                    if piece > 0.0 {
                        listed.push(json!({"name":pack_label(p),"remaining":piece,"total":v,"used":spent,"expiresAt":future_end(p)}));
                    }
                    if let Some(at) = p
                        .pointer("/entitlement_base_info/end_time")
                        .and_then(Value::as_i64)
                        .filter(|at| *at > chrono::Utc::now().timestamp())
                    {
                        period_end = Some(period_end.map_or(at, |old| old.min(at)));
                        if v > spent {
                            expires_at = Some(expires_at.map_or(at, |old| old.min(at)));
                        }
                    }
                }
                Some(_) => known = false,
            }
            continue;
        }
        count += 1;
        let pack_start = remaining;
        if let Some(at) = p
            .pointer("/entitlement_base_info/end_time")
            .and_then(Value::as_i64)
            .filter(|at| *at > chrono::Utc::now().timestamp())
        {
            period_end = Some(period_end.map_or(at, |old| old.min(at)));
        }
        let cap = p
            .pointer("/entitlement_base_info/quota/basic_usage_limit")
            .and_then(Value::as_f64);
        let spent = p
            .pointer("/usage/basic_usage_amount")
            .and_then(Value::as_f64);
        match cap {
            Some(-1.0) => unlimited = true,
            Some(v) if v >= 0.0 => total += v,
            _ => known = false,
        }
        match spent {
            Some(v) if v >= 0.0 => used += v,
            _ => known = false,
        }
        if let (Some(c), Some(s)) = (cap, spent) {
            if c >= 0.0 && s >= 0.0 {
                remaining += (c - s).max(0.0);
                if c > s {
                    if let Some(at) = p
                        .pointer("/entitlement_base_info/end_time")
                        .and_then(Value::as_i64)
                        .filter(|at| *at > chrono::Utc::now().timestamp())
                    {
                        expires_at = Some(expires_at.map_or(at, |old| old.min(at)));
                    }
                }
            }
        }
        if dollar_billing {
            if let Some(bonus) = p
                .pointer("/entitlement_base_info/quota/bonus_usage_limit")
                .and_then(Value::as_f64)
                .filter(|v| *v > 0.0)
            {
                total += bonus;
                if let Some(spent) = p
                    .pointer("/usage/bonus_usage_amount")
                    .and_then(Value::as_f64)
                    .filter(|v| *v >= 0.0)
                {
                    used += spent;
                    remaining += (bonus - spent).max(0.0);
                } else {
                    known = false;
                }
            }
        }
        let added = remaining - pack_start;
        if added > 0.0 {
            let basic_total = cap.filter(|v| *v > 0.0).unwrap_or(0.0);
            let bonus_total = p
                .pointer("/entitlement_base_info/quota/bonus_usage_limit")
                .and_then(Value::as_f64)
                .filter(|v| *v > 0.0)
                .unwrap_or(0.0);
            let row_total = basic_total + bonus_total;
            listed.push(json!({"name":pack_label(p),"remaining":added,"total":row_total,"used":(row_total-added).max(0.0),"expiresAt":future_end(p)}));
        }
    }
    // CN 3.4.15 omits is_credits_billing. Positive credit buckets plus explicit
    // non-dollar billing are evidence; zero-filled legacy credit fields aren't.
    let credit_evidence = billing == Some(true)
        || (billing.is_none()
            && data["is_dollar_usage_billing"] == false
            && (total > 0.0 || unlimited));
    known &= count > 0 && (credit_evidence || dollar_billing);
    Ok(
        json!({"source":"official-trae-entitlement-usage","checkedAt":chrono::Utc::now().timestamp(),"creditsBilling":billing,
        "total":if known&&!unlimited{Some(total)}else{None},"used":if known{Some(used)}else{None},
        "remaining":if known&&!unlimited{Some(remaining)}else{None},"unlimited":unlimited&&(credit_evidence||dollar_billing),"available":known,"unit":if dollar_billing{"usd"}else if credit_evidence{"credits"}else{"unknown"},
        "expiresAt":if known{expires_at}else{None},"periodEndAt":if known{period_end}else{None},"packs":if known{Value::Array(listed)}else{Value::Array(Vec::new())},"reason":if known{None}else{Some("unknown-or-legacy-billing-schema")}}),
    )
}
