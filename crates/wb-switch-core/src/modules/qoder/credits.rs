//! Per-account Qoder balance. Credentials are decoded in memory only.
//! Endpoint and quota fields are informed by NextAgentX/qoder-trae-workbuddy-switch;
//! errors never contain response bodies or credentials.
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

use super::{
    authorized,
    paths::{credentials, QoderRoots, QoderTarget},
    store,
};

struct Candidate {
    target: QoderTarget,
    token: String,
    uid: String,
}

fn quota_base(target: QoderTarget) -> &'static str {
    if target.is_global() {
        "https://openapi.qoder.sh"
    } else {
        "https://openapi.qoder.com.cn"
    }
}

fn other_client(target: QoderTarget) -> QoderTarget {
    match target {
        QoderTarget::Desktop => QoderTarget::Cli,
        QoderTarget::Cli => QoderTarget::Desktop,
        QoderTarget::GlobalDesktop => QoderTarget::GlobalCli,
        QoderTarget::GlobalCli => QoderTarget::GlobalDesktop,
    }
}

fn from_profile(target: QoderTarget, profile: &Value, expected: Option<&str>) -> Option<Candidate> {
    let identity = authorized::identity_from_json(target, profile)?;
    if expected.is_some_and(|value| value != identity) {
        return None;
    }
    let token = [
        "token",
        "security_oauth_token",
        "access_token",
        "accessToken",
    ]
    .iter()
    .find_map(|key| {
        profile
            .get(*key)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
    })?;
    let id = profile
        .get("user")
        .and_then(|v| v.get("id").or_else(|| v.get("user_id")))
        .or_else(|| profile.get("uid"))
        .or_else(|| profile.get("user_id"))?;
    let uid = id
        .as_str()
        .map(str::to_string)
        .or_else(|| id.as_i64().map(|id| id.to_string()))?;
    Some(Candidate {
        target,
        token: token.to_string(),
        uid,
    })
}

fn from_bundle(root: &Path, roots: &QoderRoots, meta: &store::BundleMeta) -> Option<Candidate> {
    let directory = store::bundle_dir(root, &meta.id).ok()?;
    #[cfg(windows)]
    if meta.files.len() == 1 && meta.files[0].name == super::vault::FILE {
        let bytes = std::fs::read(directory.join(super::vault::FILE)).ok()?;
        if store::sha256_hex(&bytes) != meta.files[0].sha256 {
            return None;
        }
        let (account, _) = super::vault::open(&bytes).ok()?;
        let id = account.user.get("id")?.as_str()?;
        if meta
            .identity_key
            .as_deref()
            .is_some_and(|key| key != authorized::identity_key(meta.target, id))
        {
            return None;
        }
        return (!account.access_token.is_empty()).then_some(Candidate {
            target: meta.target,
            token: account.access_token,
            uid: id.to_string(),
        });
    }
    let specs = credentials(roots, meta.target);
    let mut payload = Vec::new();
    let mut names = HashSet::new();
    for file in &meta.files {
        if !specs.iter().any(|spec| spec.name == file.name) || !names.insert(&file.name) {
            return None;
        }
        let bytes = std::fs::read(directory.join(&file.name)).ok()?;
        if store::sha256_hex(&bytes) != file.sha256 {
            return None;
        }
        payload.push((file.name.clone(), bytes));
    }
    #[cfg(not(windows))]
    let profile = if meta.target.is_desktop() {
        authorized::cached_desktop_profile(meta.target, &payload.first()?.1)?
    } else {
        authorized::snapshot_profile(&directory, meta.target, &payload)?
    };
    #[cfg(windows)]
    let profile = authorized::snapshot_profile(&directory, meta.target, &payload)?;
    from_profile(meta.target, &profile, meta.identity_key.as_deref())
}

fn from_live(roots: &QoderRoots, target: QoderTarget, expected: Option<&str>) -> Option<Candidate> {
    let specs = credentials(roots, target);
    let first = specs.first()?;
    let mut payload = vec![(first.name.to_string(), std::fs::read(&first.path).ok()?)];
    if let Some(machine) = specs.iter().find(|spec| spec.name == "machine_id") {
        if let Ok(bytes) = std::fs::read(&machine.path) {
            payload.push((machine.name.to_string(), bytes));
        }
    }
    #[cfg(not(windows))]
    let profile = if target.is_desktop() {
        authorized::cached_desktop_profile(target, &payload.first()?.1)?
    } else {
        authorized::snapshot_profile(&roots.desktop_dir_for(target), target, &payload)?
    };
    #[cfg(windows)]
    let profile = authorized::snapshot_profile(&roots.desktop_dir_for(target), target, &payload)?;
    from_profile(target, &profile, expected)
}

fn quota_body(payload: &Value) -> &Value {
    if payload.get("userQuota").is_some() || payload.get("addOnQuota").is_some() {
        payload
    } else {
        payload
            .get("data")
            .filter(|value| value.is_object())
            .unwrap_or(payload)
    }
}

fn quota(payload: &Value) -> Option<(f64, f64, f64)> {
    let body = quota_body(payload);
    let parts: Vec<&Value> = ["userQuota", "addOnQuota"]
        .iter()
        .filter_map(|key| body.get(*key).filter(|v| v.is_object()))
        .collect();
    let dedicated = dedicated_parts(payload)?;
    if parts.is_empty() && dedicated.is_empty() {
        return None;
    }
    let mut remaining = 0.0;
    let mut total = 0.0;
    let mut used = 0.0;
    for part in parts {
        if let Some(unit) = part.get("unit") {
            if !unit
                .as_str()
                .is_some_and(|value| value.eq_ignore_ascii_case("credits"))
            {
                return None;
            }
        }
        let number = |key| {
            part.get(key)
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite() && *n >= 0.0)
        };
        remaining += number("remaining")?;
        total += number("total")?;
        used += number("used")?;
    }
    for part in &dedicated {
        remaining += part.get("remaining")?.as_f64()?;
        total += part.get("total")?.as_f64()?;
        used += part.get("used")?.as_f64()?;
    }
    Some((remaining, total, used))
}

/// The upstream `expiresAt` is the plan expiry, not an add-on pack expiry.
fn plan_expiry(payload: &Value) -> Option<i64> {
    expiry_ms(quota_body(payload).get("expiresAt")?)
}

fn expiry_ms(value: &Value) -> Option<i64> {
    let raw = value
        .as_i64()
        .or_else(|| value.as_str()?.parse::<i64>().ok())?;
    let millis = if raw < 100_000_000_000 {
        raw.checked_mul(1000)?
    } else {
        raw
    };
    (978_307_200_000..4_102_444_800_000)
        .contains(&millis)
        .then_some(millis)
}

/// Dedicated packages are separate from plan and add-on buckets. Some Qoder
/// accounts expose a specific expiry per package in the normal quota response.
fn dedicated_parts(payload: &Value) -> Option<Vec<Value>> {
    let value = quota_body(payload).get("dedicatedResourcePackages");
    let Some(value) = value else {
        return Some(Vec::new());
    };
    if value.is_null() {
        return Some(Vec::new());
    }
    let mut parts = Vec::new();
    for part in value.as_array()? {
        if part.get("available").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let status = part
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_uppercase();
        if [
            "EXPIRED", "INVALID", "INACTIVE", "DISABLED", "USED_UP", "DEPLETED",
        ]
        .iter()
        .any(|s| status.contains(s))
        {
            continue;
        }
        if part
            .get("unit")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.eq_ignore_ascii_case("credits"))
        {
            return None;
        }
        let number = |key| {
            part.get(key)
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite() && *n >= 0.0)
        };
        parts.push(json!({"remaining":number("remaining")?,"total":number("total")?,"used":number("used")?,"expiresAt":part.get("expiresAt").and_then(expiry_ms)}));
    }
    Some(parts)
}

fn quota_part(payload: &Value, key: &str) -> Option<Value> {
    let part = quota_body(payload).get(key)?;
    let number = |name| {
        part.get(name)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v >= 0.0)
    };
    Some(
        json!({"remaining":number("remaining")?, "total":number("total")?, "used":number("used")?}),
    )
}

fn candidates_for(
    root: &Path,
    roots: &QoderRoots,
    ids: &[String],
    live_target: Option<QoderTarget>,
) -> Result<Vec<Candidate>, &'static str> {
    if ids.len() > 64 {
        return Err("账号数据过多");
    }
    let mut metas: Vec<_> = ids
        .iter()
        .filter_map(|id| store::load(root, id).ok())
        .collect();
    metas.sort_by_key(|m| std::cmp::Reverse(m.captured_at));
    let expected = metas.first().and_then(|m| m.identity_key.as_deref());
    if expected.is_none() && metas.len() > 1 {
        return Err("无法确认账号身份");
    }
    if metas.iter().any(|m| {
        m.identity_key.as_deref() != expected || m.target.is_global() != metas[0].target.is_global()
    }) {
        return Err("账号身份不一致");
    }
    if let (Some(target), Some(meta)) = (live_target, metas.first()) {
        if target.is_global() != meta.target.is_global() {
            return Err("账号区域不一致");
        }
    }
    let mut candidates = Vec::new();
    if let Some(target) = live_target {
        let live_matches = expected.is_some()
            || metas.is_empty()
            || metas.first().is_some_and(|meta| {
                store::current_id(root, roots, target).as_deref() == Some(meta.id.as_str())
            });
        if live_matches {
            if let Some(value) = from_live(roots, target, expected) {
                candidates.push(value);
            }
        }
        // When both clients use this account, either can hold the newest token.
        // Never try the other client's credential without a verified identity.
        if expected.is_some() {
            let other = other_client(target);
            if !other.is_desktop() || cfg!(windows) {
                if let Some(value) = from_live(roots, other, expected) {
                    candidates.push(value);
                }
            }
        }
    }
    for meta in &metas {
        if let Some(value) = from_bundle(root, roots, meta) {
            candidates.push(value);
        }
    }
    if candidates.is_empty() {
        return Err("无法读取账号凭据，请授权识别或重新登录");
    }
    Ok(candidates)
}

pub async fn query(ids: &[String], live_target: Option<QoderTarget>) -> Value {
    query_with(
        &store::default_store(),
        &QoderRoots::real(),
        ids,
        live_target,
        None,
    )
    .await
}

pub(super) async fn query_with(
    root: &Path,
    roots: &QoderRoots,
    ids: &[String],
    live_target: Option<QoderTarget>,
    endpoint: Option<&str>,
) -> Value {
    let candidates = match candidates_for(root, roots, ids, live_target) {
        Ok(value) => value,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(12));
    if endpoint.is_some() {
        builder = builder.no_proxy();
    }
    let client = match builder.build() {
        Ok(value) => value,
        Err(_) => return json!({"ok":false,"error":"无法初始化积分查询"}),
    };
    let mut seen = HashSet::new();
    let mut expired = false;
    let mut unavailable = false;
    let mut invalid_response = false;
    for candidate in candidates {
        if seen.len() >= 6 {
            break;
        }
        if !seen.insert(store::sha256_hex(candidate.token.as_bytes())) {
            continue;
        }
        let base = endpoint.unwrap_or(quota_base(candidate.target));
        for attempt in 0..2 {
            let response = client
                .get(format!("{base}/api/v2/quota/usage"))
                .header("Accept", "application/json")
                .bearer_auth(&candidate.token)
                .send()
                .await;
            let retry = match response {
                Err(_) => true,
                Ok(response) if matches!(response.status().as_u16(), 401 | 403) => {
                    expired = true;
                    break;
                }
                Ok(response)
                    if response.status().as_u16() == 429 || response.status().is_server_error() =>
                {
                    true
                }
                Ok(response) if !response.status().is_success() => {
                    unavailable = true;
                    break;
                }
                Ok(response) => {
                    if let Ok(data) = response.json::<Value>().await {
                        if let Some((remaining, total, used)) = quota(&data) {
                            let mut result = json!({"ok":true,"remaining":remaining,"total":total,"used":used,
                                "planExpiresAt":plan_expiry(&data), "plan":quota_part(&data,"userQuota"),
                                "addOn":quota_part(&data,"addOnQuota"), "dedicated":dedicated_parts(&data)});
                            if super::credit_stats::record(
                                root,
                                &authorized::identity_key(candidate.target, &candidate.uid),
                                candidate.target.is_global(),
                                ids,
                                &result,
                            )
                            .is_err()
                            {
                                result["statisticsWarning"] =
                                    json!("积分已查询，但本地统计快照保存失败");
                            }
                            return result;
                        }
                    }
                    invalid_response = true;
                    break;
                }
            };
            if retry {
                unavailable = true;
                if attempt == 0 {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
            }
        }
    }
    json!({"ok":false,"error":if unavailable {"积分服务暂时不可用，请检查网络后刷新"} else if invalid_response {"积分服务返回的数据无法识别，请稍后刷新"} else if expired {"登录状态可能已失效，请重新登录"} else {"积分查询失败，请稍后重试"}})
}

/// Run the upstream campaign and Pro benefit workflow with the same
/// identity-checked credential candidates as the balance query.
pub async fn claim(ids: &[String], live_target: Option<QoderTarget>) -> Value {
    let candidates = match candidates_for(
        &store::default_store(),
        &QoderRoots::real(),
        ids,
        live_target,
    ) {
        Ok(candidates) => candidates,
        Err(error) => return json!({"ok":false,"message":error}),
    };
    let mut seen = HashSet::new();
    for candidate in candidates {
        if !seen.insert(store::sha256_hex(candidate.token.as_bytes())) {
            continue;
        }
        let mode = if candidate.target.is_global() {
            "qoder-global"
        } else {
            "qoder-cn"
        };
        let result =
            crate::modules::daily_runner::run(mode, candidate.token, candidate.uid, false).await;
        record_claim(&result, candidate.target, ids);
        if result.get("authInvalid").and_then(Value::as_bool) != Some(true) {
            return result;
        }
    }
    json!({"ok":false,"message":"登录状态可能已失效，请重新登录"})
}

/// 把 Qoder 领奖结果写进领取记忆。`ids` 为空表示「本机当前登录、尚未加入账号库」，
/// 用 `live:<target>` 作为键，页面按同一约定聚合。
fn record_claim(result: &Value, target: QoderTarget, ids: &[String]) {
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            if result.get("ok").and_then(Value::as_bool) == Some(true) {
                crate::modules::claim_ledger::STATE_CLAIMED
            } else {
                crate::modules::claim_ledger::STATE_ERROR
            }
        });
    let message = result.get("message").and_then(Value::as_str).unwrap_or("");
    let variant = if target.is_global() { "ai" } else { "cn" };
    let keys: Vec<String> = if ids.is_empty() {
        let raw = serde_json::to_value(target)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        vec![format!("live:{raw}")]
    } else {
        ids.to_vec()
    };
    for key in keys {
        crate::modules::claim_ledger::record(
            crate::modules::claim_ledger::PLATFORM_QODER,
            Some(variant),
            &key,
            crate::modules::claim_ledger::SCOPE_REWARDS,
            crate::modules::claim_ledger::SOURCE_MANUAL,
            crate::modules::claim_ledger::Outcome::single(state, message),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "requires locally authorized Qoder accounts and network"]
    async fn real_expiry_field_probe_without_secrets() {
        let root = store::default_store();
        let roots = QoderRoots::real();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap();
        let mut identities = HashSet::new();
        for meta in store::list(&root, None) {
            let Some(identity) = &meta.identity_key else {
                continue;
            };
            if !identities.insert(identity.clone()) {
                continue;
            }
            let Some(candidate) = from_bundle(&root, &roots, &meta) else {
                continue;
            };
            let reply = client
                .get(format!("{}/api/v2/quota/usage", quota_base(meta.target)))
                .bearer_auth(&candidate.token)
                .send()
                .await;
            let Ok(reply) = reply else {
                println!("quota=network-error");
                continue;
            };
            let status = reply.status().as_u16();
            let payload = reply.json::<Value>().await.unwrap_or(Value::Null);
            let root = payload
                .get("data")
                .filter(|v| v.is_object())
                .unwrap_or(&payload);
            let describe = |value: Option<&Value>| -> &'static str {
                match value.and_then(Value::as_i64) {
                    None => "missing-or-nonnumeric",
                    Some(n) if n <= 0 || n >= 4_102_444_800_000 => "sentinel-or-invalid",
                    Some(_) => "plausible",
                }
            };
            let plan = client
                .get(format!("{}/api/v2/user/plan", quota_base(meta.target)))
                .bearer_auth(&candidate.token)
                .send()
                .await;
            let (plan_status, plan_has_expiry) = match plan {
                Ok(response) => {
                    let code = response.status().as_u16();
                    let body = response.json::<Value>().await.unwrap_or(Value::Null);
                    let found = body.as_object().is_some_and(|map| {
                        map.keys().any(|key| {
                            let key = key.to_ascii_lowercase();
                            key.contains("expire") || key.contains("end_time")
                        })
                    });
                    (code, found)
                }
                Err(_) => (0, false),
            };
            let website = if meta.target.is_global() {
                "https://qoder.com"
            } else {
                "https://qoder.com.cn"
            };
            let web_status = client
                .get(format!("{website}/api/v2/me/usages/big_model_credits"))
                .bearer_auth(&candidate.token)
                .send()
                .await
                .map(|r| r.status().as_u16())
                .unwrap_or(0);
            println!("quota={status} plan-expiry={} user-quota-expiry={} addon-expiry={} plan-api={plan_status} plan-has-expiry={plan_has_expiry} web-bearer={web_status}",
                describe(root.get("expiresAt")),
                describe(root.get("userQuota").and_then(|v| v.get("expiresAt"))),
                describe(root.get("addOnQuota").and_then(|v| v.get("expiresAt"))));
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "requires locally authorized Qoder accounts and network"]
    async fn real_saved_account_balances_are_readable() {
        let root = store::default_store();
        let entries = store::list(&root, None);
        let mut identities = HashSet::new();
        for item in &entries {
            if let Some(identity) = item.identity_key.as_ref() {
                identities.insert(identity.clone());
            }
        }
        assert!(!identities.is_empty());
        let mut successes = 0;
        let mut failures = 0;
        let mut with_plan_expiry = 0;
        for identity in identities {
            let ids: Vec<String> = entries
                .iter()
                .filter(|item| item.identity_key.as_deref() == Some(identity.as_str()))
                .map(|item| item.id.clone())
                .collect();
            let result = query(&ids, None).await;
            if result["ok"] == true {
                successes += 1;
                if result["planExpiresAt"].is_i64() {
                    with_plan_expiry += 1;
                }
            } else {
                failures += 1;
            }
        }
        println!("Real credit probe: successes={successes}, failures={failures}, plan_expiry={with_plan_expiry}");
        assert!(successes > 0, "no real account balance was readable");
    }

    #[test]
    fn aggregates_fractional_credits_and_rejects_incomplete_responses() {
        let data = json!({"userQuota":{"remaining":12.5,"total":20.0,"used":7.5},"addOnQuota":{"remaining":3,"total":5,"used":2}});
        assert_eq!(quota(&data), Some((15.5, 25.0, 9.5)));
        assert_eq!(quota(&json!({"data":data})), Some((15.5, 25.0, 9.5)));
        assert!(quota(&json!({"userQuota":{"remaining":4}})).is_none());
        assert!(
            quota(&json!({"userQuota":{"remaining":4,"total":5,"used":1,"unit":"requests"}}))
                .is_none()
        );
        assert_eq!(
            quota(&json!({"isQuotaExceeded":true,"userQuota":{"remaining":0,"used":0,"total":0}})),
            Some((0.0, 0.0, 0.0))
        );
        assert_eq!(
            quota(&json!({"userQuota":{"remaining":0,"used":3,"total":3}})),
            Some((0.0, 3.0, 3.0))
        );
    }

    #[test]
    fn plan_expiry_is_not_fabricated_from_add_on_quota_or_sentinels() {
        assert_eq!(
            plan_expiry(&json!({"expiresAt":1795595404000_i64})),
            Some(1795595404000)
        );
        assert_eq!(
            plan_expiry(&json!({"data":{"expiresAt":1795595404_i64}})),
            Some(1795595404000)
        );
        assert_eq!(plan_expiry(&json!({"expiresAt":253402214400000_i64})), None);
        assert_eq!(
            plan_expiry(&json!({"addOnQuota":{"expiresAt":1795595404000_i64}})),
            None
        );
        assert_eq!(plan_expiry(&json!({"expiresAt":0})), None);
    }

    #[test]
    fn dedicated_credits_are_counted_and_keep_their_own_expiry() {
        let data = json!({"userQuota":{"remaining":10,"total":20,"used":10},
        "dedicatedResourcePackages":[
            {"remaining":3.5,"total":5,"used":1.5,"expiresAt":1795595404000_i64},
            {"remaining":99,"total":100,"used":1,"available":false},
            {"remaining":99,"total":100,"used":1,"status":"QUOTA_DETAIL_STATUS_EXPIRED"}
        ]});
        assert_eq!(quota(&data), Some((13.5, 25.0, 11.5)));
        let parts = dedicated_parts(&data).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["expiresAt"], 1795595404000_i64);
        assert!(quota(&json!({"dedicatedResourcePackages":[{"remaining":1}]})).is_none());
    }

    #[test]
    fn refuses_to_use_a_different_account_or_missing_identity() {
        let target = QoderTarget::Cli;
        let expected = authorized::identity_key(target, "one");
        assert!(from_profile(
            target,
            &json!({"uid":"other","access_token":"test"}),
            Some(&expected)
        )
        .is_none());
        assert!(from_profile(target, &json!({"access_token":"test"}), Some(&expected)).is_none());
        assert!(from_profile(
            target,
            &json!({"uid":"one","access_token":"test"}),
            Some(&expected)
        )
        .is_some());
    }

    #[test]
    fn routes_domestic_and_global_accounts_to_their_own_services() {
        assert_eq!(
            quota_base(QoderTarget::Desktop),
            quota_base(QoderTarget::Cli)
        );
        assert_eq!(
            quota_base(QoderTarget::GlobalDesktop),
            quota_base(QoderTarget::GlobalCli)
        );
        assert_ne!(
            quota_base(QoderTarget::Desktop),
            quota_base(QoderTarget::GlobalDesktop)
        );
    }

    #[tokio::test]
    async fn falls_back_to_same_account_snapshot_and_retries_temporary_failure() {
        use std::io::{Read, Write};
        let temp = std::env::temp_dir().join(format!("wb-qoder-credit-{}", uuid::Uuid::new_v4()));
        let roots = QoderRoots {
            home: temp.join("home"),
            app_data: temp.join("app"),
        };
        let store_root = temp.join("store");
        let auth = roots.cli_dir_for(QoderTarget::Cli).join(".auth");
        std::fs::create_dir_all(&auth).unwrap();
        std::fs::write(
            auth.join("user"),
            br#"{"uid":"one","access_token":"expired-test"}"#,
        )
        .unwrap();
        let identity = authorized::identity_key(QoderTarget::Cli, "one");
        let saved = store::save_authorized(
            &store_root,
            QoderTarget::Cli,
            "test",
            &identity,
            vec![(
                "user".into(),
                br#"{"uid":"one","access_token":"valid-test"}"#.to_vec(),
            )],
        )
        .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for index in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut bytes = [0u8; 8192];
                let count = stream.read(&mut bytes).unwrap();
                let request = String::from_utf8_lossy(&bytes[..count]).to_ascii_lowercase();
                assert!(request.contains(if index == 0 {
                    "bearer expired-test"
                } else {
                    "bearer valid-test"
                }));
                let (status, body) = if index == 0 {
                    ("401 Unauthorized", "{}")
                } else if index == 1 {
                    ("503 Service Unavailable", "{}")
                } else {
                    (
                        "200 OK",
                        r#"{"expiresAt":1795595404000,"userQuota":{"remaining":12.5,"total":20,"used":7.5},"addOnQuota":{"remaining":3,"total":5,"used":2}}"#,
                    )
                };
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let result = query_with(
            &store_root,
            &roots,
            &[saved.id],
            Some(QoderTarget::Cli),
            Some(&endpoint),
        )
        .await;
        server.join().unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(result["remaining"], 15.5);
        assert_eq!(result["planExpiresAt"], 1795595404000_i64);
        assert_eq!(result["plan"]["remaining"], 12.5);
        assert_eq!(result["addOn"]["remaining"], 3.0);
        assert!(!result.to_string().contains("valid-test"));
        std::fs::remove_dir_all(temp).unwrap();
    }
}
