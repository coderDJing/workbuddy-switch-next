//! Official quota snapshots only. No Token estimates or inferred request counts.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Snapshot {
    account_id: String,
    name: String,
    region: Region,
    at: i64,
    credit: Value,
}
pub(super) fn record(store: &mut Store, account: &Account, credit: &Value) {
    let at = credit["checkedAt"]
        .as_i64()
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    store
        .snapshots
        .retain(|s| s.at >= at - 90 * 86400 && !(s.account_id == account.id && s.at == at));
    store.snapshots.push(Snapshot {
        account_id: account.id.clone(),
        name: account.name.clone(),
        region: account.region,
        at,
        credit: credit.clone(),
    });
    store.snapshots.sort_by_key(|s| s.at);
    if store.snapshots.len() > 5000 {
        store.snapshots.drain(..store.snapshots.len() - 5000);
    }
}
impl Engine {
    pub(super) async fn statistics(&self, region: Region, refresh: bool) -> Result<Value, String> {
        let accounts = {
            let _guard = self.lock()?;
            self.load()?
                .accounts
                .into_iter()
                .filter(|a| a.region == region)
                .collect::<Vec<_>>()
        };
        let mut errors = Vec::new();
        if refresh {
            for a in &accounts {
                if let Err(error) = self.credit(&a.id, None).await {
                    errors.push(json!({"accountId":a.id,"message":error}));
                }
            }
        }
        let _guard = self.lock()?;
        let store = self.load()?;
        Ok(
            json!({"generatedAt":chrono::Utc::now().timestamp(),"retentionDays":90,"region":region,
            "accounts":store.accounts.iter().filter(|a|a.region==region).map(|a| a.meta(false)).collect::<Vec<_>>(),
            "snapshots":store.snapshots.iter().filter(|s|s.region==region&&s.at>=chrono::Utc::now().timestamp()-90*86400).collect::<Vec<_>>(),"errors":errors}),
        )
    }
}
