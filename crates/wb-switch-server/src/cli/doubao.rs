use super::{
    args::{Command, Options},
    control::Control,
    output,
    protocol::{error, response, Failure},
};
use serde_json::{json, Value};
use wb_switch_core::modules::doubao;
async fn action(name: &str, input: Value) -> Result<Value, Failure> {
    doubao::action(name, input)
        .await
        .map_err(|e| Failure::operation("DOUBAO_OPERATION_FAILED", e))
}
pub(super) async fn execute(o: &Options, control: &Control) -> Result<(Value, u8), Failure> {
    control.check()?;
    let mut out = response(o.command.as_str(), o.target);
    if o.command == Command::Automation {
        out["automation"] = match o.action.as_str() {
            "set" => {
                let file = o
                    .settings_file
                    .as_deref()
                    .ok_or_else(|| Failure::usage("Missing settings file"))?;
                let bytes = std::fs::read(file)
                    .ok()
                    .filter(|v| v.len() < 1024 * 1024)
                    .ok_or_else(|| Failure::usage("Cannot read settings file"))?;
                let patch = serde_json::from_slice(&bytes)
                    .map_err(|_| Failure::usage("Invalid settings JSON"))?;
                doubao::automation::patch(patch)
                    .map_err(|e| Failure::operation("DOUBAO_SETTINGS_FAILED", e))?
            }
            "run" => action("automation_run", json!({})).await?,
            _ => action("get_settings", json!({})).await?,
        };
        let failed = out["automation"]["errors"]
            .as_array()
            .is_some_and(|v| !v.is_empty());
        out["ok"] = json!(!failed);
        return Ok((out, if failed { 3 } else { 0 }));
    }
    if o.command == Command::CreditStats {
        out["statistics"] = action("statistics", json!({"refresh":o.refresh})).await?;
        let failed = out["statistics"]["errors"]
            .as_array()
            .is_some_and(|v| !v.is_empty());
        out["ok"] = json!(!failed);
        return Ok((out, if failed { 3 } else { 0 }));
    }
    if o.command == Command::Login {
        let start = std::time::Instant::now();
        action("open", json!({})).await?;
        output::progress(
            o,
            "authorization_required",
            "请在豆包客户端登录，然后从托盘完全退出；本命令将在退出后保存账号（最多等待 5 分钟）",
            start,
            None,
        );
        loop {
            control.check()?;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if start.elapsed().as_secs() > 300 {
                out["ok"] = json!(false);
                out["status"] = json!("expired");
                out["error"] = error("DOUBAO_LOGIN_TIMEOUT", "尚未完成登录并退出豆包");
                return Ok((out, 3));
            }
            let state = tokio::task::spawn_blocking(doubao::status)
                .await
                .map_err(|_| Failure::operation("DOUBAO_STATUS_FAILED", "状态任务异常"))?
                .map_err(|e| Failure::operation("DOUBAO_STATUS_FAILED", e))?;
            if state["client"]["running"] == false && state["client"]["configured"] == true {
                out["account"] = action("capture", json!({})).await?;
                out["status"] = json!("authorized");
                return Ok((out, 0));
            }
        }
    }
    if !matches!(
        o.command,
        Command::Accounts
            | Command::Status
            | Command::Inspect
            | Command::Credits
            | Command::Rewards
            | Command::Switch
    ) {
        return Err(Failure{code:"UNSUPPORTED_CAPABILITY",message:"Doubao supports accounts/status/inspect/credits/rewards/switch/login/credit-stats/automation; native CLI and Token statistics are unavailable".into(),exit:2});
    }
    let state = tokio::task::spawn_blocking(doubao::status)
        .await
        .map_err(|_| Failure::operation("DOUBAO_STATUS_FAILED", "状态任务异常"))?
        .map_err(|e| Failure::operation("DOUBAO_STATUS_FAILED", e))?;
    let accounts: Vec<_> = state["accounts"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| o.account_id.as_deref().is_none_or(|id| a["id"] == id))
        .map(|mut a| {
            a["activeForTarget"] = a["active"].clone();
            a
        })
        .collect();
    if o.account_id.is_some() && accounts.is_empty() {
        return Err(Failure {
            code: "ACCOUNT_NOT_FOUND",
            message: "Doubao account not found".into(),
            exit: 2,
        });
    }
    out["clientStatus"] = state["client"].clone();
    out["nativeCliSupported"] = json!(false);
    out["accountCount"] = json!(accounts.len());
    out["accounts"] = json!(accounts);
    if o.command == Command::Switch {
        out["dryRun"] = json!(o.dry_run);
        out["result"]=action("switch",json!({"accountId":o.account_id,"dryRun":o.dry_run,"closeRunning":o.close_running,"relaunch":o.relaunch})).await?;
        return Ok((out, 0));
    }
    if matches!(
        o.command,
        Command::Credits | Command::Inspect | Command::Rewards
    ) {
        let rewards = o.command == Command::Rewards;
        let mut values = vec![];
        let mut failed = 0;
        for a in accounts {
            control.check()?;
            match action(
                if rewards { "checkin" } else { "credits" },
                json!({"accountId":a["id"],"timeoutSeconds":o.timeout}),
            )
            .await
            {
                Ok(v) => {
                    let ok = !rewards || v["ok"] == true;
                    if !ok {
                        failed += 1;
                    }
                    let mut entry = json!({"accountId":a["id"],"accountName":a["name"],"ok":ok});
                    if rewards {
                        entry["state"] = v["state"].clone();
                        entry["message"] = v["message"].clone();
                        entry["credits"] = v["credits"].clone();
                    } else {
                        entry["balance"] = v;
                    }
                    values.push(entry);
                }
                Err(e) => {
                    failed += 1;
                    values.push(
                        json!({"accountId":a["id"],"ok":false,"error":error(e.code,&e.message)}),
                    );
                }
            }
        }
        out[if rewards { "rewards" } else { "credits" }] = json!(values);
        out["complete"] = json!(failed == 0);
        out["ok"] = json!(failed == 0);
        if failed > 0 {
            out["error"] = error(
                "PARTIAL_DOUBAO_RESULTS",
                "部分豆包操作未完成，请查看每个账号的结果",
            );
        }
        return Ok((out, if failed == 0 { 0 } else { 3 }));
    }
    Ok((out, 0))
}
