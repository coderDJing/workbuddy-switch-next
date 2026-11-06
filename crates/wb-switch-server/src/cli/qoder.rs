//! AI CLI adapter for Qoder's independent account store. No capture, daemon or hooks on reads.
use super::{
    args::{Command, Options},
    control::Control,
    output::progress,
    protocol::{error, response, Failure},
    target::Target,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};
use wb_switch_core::modules::{
    config,
    qoder::{
        self,
        proc::{QoderProcessOps, RealProcessOps},
        store,
    },
    state_lock::StateLock,
    token_stats,
    variant::WbVariant,
};

pub(super) fn capabilities() -> Value {
    json!({"codebuddy-cli":{"commands":["accounts","status","inspect","credits","switch","limits"]},
        "qoder-cli":{"commands":["accounts","status","inspect","credits","switch","token-stats","credit-stats","rewards","automation"],"variants":["cn","ai"],"modelLimits":false,"switchClosesRunningByDefault":false},
        "qoder-desktop":{"commands":["accounts","status","inspect","credits","switch","token-stats","credit-stats","rewards","automation"],"variants":["cn","ai"],"modelLimits":false,"switchClosesRunningByDefault":false}})
}
#[derive(Clone)]
struct Account {
    id: String,
    global: bool,
    name: String,
    bundles: Vec<store::BundleMeta>,
}
impl Account {
    fn ids(&self) -> Vec<String> {
        self.bundles.iter().map(|m| m.id.clone()).collect()
    }
    fn bundle(&self, target: Target) -> Option<&store::BundleMeta> {
        self.bundles
            .iter()
            .filter(|b| b.target == target.qoder_target(self.global))
            .max_by_key(|b| b.captured_at)
    }
    fn meta(&self, target: Target, clients: &[Value]) -> Value {
        let active = clients.iter().any(|c| c["activeAccountId"] == self.id);
        let mut targets: Vec<_> = self
            .bundles
            .iter()
            .map(|b| {
                serde_json::to_value(b.target)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        targets.sort();
        targets.dedup();
        json!({"id":self.id,"name":self.name,"nickname":self.name,"variant":if self.global {"ai"} else {"cn"},
            "activeForTarget":active,"clientPrepared":self.bundle(target).is_some(),
            "bundleId":self.bundle(target).map(|b|&b.id),"targets":targets})
    }
}
fn groups(mut bundles: Vec<store::BundleMeta>, options: &Options) -> Result<Vec<Account>, Failure> {
    bundles.sort_by(|a, b| {
        a.captured_at
            .cmp(&b.captured_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut groups: BTreeMap<(bool, String), Account> = BTreeMap::new();
    for b in bundles.into_iter().filter(|b| {
        options
            .variant
            .is_none_or(|v| (v == WbVariant::Ai) == b.target.is_global())
    }) {
        let global = b.target.is_global();
        let id = match &b.identity_key {
            Some(key) => format!("qoder-{}:{key}", if global { "ai" } else { "cn" }),
            None => format!("bundle:{}", b.id),
        };
        let group = groups
            .entry((global, id.clone()))
            .or_insert_with(|| Account {
                id,
                global,
                name: b.label.clone(),
                bundles: Vec::new(),
            });
        group.name = b.label.clone();
        group.bundles.push(b);
    }
    let accounts: Vec<_> = groups.into_values().collect();
    if let Some(id) = &options.account_id {
        let matches: Vec<_> = accounts
            .into_iter()
            .filter(|a| &a.id == id || a.bundles.iter().any(|b| &b.id == id))
            .collect();
        return match matches.len() {
            1=>Ok(matches),
            0=>Err(Failure{code:"ACCOUNT_NOT_FOUND",message:"Qoder account not found in selected region; run accounts with the same --target".into(),exit:2}),
            _=>Err(Failure{code:"AMBIGUOUS_ACCOUNT",message:"Use a stable Qoder account ID from accounts".into(),exit:2}),
        };
    }
    Ok(accounts)
}
/// Strict metadata reading: the GUI's tolerant loader must not turn corruption into an empty success.
fn load_at(root: &Path) -> Result<Vec<store::BundleMeta>, Failure> {
    let entries = match std::fs::read_dir(root.join("bundles")) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => {
            return Err(Failure::operation(
                "ACCOUNT_STORE_UNREADABLE",
                "Cannot read Qoder account store",
            ))
        }
    };
    let invalid = || {
        Failure::operation(
            "ACCOUNT_STORE_INVALID",
            "Invalid Qoder bundle metadata; repair the store before continuing",
        )
    };
    let mut result = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| invalid())?;
        let id = entry.file_name().to_string_lossy().to_string();
        if id.starts_with(".pending-") {
            continue;
        }
        if !entry.file_type().map_err(|_| invalid())?.is_dir() {
            return Err(invalid());
        }
        let meta = store::load(root, &id).map_err(|_| invalid())?;
        if meta.id != id
            || meta.label.trim().is_empty()
            || meta.identity_key.as_ref().is_some_and(|k| k.is_empty())
        {
            return Err(invalid());
        }
        result.push(meta);
    }
    Ok(result)
}
async fn lock(options: &Options, control: &Control, name: &str) -> Result<StateLock, Failure> {
    let start = Instant::now();
    loop {
        control.check()?;
        match StateLock::try_at(&store::default_store().join(name)) {
            Ok(guard) => return Ok(guard),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    && start.elapsed() < Duration::from_secs(options.wait) =>
            {
                tokio::select! {_ = control.cancelled()=>return Err(super::control::interrupted()),_ = tokio::time::sleep(Duration::from_millis(50))=>{}}
            }
            Err(e) => return Err(Failure::operation("STATE_LOCK_FAILED", e.to_string())),
        }
    }
}
async fn snapshot(
    options: &Options,
    control: &Control,
) -> Result<(Vec<Account>, Vec<Value>), Failure> {
    let _guard = lock(options, control, "switch.lock").await?;
    let all = groups(
        load_at(&store::default_store())?,
        &Options {
            account_id: None,
            ..options.clone()
        },
    )?;
    let target = options.target;
    let variant = options.variant;
    let mut clients = tokio::task::spawn_blocking(move || {
        [false, true]
            .into_iter()
            .filter(|g| variant.is_none_or(|v| (v == WbVariant::Ai) == *g))
            .map(|g| qoder::client_status(target.qoder_target(g)))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|_| Failure::operation("STATUS_FAILED", "Cannot read Qoder client status"))?;
    for client in &mut clients {
        let active = all.iter().find(|a| {
            a.bundles.iter().any(|b| {
                serde_json::to_value(b.target).ok().as_ref() == Some(&client["target"])
                    && (client["currentId"] == b.id
                        || b.identity_key
                            .as_deref()
                            .is_some_and(|k| client["currentIdentityKey"] == k))
            })
        });
        client["activeAccountId"] = json!(active.map(|a| &a.id));
        client["activeAccountName"] = json!(active.map(|a| &a.name));
    }
    let selected = groups(all.into_iter().flat_map(|a| a.bundles).collect(), options)?;
    Ok((selected, clients))
}
async fn query_accounts(
    accounts: &[Account],
    options: &Options,
    started: Instant,
    control: &Control,
) -> (Value, u8) {
    let count = accounts.len();
    let mut next = 0;
    let mut tasks = tokio::task::JoinSet::new();
    let mut results = BTreeMap::new();
    let queried = config::now_ms();
    loop {
        while !control.is_interrupted() && next < count && tasks.len() < options.concurrency {
            let a = accounts[next].clone();
            let index = next;
            next += 1;
            let timeout = options.timeout;
            tasks.spawn(async move {
                let at=config::now_ms();
                // Quota GET does not rotate credentials; cancellation is safe here.
                let mut credit=tokio::time::timeout(Duration::from_secs(timeout),qoder::credits::query(&a.ids(),None)).await
                    .unwrap_or_else(|_|json!({"ok":false,"error":"Qoder quota query timed out","errorCode":"TIMEOUT"}));
                credit["accountId"]=json!(a.id);credit["accountName"]=json!(a.name);credit["variant"]=json!(if a.global {"ai"} else {"cn"});
                credit["queryStartedAt"]=json!(at);credit["queriedAt"]=json!(config::now_ms());
                if credit["ok"]==true {
                    credit["totalRemaining"]=credit["remaining"].clone();credit["totalCapacity"]=credit["total"].clone();credit["totalUsed"]=credit["used"].clone();
                    credit["soonestExpireAt"]=json!(qoder::automation::expiry(&credit,config::now_ms()));
                }
                (index,credit)
            });
        }
        let Some(task) = tasks.join_next().await else {
            break;
        };
        if let Ok((index, result)) = task {
            results.insert(index, result);
            progress(
                options,
                "account_completed",
                "Qoder 积分查询完成",
                started,
                Some(json!({"completed":results.len(),"total":count})),
            );
        }
    }
    for (i, a) in accounts.iter().enumerate() {
        results.entry(i).or_insert_with(||json!({"ok":false,"accountId":a.id,"accountName":a.name,"variant":if a.global {"ai"} else {"cn"},"error":"Qoder query not completed","errorCode":if control.is_interrupted(){"INTERRUPTED"}else{"CREDIT_QUERY_FAILED"}}));
    }
    let (mut value, exit) =
        super::credits::summarize_credits(results.into_values().collect(), count, options.target);
    value["queryStartedAt"] = json!(queried);
    value["queriedAt"] = json!(config::now_ms());
    (value, exit)
}
fn source_matches(source: &Value, options: &Options) -> bool {
    options.variant.is_none_or(|v| {
        source["source"]
            == if v == WbVariant::Ai {
                "qoder-ai"
            } else {
                "qoder"
            }
    })
}
pub(super) async fn execute(
    options: &Options,
    started: Instant,
    control: &Control,
) -> Result<(Value, u8), Failure> {
    control.check()?;
    let mut value = response(options.command.as_str(), options.target);
    value["variant"] = json!(options.variant.map(|v| v.as_str()).unwrap_or("all"));
    if options.command == Command::Limits {
        return Err(Failure{code:"UNSUPPORTED_CAPABILITY",message:"Qoder model limits and reset times are not available; missing observations do not imply availability".into(),exit:2});
    }
    if options.command == Command::Automation {
        let global = options.variant.map(|v| v == WbVariant::Ai);
        value["action"] = json!(options.action);
        match options.action.as_str() {
            "set" => {
                use std::io::Read;
                let file = std::fs::File::open(options.settings_file.as_ref().unwrap())
                    .map_err(|_| Failure::usage("Cannot read --settings-file"))?;
                let mut bytes = Vec::new();
                file.take(65_537)
                    .read_to_end(&mut bytes)
                    .map_err(|_| Failure::usage("Cannot read --settings-file"))?;
                if bytes.len() > 65_536 {
                    return Err(Failure::usage("Settings file exceeds 64 KiB"));
                }
                let patch: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| Failure::usage("Settings file must contain a JSON object"))?;
                if !patch.is_object() {
                    return Err(Failure::usage("Settings file must contain a JSON object"));
                }
                value["result"] = qoder::automation::patch_region(global.unwrap(), patch)
                    .map_err(|e| Failure::operation("SETTINGS_FAILED", e))?;
            }
            "run" => {
                value["result"] =
                    qoder::automation::cycle_for(global, &|| control.is_interrupted())
                        .await
                        .map_err(|e| Failure::operation("AUTOMATION_FAILED", e))?;
                if value["result"]["skipped"] == true {
                    return Err(Failure::operation(
                        "AUTH_STATE_BUSY",
                        "Qoder automation is already running",
                    ));
                }
                let failed = value["result"]["outcomes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|v| v["outcome"] == "failed")
                    .count();
                if failed > 0 {
                    value["ok"] = json!(false);
                    value["error"] = error(
                        "AUTOMATION_PARTIAL",
                        "Some enabled tasks failed; consult result.outcomes",
                    );
                    return Ok((value, 3));
                }
            }
            _ => {
                let mut status = qoder::automation::status()
                    .map_err(|e| Failure::operation("SETTINGS_FAILED", e))?;
                if let Some(global) = global {
                    let key = if global { "ai" } else { "cn" };
                    status["settings"] = json!({key:status["settings"][key].clone()});
                    if let Some(logs) = status["logs"].as_array_mut() {
                        logs.retain(|l| l["region"] == key);
                    }
                }
                value["result"] = status;
            }
        }
        return Ok((value, 0));
    }
    if matches!(options.command, Command::TokenStats | Command::CreditStats) {
        let local = tokio::task::spawn_blocking(token_stats::get_qoder_statistics)
            .await
            .map_err(|_| Failure::operation("STATS_FAILED", "Qoder local usage scan failed"))?;
        value["generatedAt"] = local["generatedAt"].clone();
        if options.command == Command::TokenStats {
            value["sources"] = json!(local["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|s| source_matches(s, options))
                .collect::<Vec<_>>());
            value["accountAttributionAvailable"] = json!(false);
            return Ok((value, 0));
        }
        value["localCredits"] = json!(local["credits"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| source_matches(s, options))
            .collect::<Vec<_>>());
        let mut exit = 0;
        if options.refresh {
            let (selected, _) = snapshot(options, control).await?;
            let (refresh, code) = query_accounts(&selected, options, started, control).await;
            value["refresh"] = refresh;
            exit = code;
        }
        let mut stats = qoder::credit_stats::get_statistics(false).await;
        for key in ["accounts", "daily"] {
            if let Some(rows) = stats[key].as_array_mut() {
                rows.retain(|a| {
                    options
                        .variant
                        .is_none_or(|v| a["global"] == (v == WbVariant::Ai))
                });
            }
        }
        let _guard = lock(options, control, "switch.lock").await?;
        stats["savedAccountCount"] =
            json!(groups(load_at(&store::default_store())?, options)?.len());
        let errors = stats["errors"].as_array().is_some_and(|e| !e.is_empty());
        value["snapshots"] = stats;
        value["accountAttributionAvailable"] = json!(false);
        value["note"]=json!("Local request credits and observed quota deltas are separate measurements; do not add them together");
        if errors || exit != 0 {
            value["ok"] = json!(false);
            value["error"] = error(
                "STATS_PARTIAL",
                "Quota refresh or snapshot reading was incomplete",
            );
            if exit == 0 {
                exit = 1;
            }
        }
        return Ok((value, exit));
    }
    let (accounts, clients) = snapshot(options, control).await?;
    match options.command {
        Command::Accounts => {
            value["accounts"] = json!(accounts
                .iter()
                .map(|a| a.meta(options.target, &clients))
                .collect::<Vec<_>>());
            value["clients"] = json!(clients);
        }
        Command::Status => {
            value["clients"] = json!(clients);
            value["accountCount"] = json!(accounts.len());
        }
        Command::Credits | Command::Inspect => {
            let (mut queried, exit) = query_accounts(&accounts, options, started, control).await;
            queried["variant"] = value["variant"].clone();
            if options.command == Command::Inspect {
                queried["command"] = json!("inspect");
                queried["scope"] = json!("mixed");
                queried["availabilityKnown"] = json!(false);
                queried["limitsSupported"] = json!(false);
                // Re-read status after network operations; never report a stale active account as current.
                let status_options = Options {
                    account_id: None,
                    ..options.clone()
                };
                match snapshot(&status_options, control).await {
                    Ok((latest, current)) => {
                        let details: Vec<_> = accounts
                            .iter()
                            .enumerate()
                            .map(|(i, a)| {
                                let mut meta = a.meta(options.target, &current);
                                meta["present"] = json!(latest.iter().any(|b| b.id == a.id));
                                meta["credits"] = queried["accounts"][i].clone();
                                meta["availabilityKnown"] = json!(false);
                                meta
                            })
                            .collect();
                        queried["accounts"] = json!(details);
                        queried["clients"] = json!(current);
                    }
                    Err(e) => {
                        let details: Vec<_> = accounts
                            .iter()
                            .enumerate()
                            .map(|(i, a)| {
                                let mut meta = a.meta(options.target, &[]);
                                meta["present"] = Value::Null;
                                meta["activeForTarget"] = Value::Null;
                                meta["credits"] = queried["accounts"][i].clone();
                                meta["availabilityKnown"] = json!(false);
                                meta
                            })
                            .collect();
                        queried["accounts"] = json!(details);
                        queried["clients"] = Value::Null;
                        queried["clientStatusError"] = error(e.code, &e.message);
                        queried["ok"] = json!(false);
                        queried["error"] = error(
                            "STATUS_FAILED",
                            "Could not re-read client status after credit queries",
                        );
                        return Ok((queried, if exit == 0 { 1 } else { exit }));
                    }
                }
            }
            return Ok((queried, exit));
        }
        Command::Switch => {
            let a = &accounts[0];
            let bundle = a.bundle(options.target).ok_or_else(|| {
                Failure::operation(
                    "CLIENT_NOT_PREPARED",
                    "This account has no saved credentials for the selected Qoder client",
                )
            })?;
            value["account"] = a.meta(options.target, &clients);
            value["dryRun"] = json!(options.dry_run);
            let target = bundle.target;
            let impact=tokio::task::spawn_blocking(move||{
                let pids=RealProcessOps.running_pids(target);let idle=RealProcessOps.confirmed_idle(target);
                json!({"target":target,"running":!pids.is_empty(),"idleConfirmed":idle,"processIds":pids,"availabilityKnown":false})
            }).await.map_err(|_|Failure::operation("PROCESS_SCAN_FAILED","Could not inspect Qoder processes"))?;
            value["impact"] = impact.clone();
            value["willCloseRunningClient"] =
                json!(options.close_running && impact["running"] == true);
            value["relaunchRequested"] = json!(options.relaunch);
            if options.dry_run {
                value["note"]=json!("Preview only; process state is a snapshot. Credentials are not validated. Actual switch rechecks the target.");
            } else {
                if !options.close_running && impact["idleConfirmed"] != true {
                    return Err(Failure::operation(
                        "CLIENT_RUNNING",
                        "Qoder may be running; close it or explicitly pass --close-running",
                    ));
                }
                let wait = Instant::now();
                loop {
                    control.check()?;
                    let id = bundle.id.clone();
                    let close = options.close_running;
                    let relaunch = options.relaunch;
                    let result = tokio::task::spawn_blocking(move || {
                        qoder::switch::execute_with_timeout(
                            &store::default_store(),
                            &qoder::QoderRoots::real(),
                            &RealProcessOps,
                            &id,
                            qoder::SwitchOptions {
                                close_running: close,
                                relaunch,
                            },
                            Duration::ZERO,
                        )
                    })
                    .await
                    .map_err(|_| {
                        Failure::operation("SWITCH_FAILED", "Qoder switch worker failed")
                    })?;
                    match result {
                        Err(e)
                            if e.starts_with(wb_switch_core::modules::state_lock::BUSY_PREFIX)
                                && wait.elapsed() < Duration::from_secs(options.wait) =>
                        {
                            tokio::select! {_ = control.cancelled()=>return Err(super::control::interrupted()),_ = tokio::time::sleep(Duration::from_millis(50))=>{}}
                        }
                        result => {
                            value["result"] =
                                result.map_err(|e| Failure::operation("SWITCH_FAILED", e))?;
                            break;
                        }
                    }
                }
            }
        }
        Command::Rewards => {
            let mut results = Vec::new();
            for a in &accounts {
                if control.is_interrupted() {
                    results.push(json!({"accountId":a.id,"ok":false,"error":error("INTERRUPTED","Reward not started")}));
                    continue;
                }
                let result = qoder::credits::claim(&a.ids(), None).await;
                let ok = result["ok"] == true;
                results.push(json!({"accountId":a.id,"accountName":a.name,"variant":if a.global {"ai"}else{"cn"},"ok":ok,"message":result["message"],"error":if ok {Value::Null}else{error(if result["authInvalid"]==true{"AUTH_RELOGIN_REQUIRED"}else{"REWARD_FAILED"},result["message"].as_str().unwrap_or("Reward claim failed"))}}));
                progress(
                    options,
                    "account_completed",
                    "Qoder 每日领取已处理",
                    started,
                    Some(json!({"completed":results.len(),"total":accounts.len()})),
                );
            }
            let success = results.iter().filter(|v| v["ok"] == true).count();
            let exit = if success == accounts.len() {
                0
            } else if success == 0 {
                1
            } else {
                3
            };
            value["accounts"] = json!(results);
            value["summary"] = json!({"accountCount":accounts.len(),"successfulAccounts":success,"failedAccounts":accounts.len()-success,"complete":exit==0});
            if exit != 0 {
                value["ok"] = json!(false);
                value["error"] = error("REWARD_FAILED", "Some Qoder rewards were not claimed");
            }
            return Ok((value, exit));
        }
        _ => return Err(Failure::usage("Unsupported Qoder command")),
    }
    Ok((value, 0))
}
pub(super) fn compact(value: &Value) -> Value {
    let mut result = value.clone();
    result["outputMode"] = json!("summary");
    if let Some(accounts) = result["accounts"].as_array_mut() {
        for a in accounts {
            let credit = if value["command"] == "inspect" {
                &mut a["credits"]
            } else {
                a
            };
            if let Some(map) = credit.as_object_mut() {
                for key in ["plan", "addOn", "dedicated"] {
                    map.remove(key);
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options(command: &str) -> Options {
        super::super::parse(&[command.into(), "--target=qoder-cli".into()])
            .unwrap_or_else(|e| panic!("{}", e.message))
    }
    fn bundle(
        id: &str,
        target: qoder::QoderTarget,
        key: Option<&str>,
        at: i64,
    ) -> store::BundleMeta {
        store::BundleMeta {
            id: id.into(),
            target,
            label: "Same name".into(),
            identity_key: key.map(str::to_owned),
            echo_name: None,
            captured_at: at,
            last_switched_at: None,
            files: vec![],
        }
    }
    #[test]
    fn groups_verified_identities_and_regions_but_never_display_names() {
        use qoder::QoderTarget as Q;
        let bundles = vec![
            bundle("cn-old", Q::Cli, Some("verified"), 1),
            bundle("cn-new", Q::Cli, Some("verified"), 3),
            bundle("cn-desktop", Q::Desktop, Some("verified"), 2),
            bundle("ai", Q::GlobalCli, Some("verified"), 4),
            bundle("unknown-a", Q::Cli, None, 1),
            bundle("unknown-b", Q::Cli, None, 1),
        ];
        let o = options("accounts");
        let all = groups(bundles.clone(), &o).unwrap_or_else(|e| panic!("{}", e.message));
        assert_eq!(all.len(), 4);
        let cn = all.iter().find(|a| a.id == "qoder-cn:verified").unwrap();
        assert_eq!(cn.bundle(o.target).unwrap().id, "cn-new");
        let selected = groups(
            bundles.clone(),
            &Options {
                account_id: Some("cn-desktop".into()),
                ..o.clone()
            },
        )
        .unwrap_or_else(|e| panic!("{}", e.message));
        assert_eq!(selected[0].id, cn.id);
        assert!(groups(
            bundles,
            &Options {
                account_id: Some(cn.id.clone()),
                variant: Some(WbVariant::Ai),
                ..o
            }
        )
        .is_err());
        let meta = cn.meta(Target::QoderCli, &[json!({"activeAccountId":cn.id})]);
        assert_eq!(meta["activeForTarget"], true);
        assert!(meta.get("files").is_none());
    }
    #[test]
    fn corrupt_bundle_is_not_an_empty_library_and_staging_is_not_an_account() {
        let root = std::env::temp_dir().join(format!("qoder-cli-store-{}", std::process::id()));
        std::fs::create_dir_all(root.join("bundles/.pending-fixture")).unwrap();
        assert!(load_at(&root)
            .unwrap_or_else(|e| panic!("{}", e.message))
            .is_empty());
        std::fs::create_dir_all(root.join("bundles/fixture")).unwrap();
        std::fs::write(root.join("bundles/fixture/bundle.json"), "{broken SECRET}").unwrap();
        let e = load_at(&root).err().unwrap();
        assert_eq!(e.code, "ACCOUNT_STORE_INVALID");
        assert!(!e.message.contains("SECRET"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn compact_keeps_partial_errors_precision_and_unknown_availability() {
        let full = json!({"schemaVersion":1,"command":"inspect","target":"qoder-cli","ok":false,"error":{"code":"PARTIAL"},"availabilityKnown":false,"clients":[{"activeAccountId":"a"}],"accounts":[{"id":"a","credits":{"ok":true,"totalRemaining":1.234567,"plan":{"raw":true},"totalRemainingDisplay":"1.23"}},{"id":"b","credits":{"ok":false,"error":{"code":"TIMEOUT"}}}]});
        let brief = compact(&full);
        assert_eq!(brief["accounts"][0]["credits"]["totalRemaining"], 1.234567);
        assert!(brief["accounts"][0]["credits"].get("plan").is_none());
        assert_eq!(brief["accounts"][1]["credits"]["error"]["code"], "TIMEOUT");
        assert_eq!(brief["availabilityKnown"], false);
        assert_eq!(brief["clients"], full["clients"]);
    }
}
