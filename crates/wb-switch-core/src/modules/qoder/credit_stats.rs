//! Account quota snapshots are separate from local request credits. Never add
//! these two measurements together. No credentials or response bodies persist.
use super::{credits, store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, sync::Mutex};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
static REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const RETENTION: i64 = 90 * 86_400_000;
const MAX_SNAPSHOTS: usize = 5_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    ts: i64,
    account_key: String,
    global: bool,
    bundle_ids: Vec<String>,
    remaining: f64,
    total: f64,
    used: f64,
    plan_expires_at: Option<i64>,
    buckets: Vec<Bucket>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Bucket {
    key: String,
    total: f64,
    used: f64,
    expires: Option<i64>,
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value
        .get(key)?
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
}

fn read(root: &Path) -> Result<Vec<Snapshot>, String> {
    match std::fs::read(root.join("credit-snapshots.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "Qoder 积分快照无法读取".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err("Qoder 积分快照无法读取".into()),
    }
}

pub(super) fn record(
    root: &Path,
    key: &str,
    global: bool,
    ids: &[String],
    value: &Value,
) -> Result<(), String> {
    record_at(
        root,
        key,
        global,
        ids,
        value,
        crate::modules::config::now_ms(),
    )
}

fn record_at(
    root: &Path,
    key: &str,
    global: bool,
    ids: &[String],
    value: &Value,
    now: i64,
) -> Result<(), String> {
    let valid = || -> Option<Snapshot> {
        let mut buckets = Vec::new();
        for name in ["plan", "addOn"] {
            if let Some(part) = value.get(name).filter(|p| p.is_object()) {
                buckets.push(Bucket {
                    key: name.into(),
                    total: number(part, "total")?,
                    used: number(part, "used")?,
                    expires: if name == "plan" {
                        value["planExpiresAt"].as_i64()
                    } else {
                        None
                    },
                });
            }
        }
        // Dedicated packages have no stable IDs in the quota adapter. Compare
        // only their aggregate within an unchanged expiry/total composition.
        if let Some(parts) = value["dedicated"].as_array() {
            let mut signatures = Vec::new();
            let mut total = 0.0;
            let mut used = 0.0;
            for part in parts {
                let t = number(part, "total")?;
                total += t;
                used += number(part, "used")?;
                signatures.push(format!("{}:{t}", part["expiresAt"]));
            }
            signatures.sort();
            if !parts.is_empty() {
                buckets.push(Bucket {
                    key: format!("dedicated:{}", signatures.join("|")),
                    total,
                    used,
                    expires: None,
                });
            }
        }
        Some(Snapshot {
            ts: now,
            account_key: key.into(),
            global,
            bundle_ids: ids.to_vec(),
            remaining: number(value, "remaining")?,
            total: number(value, "total")?,
            used: number(value, "used")?,
            plan_expires_at: value["planExpiresAt"].as_i64(),
            buckets,
        })
    };
    let snapshot = valid().ok_or("无效的积分快照")?;
    let _guard = WRITE_LOCK.lock().map_err(|_| "积分快照暂时不可用")?;
    let mut rows = read(root)?;
    rows.retain(|s| s.ts >= now - RETENTION && s.ts <= now);
    // Identical rapid refreshes do not flood history or move an earlier consumption event.
    if let Some(last) = rows
        .iter()
        .rev()
        .find(|s| s.account_key == key && s.global == global)
    {
        if now - last.ts < 60_000
            && last.used == snapshot.used
            && last.total == snapshot.total
            && last.remaining == snapshot.remaining
            && last.plan_expires_at == snapshot.plan_expires_at
        {
            return Ok(());
        }
    }
    rows.push(snapshot);
    rows.sort_by_key(|r| r.ts);
    if rows.len() > MAX_SNAPSHOTS {
        rows.drain(..rows.len() - MAX_SNAPSHOTS);
    }
    std::fs::create_dir_all(root).map_err(|_| "无法保存积分快照")?;
    crate::modules::config::atomic_write(
        &root.join("credit-snapshots.json"),
        &serde_json::to_string(&rows).map_err(|_| "无法编码积分快照")?,
    )
    .map_err(|_| "无法保存积分快照".into())
}

fn observed(previous: &Snapshot, next: &Snapshot) -> Option<f64> {
    if previous.global != next.global
        || previous.account_key != next.account_key
        || next.ts <= previous.ts
    {
        return None;
    }
    let mut total = 0.0;
    for bucket in &next.buckets {
        let Some(old) = previous.buckets.iter().find(|old| old.key == bucket.key) else {
            continue;
        };
        if old.total == bucket.total
            && old.expires == bucket.expires
            && bucket.used >= old.used
            && old.expires.is_none_or(|expiry| expiry > next.ts)
        {
            total += bucket.used - old.used;
        }
    }
    Some(total)
}

fn project(rows: &[Snapshot], bundles: &[store::BundleMeta], now: i64) -> Value {
    let mut latest: BTreeMap<(bool, String), &Snapshot> = BTreeMap::new();
    let mut daily: BTreeMap<(bool, String, String), f64> = BTreeMap::new();
    for row in rows
        .iter()
        .filter(|s| s.ts >= now - RETENTION && s.ts <= now)
    {
        let key = (row.global, row.account_key.clone());
        if let Some(previous) = latest.get(&key) {
            if let Some(amount) = observed(previous, row).filter(|v| *v > 0.0) {
                if let Some(date) = chrono::DateTime::from_timestamp_millis(row.ts) {
                    *daily
                        .entry((
                            row.global,
                            row.account_key.clone(),
                            date.with_timezone(&chrono::Local)
                                .format("%Y-%m-%d")
                                .to_string(),
                        ))
                        .or_default() += amount;
                }
            }
        }
        latest.insert(key, row);
    }
    let accounts: Vec<Value>=latest.into_values().filter_map(|s| {
        let meta=bundles.iter().filter(|m| m.target.is_global()==s.global && (m.identity_key.as_deref()==Some(&s.account_key) || s.bundle_ids.contains(&m.id))).max_by_key(|m|m.captured_at)?;
        Some(json!({"accountKey":s.account_key,"name":meta.label,"global":s.global,"updatedAt":s.ts,"remaining":s.remaining,"total":s.total,"used":s.used,"planExpiresAt":s.plan_expires_at}))
    }).collect();
    let daily: Vec<_>=daily.into_iter().filter(|((global,key,_),_)|accounts.iter().any(|a| a["accountKey"]==*key && a["global"]==*global)).map(|((global,key,date),amount)|json!({"global":global,"accountKey":key,"date":date,"credits":amount})).collect();
    json!({"generatedAt":now,"accounts":accounts,"daily":daily,"retentionDays":90})
}

pub async fn get_statistics(refresh: bool) -> Value {
    let root = store::default_store();
    let bundles = store::list(&root, None);
    let mut errors = Vec::new();
    if refresh {
        let _guard = REFRESH_LOCK.lock().await;
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for meta in &bundles {
            groups
                .entry(meta.identity_key.clone().unwrap_or_else(|| meta.id.clone()))
                .or_default()
                .push(meta.id.clone());
        }
        let groups: Vec<_> = groups.into_iter().collect();
        for chunk in groups.chunks(4) {
            let mut tasks = tokio::task::JoinSet::new();
            for (key, ids) in chunk {
                let key = key.clone();
                let ids = ids.clone();
                tasks.spawn(async move { (key, credits::query(&ids, None).await) });
            }
            while let Some(result) = tasks.join_next().await {
                match result {
                    Ok((key, value)) if value["ok"] != true => {
                        errors.push(json!({"accountKey":key,"message":value["error"]}))
                    }
                    Ok((key, value)) if value.get("statisticsWarning").is_some() => {
                        errors.push(json!({"accountKey":key,"message":value["statisticsWarning"]}))
                    }
                    Err(_) => errors.push(json!({"message":"部分账号积分刷新失败"})),
                    _ => {}
                }
            }
        }
    }
    let rows = match read(&root) {
        Ok(rows) => rows,
        Err(message) => {
            errors.push(json!({"message":message}));
            Vec::new()
        }
    };
    let mut result = project(&rows, &bundles, crate::modules::config::now_ms());
    result["errors"] = json!(errors);
    result["savedAccountCount"] = json!(bundles
        .iter()
        .map(|m| m.identity_key.as_deref().unwrap_or(&m.id))
        .collect::<std::collections::HashSet<_>>()
        .len());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(total: f64, used: f64, expires: Option<i64>) -> Snapshot {
        Snapshot {
            ts: 1,
            account_key: "a".into(),
            global: false,
            bundle_ids: vec![],
            remaining: total - used,
            total,
            used,
            plan_expires_at: expires,
            buckets: vec![Bucket {
                key: "plan".into(),
                total,
                used,
                expires,
            }],
        }
    }
    #[test]
    fn replenishment_reset_expiry_and_region_do_not_become_consumption() {
        let a = sample(100.0, 10.0, Some(100));
        let mut b = sample(100.0, 20.0, Some(100));
        b.ts = 2;
        assert_eq!(observed(&a, &b), Some(10.0));
        b.buckets[0].total = 200.0;
        assert_eq!(observed(&a, &b), Some(0.0));
        b.buckets[0].total = 100.0;
        b.buckets[0].used = 0.0;
        assert_eq!(observed(&a, &b), Some(0.0));
        b.buckets[0].used = 100.0;
        b.ts = 101;
        assert_eq!(observed(&a, &b), Some(0.0));
        b.global = true;
        assert_eq!(observed(&a, &b), None);
    }
    #[test]
    fn snapshot_roundtrip_keeps_only_allowed_quota_fields() {
        let root =
            std::env::temp_dir().join(format!("qoder-credit-stats-{}", uuid::Uuid::new_v4()));
        let value = json!({"total":100,"remaining":90,"used":10,"plan":{"total":100,"used":10},"accessToken":"SECRET","responseBody":"PRIVATE"});
        record_at(&root, "account", false, &[], &value, 1).unwrap();
        assert_eq!(read(&root).unwrap().len(), 1);
        let file = std::fs::read_to_string(root.join("credit-snapshots.json")).unwrap();
        assert!(!file.contains("SECRET") && !file.contains("PRIVATE"));
        record_at(&root, "account", false, &[], &value, 2).unwrap();
        assert_eq!(
            read(&root).unwrap().len(),
            1,
            "identical rapid queries are coalesced"
        );
        record_at(&root, "account", false, &[], &value, RETENTION + 10).unwrap();
        assert_eq!(read(&root).unwrap().len(), 1, "expired history is pruned");
        std::fs::write(root.join("credit-snapshots.json"), "corrupt").unwrap();
        assert!(record_at(&root, "account", false, &[], &value, RETENTION + 20).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("credit-snapshots.json")).unwrap(),
            "corrupt"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn same_account_clients_merge_and_removed_accounts_do_not_leak_into_totals() {
        let mut a = sample(100.0, 10.0, None);
        let mut b = sample(100.0, 15.0, None);
        b.ts = 2;
        a.account_key = "identity".into();
        b.account_key = "identity".into();
        let meta = |id: &str, target| store::BundleMeta {
            id: id.into(),
            target,
            label: id.into(),
            echo_name: None,
            identity_key: Some("identity".into()),
            captured_at: 1,
            last_switched_at: None,
            files: vec![],
        };
        let bundles = vec![
            meta("desktop", super::super::QoderTarget::Desktop),
            meta("cli", super::super::QoderTarget::Cli),
        ];
        let rows = vec![a, b];
        let result = project(&rows, &bundles, 3);
        assert_eq!(result["accounts"].as_array().unwrap().len(), 1);
        assert_eq!(result["daily"][0]["credits"], 5.0);
        let removed = project(&rows, &[], 3);
        assert!(removed["accounts"].as_array().unwrap().is_empty());
        assert!(removed["daily"].as_array().unwrap().is_empty());
    }
}
