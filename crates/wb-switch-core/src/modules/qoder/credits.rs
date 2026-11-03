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
    Some(Candidate {
        target,
        token: token.to_string(),
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

fn quota(payload: &Value) -> Option<(f64, f64, f64)> {
    let body = if payload.get("userQuota").is_some() || payload.get("addOnQuota").is_some() {
        payload
    } else {
        payload
            .get("data")
            .filter(|value| value.is_object())
            .unwrap_or(payload)
    };
    let parts: Vec<&Value> = ["userQuota", "addOnQuota"]
        .iter()
        .filter_map(|key| body.get(*key).filter(|v| v.is_object()))
        .collect();
    if parts.is_empty() {
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
    Some((remaining, total, used))
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

async fn query_with(
    root: &Path,
    roots: &QoderRoots,
    ids: &[String],
    live_target: Option<QoderTarget>,
    endpoint: Option<&str>,
) -> Value {
    if ids.len() > 64 {
        return json!({"ok":false,"error":"账号数据过多"});
    }
    let mut metas: Vec<_> = ids
        .iter()
        .filter_map(|id| store::load(root, id).ok())
        .collect();
    metas.sort_by_key(|m| std::cmp::Reverse(m.captured_at));
    let expected = metas.first().and_then(|m| m.identity_key.as_deref());
    if expected.is_none() && metas.len() > 1 {
        return json!({"ok":false,"error":"无法确认账号身份"});
    }
    if metas.iter().any(|m| {
        m.identity_key.as_deref() != expected || m.target.is_global() != metas[0].target.is_global()
    }) {
        return json!({"ok":false,"error":"账号身份不一致"});
    }
    if let (Some(target), Some(meta)) = (live_target, metas.first()) {
        if target.is_global() != meta.target.is_global() {
            return json!({"ok":false,"error":"账号区域不一致"});
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
        return json!({"ok":false,"error":"无法读取账号凭据，请授权识别或重新登录"});
    }
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
    for candidate in candidates {
        if seen.len() >= 6 {
            break;
        }
        if !seen.insert(store::sha256_hex(candidate.token.as_bytes())) {
            continue;
        }
        let base = endpoint.unwrap_or(quota_base(candidate.target));
        let response = client
            .get(format!("{base}/api/v2/quota/usage"))
            .header("Accept", "application/json")
            .bearer_auth(&candidate.token)
            .send()
            .await;
        let Ok(response) = response else {
            continue;
        };
        if matches!(response.status().as_u16(), 401 | 403) {
            expired = true;
            continue;
        }
        if !response.status().is_success() {
            continue;
        }
        if let Ok(data) = response.json::<Value>().await {
            if let Some((remaining, total, used)) = quota(&data) {
                return json!({"ok":true,"remaining":remaining,"total":total,"used":used});
            }
        }
    }
    json!({"ok":false,"error":if expired {"登录状态可能已失效，请重新登录"} else {"积分查询失败，请稍后重试"}})
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for identity in identities {
            let ids: Vec<String> = entries
                .iter()
                .filter(|item| item.identity_key.as_deref() == Some(identity.as_str()))
                .map(|item| item.id.clone())
                .collect();
            let result = query(&ids, None).await;
            if result["ok"] == true {
                successes += 1;
            } else {
                failures += 1;
            }
        }
        println!("Real credit probe: successes={successes}, failures={failures}");
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
    async fn falls_back_to_same_account_snapshot_when_live_token_is_rejected() {
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
            for index in 0..2 {
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
                } else {
                    (
                        "200 OK",
                        r#"{"userQuota":{"remaining":12.5,"total":20,"used":7.5}}"#,
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
        assert_eq!(result["remaining"], 12.5);
        assert!(!result.to_string().contains("valid-test"));
        std::fs::remove_dir_all(temp).unwrap();
    }
}
