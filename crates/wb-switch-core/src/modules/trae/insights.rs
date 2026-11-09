//! Official quota snapshots only. No Token estimates or inferred request counts.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Snapshot {
    account_id: String,
    name: String,
    variant: Region,
    at: i64,
    credit: Value,
}
pub(super) fn record(store: &mut Store, account: &Account, credit: &Value) {
    let at = credit["checkedAt"]
        .as_i64()
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    store
        .snapshots
        .retain(|s| s.at >= at - 365 * 86400 && !(s.account_id == account.id && s.at == at));
    store.snapshots.push(Snapshot {
        account_id: account.id.clone(),
        name: account.name.clone(),
        variant: account.variant,
        at,
        credit: credit.clone(),
    });
    store.snapshots.sort_by_key(|s| s.at);
    if store.snapshots.len() > 20_000 {
        store.snapshots.drain(..store.snapshots.len() - 20_000);
    }
}
impl Engine {
    pub(super) async fn statistics(&self, region: Region, refresh: bool) -> Result<Value, String> {
        let accounts = {
            let _guard = self.lock()?;
            self.load()?
                .accounts
                .into_iter()
                .filter(|a| a.variant == region)
                .collect::<Vec<_>>()
        };
        let mut errors = Vec::new();
        if refresh {
            for a in &accounts {
                let args: Args = serde_json::from_value(json!({"accountId":a.id}))
                    .map_err(|_| "查询参数无效")?;
                if let Err(error) = self.credit(&args).await {
                    errors.push(json!({"accountId":a.id,"message":error}));
                }
            }
        }
        let _guard = self.lock()?;
        let store = self.load()?;
        Ok(
            json!({"generatedAt":chrono::Utc::now().timestamp(),"retentionDays":365,"variant":region,
            "accounts":store.accounts.iter().filter(|a|a.variant==region).map(Account::meta).collect::<Vec<_>>(),
            "snapshots":store.snapshots.iter().filter(|s|s.variant==region&&s.at>=chrono::Utc::now().timestamp()-365*86400).collect::<Vec<_>>(),"errors":errors}),
        )
    }
}
