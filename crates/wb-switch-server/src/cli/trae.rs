//! Management of TRAE desktop snapshots, not the enterprise TRAE native CLI.
use super::{
    args::{Command, Options},
    control::Control,
    protocol::{error, response, Failure},
};
use serde_json::{json, Value};
use wb_switch_core::modules::trae;

pub(super) async fn execute(options: &Options, control: &Control) -> Result<(Value, u8), Failure> {
    if matches!(options.command, Command::CreditStats | Command::Automation) {
        let mut value = response(options.command.as_str(), options.target);
        let regions = options
            .variant
            .map(|v| vec![v.as_str()])
            .unwrap_or_else(|| vec!["cn", "ai"]);
        control.check()?;
        if options.command == Command::Automation && options.action == "set" {
            let path = options
                .settings_file
                .as_deref()
                .ok_or_else(|| Failure::usage("Missing settings file"))?;
            let bytes = std::fs::read(path)
                .ok()
                .filter(|b| b.len() <= 1024 * 1024)
                .ok_or_else(|| Failure::usage("Cannot read settings file or file too large"))?;
            let patch = serde_json::from_slice(&bytes)
                .map_err(|_| Failure::usage("Settings file must contain valid JSON"))?;
            let region = if regions[0] == "ai" {
                trae::Region::Ai
            } else {
                trae::Region::Cn
            };
            value["automation"] = trae::automation::patch_region(region, patch)
                .map_err(|e| Failure::operation("TRAE_SETTINGS_FAILED", e))?;
        } else if options.command == Command::Automation && options.action == "get" {
            let mut result = trae::action("get_settings", json!({}))
                .await
                .map_err(|e| Failure::operation("TRAE_SETTINGS_FAILED", e))?;
            if let Some(variant) = options.variant {
                result["settings"]
                    .as_object_mut()
                    .unwrap()
                    .retain(|key, _| key == variant.as_str());
                if let Some(logs) = result["logs"].as_array_mut() {
                    logs.retain(|l| l["region"] == variant.as_str());
                }
            }
            value["automation"] = result;
        } else {
            let mut results = Vec::new();
            for region in regions {
                control.check()?;
                let action = if options.command == Command::CreditStats {
                    "statistics"
                } else {
                    "automation_run"
                };
                results.push(
                    trae::action(action, json!({"variant":region,"refresh":options.refresh}))
                        .await
                        .map_err(|e| Failure::operation("TRAE_OPERATION_FAILED", e))?,
                );
            }
            value["regions"] = json!(results);
        }
        let partial = value["regions"].as_array().is_some_and(|regions| {
            regions.iter().any(|r| {
                r["errors"]
                    .as_array()
                    .is_some_and(|errors| !errors.is_empty())
            })
        });
        if partial {
            value["ok"] = json!(false);
            value["error"] = error("PARTIAL_CREDITS", "Some TRAE statistics refreshes failed");
        }
        return Ok((value, if partial { 3 } else { 0 }));
    }
    if !matches!(
        options.command,
        Command::Accounts
            | Command::Status
            | Command::Inspect
            | Command::Credits
            | Command::Rewards
            | Command::Switch
    ) {
        return Err(Failure{code:"UNSUPPORTED_CAPABILITY",message:"TRAE desktop supports accounts/status/inspect/credits/rewards/switch. Login in the official client, then use call trae_action with action=capture. Native TRAE CLI is not integrated.".into(),exit:2});
    }
    control.check()?;
    let status = tokio::task::spawn_blocking(trae::status)
        .await
        .map_err(|_| Failure::operation("TRAE_TASK_FAILED", "TRAE status task failed"))?
        .map_err(|e| Failure::operation("TRAE_STATUS_FAILED", e))?;
    let client = "desktop";
    let region = options.variant.map(|v| v.as_str());
    let clients: Vec<_> = status["clients"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["client"] == client && region.is_none_or(|r| c["variant"] == r))
        .cloned()
        .collect();
    let mut accounts: Vec<_> = status["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| {
            a["client"] == client
                && region.is_none_or(|r| a["variant"] == r)
                && options.account_id.as_deref().is_none_or(|id| a["id"] == id)
        })
        .cloned()
        .collect();
    if options.account_id.is_some() && accounts.is_empty() {
        return Err(Failure {
            code: "ACCOUNT_NOT_FOUND",
            message: "TRAE account not found for this target/region".into(),
            exit: 2,
        });
    }
    for a in &mut accounts {
        a["activeForTarget"] = json!(clients.iter().any(|c| c["activeAccountId"] == a["id"]));
    }
    let mut value = response(options.command.as_str(), options.target);
    value["clientStatus"] = json!({"clients":clients});
    value["accountCount"] = json!(accounts.len());
    if options.command == Command::Switch {
        let a = accounts
            .first()
            .ok_or_else(|| Failure::usage("Select a TRAE account ID"))?;
        control.check()?;
        value["account"] = a.clone();
        value["dryRun"] = json!(options.dry_run);
        value["result"]=trae::action("switch",json!({"accountId":a["id"],"variant":a["variant"],"client":client,"dryRun":options.dry_run,"closeRunning":options.close_running,"relaunch":options.relaunch})).await.map_err(|e|Failure::operation("TRAE_SWITCH_FAILED",e))?;
        return Ok((value, 0));
    }
    let mut failed = 0;
    if options.command == Command::Rewards {
        let mut rewards = Vec::new();
        for a in &accounts {
            control.check()?;
            if a["variant"] != "cn" {
                failed += 1;
                rewards.push(json!({"accountId":a["id"],"accountName":a["name"],"variant":a["variant"],"ok":false,
                    "error":error("UNSUPPORTED_REGION","TRAE daily check-in is only available for the CN build")}));
                continue;
            }
            match trae::action(
                "checkin",
                json!({"accountId":a["id"],"timeoutSeconds":options.timeout}),
            )
            .await
            {
                Ok(result) => {
                    let ok = result["ok"].as_bool() == Some(true);
                    if !ok {
                        failed += 1;
                    }
                    let message = result["message"].as_str().unwrap_or("Reward claim failed");
                    rewards.push(json!({"accountId":a["id"],"accountName":a["name"],"variant":a["variant"],"ok":ok,
                        "message":result["message"],"state":result["state"],"credits":result["credits"],
                        "error":if ok {Value::Null}else{error(if result["authInvalid"]==true{"AUTH_RELOGIN_REQUIRED"}else{"REWARD_FAILED"},message)}}));
                }
                Err(e) => {
                    failed += 1;
                    rewards.push(json!({"accountId":a["id"],"accountName":a["name"],"variant":a["variant"],"ok":false,
                        "error":error("REWARD_FAILED",&e)}));
                }
            }
        }
        value["rewards"] = json!(rewards);
        value["complete"] = json!(failed == 0);
    }
    if matches!(options.command, Command::Inspect | Command::Credits) {
        let mut credits = Vec::new();
        for a in &accounts {
            control.check()?;
            let mut credit = json!({"accountId":a["id"],"variant":a["variant"]});
            match trae::action(
                "credits",
                json!({"accountId":a["id"],"timeoutSeconds":options.timeout}),
            )
            .await
            {
                Ok(v) => {
                    credit["ok"] = json!(true);
                    credit["balance"] = v;
                }
                Err(e) => {
                    failed += 1;
                    credit["ok"] = json!(false);
                    credit["error"] = error("TRAE_CREDITS_FAILED", &e);
                }
            }
            credits.push(credit);
        }
        value["credits"] = json!(credits);
        value["complete"] = json!(failed == 0);
    }
    value["accounts"] = json!(accounts);
    value["nativeCliSupported"] = json!(false);
    value["ok"] = json!(failed == 0);
    if failed > 0 {
        value["error"] = if options.command == Command::Rewards {
            error(
                "PARTIAL_REWARDS",
                "Some TRAE reward claims failed; retry later or sign in again",
            )
        } else {
            error(
                "PARTIAL_CREDITS",
                "Some TRAE credit queries failed; unknown balances are not zero",
            )
        };
    }
    Ok((value, if failed > 0 { 3 } else { 0 }))
}
