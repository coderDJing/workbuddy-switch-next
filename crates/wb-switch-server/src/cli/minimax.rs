//! MiniMax Code 的桌面端 / CLI 账号管理。
//!
//! 桌面端与 CLI 共用同一个登录槽，因此只有一个目标（`minimax-desktop`），
//! `--variant cn` 指国内版、`--variant ai` 指国际版（凭据目录里的 `en`）。
use super::{
    args::{Command, Options},
    control::Control,
    output,
    protocol::{error, response, Failure},
};
use serde_json::{json, Value};
use wb_switch_core::modules::minimax;

pub(super) fn region_arg(options: &Options) -> Option<&'static str> {
    options
        .variant
        .map(|v| if v.as_str() == "ai" { "en" } else { "cn" })
}

pub(super) async fn execute(options: &Options, control: &Control) -> Result<(Value, u8), Failure> {
    if !matches!(
        options.command,
        Command::Accounts
            | Command::Status
            | Command::Inspect
            | Command::Credits
            | Command::Switch
            | Command::Login
    ) {
        return Err(Failure{code:"UNSUPPORTED_CAPABILITY",message:"MiniMax Code supports accounts/status/inspect/credits/switch/login. The desktop app and its CLI share one login slot; there is no per-client target.".into(),exit:2});
    }
    if options.command == Command::Login {
        return login(options, control).await;
    }
    control.check()?;
    let status = tokio::task::spawn_blocking(minimax::status)
        .await
        .map_err(|_| Failure::operation("MINIMAX_TASK_FAILED", "MiniMax status task failed"))?
        .map_err(|e| Failure::operation("MINIMAX_STATUS_FAILED", e))?;
    let region = region_arg(options);
    let clients: Vec<_> = status["clients"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|c| region.is_none_or(|r| c["region"] == r))
        .collect();
    let accounts: Vec<_> = status["accounts"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| {
            region.is_none_or(|r| a["region"] == r)
                && options.account_id.as_deref().is_none_or(|id| a["id"] == id)
        })
        .collect();
    if options.account_id.is_some() && accounts.is_empty() {
        return Err(Failure {
            code: "ACCOUNT_NOT_FOUND",
            message: "MiniMax account not found for this region".into(),
            exit: 2,
        });
    }
    let mut value = response(options.command.as_str(), options.target);
    value["clientStatus"] = json!({"clients":clients});
    value["accountCount"] = json!(accounts.len());
    value["sharedLogin"] = json!(true);
    if options.command == Command::Switch {
        let account = accounts
            .first()
            .ok_or_else(|| Failure::usage("Select a MiniMax account ID"))?;
        control.check()?;
        value["account"] = account.clone();
        value["dryRun"] = json!(options.dry_run);
        value["result"] = minimax::action(
            "switch",
            json!({
                "region": account["region"],
                "accountId": account["id"],
                "dryRun": options.dry_run,
                "closeRunning": options.close_running,
                "relaunch": options.relaunch,
            }),
        )
        .await
        .map_err(|e| Failure::operation("MINIMAX_SWITCH_FAILED", e))?;
        return Ok((value, 0));
    }
    let mut failed = 0;
    if matches!(options.command, Command::Inspect | Command::Credits) {
        let mut credits = Vec::new();
        for account in &accounts {
            control.check()?;
            let mut credit = json!({"accountId":account["id"],"region":account["region"]});
            match minimax::action(
                "credits",
                json!({"accountId":account["id"],"timeoutSeconds":options.timeout}),
            )
            .await
            {
                Ok(result) => {
                    credit["ok"] = json!(true);
                    credit["balance"] = result;
                }
                Err(e) => {
                    failed += 1;
                    credit["ok"] = json!(false);
                    credit["error"] = error("MINIMAX_CREDITS_FAILED", &e);
                }
            }
            credits.push(credit);
        }
        value["credits"] = json!(credits);
        value["complete"] = json!(failed == 0);
    }
    let active: Vec<&str> = value["clientStatus"]["clients"]
        .as_array()
        .map(|clients| {
            clients
                .iter()
                .filter_map(|c| c["activeAccountId"].as_str())
                .collect()
        })
        .unwrap_or_default();
    let accounts: Vec<Value> = accounts
        .into_iter()
        .map(|mut account| {
            let is_active = account["id"]
                .as_str()
                .is_some_and(|id| active.contains(&id));
            account["activeForTarget"] = json!(is_active);
            account
        })
        .collect();
    value["accounts"] = json!(accounts);
    value["nativeCliSupported"] = json!(true);
    value["ok"] = json!(failed == 0);
    if failed > 0 {
        value["error"] = error(
            "PARTIAL_CREDITS",
            "Some MiniMax credit queries failed; unknown balances are not zero",
        );
    }
    Ok((value, if failed > 0 { 3 } else { 0 }))
}

/// 设备码登录：先打印用户码与授权地址，再在本进程内轮询到授权完成。
async fn login(options: &Options, control: &Control) -> Result<(Value, u8), Failure> {
    let started_at = std::time::Instant::now();
    let region = region_arg(options).unwrap_or("cn");
    control.check()?;
    let started = minimax::action("login_start", json!({"region":region}))
        .await
        .map_err(|e| Failure::operation("MINIMAX_LOGIN_FAILED", e))?;
    let login_id = started["loginId"]
        .as_str()
        .ok_or_else(|| Failure::operation("MINIMAX_LOGIN_FAILED", "缺少登录会话"))?
        .to_string();
    let expires_at = std::time::Instant::now()
        + std::time::Duration::from_secs(
            started["expiresIn"].as_u64().unwrap_or(300).clamp(30, 900),
        );
    let mut value = response(options.command.as_str(), options.target);
    value["region"] = json!(region);
    value["userCode"] = started["userCode"].clone();
    value["verificationUri"] = started["verificationUri"].clone();
    value["verificationUriComplete"] = started["verificationUriComplete"].clone();
    value["expiresIn"] = started["expiresIn"].clone();
    // 用户码必须在轮询开始前就可见，否则调用方要等到超时才知道该输入什么。
    output::progress(
        options,
        "authorization_required",
        &format!(
            "请在浏览器打开 {} 并确认用户码 {}",
            started["verificationUriComplete"]
                .as_str()
                .or_else(|| started["verificationUri"].as_str())
                .unwrap_or(""),
            started["userCode"].as_str().unwrap_or("")
        ),
        started_at,
        Some(json!({
            "region": region,
            "userCode": started["userCode"],
            "verificationUri": started["verificationUri"],
            "verificationUriComplete": started["verificationUriComplete"],
            "expiresIn": started["expiresIn"],
        })),
    );

    let mut interval =
        std::time::Duration::from_millis(started["intervalMs"].as_u64().unwrap_or(3000).max(1000));
    loop {
        control.check()?;
        if std::time::Instant::now() > expires_at {
            let _ = minimax::action("login_cancel", json!({"loginId":login_id})).await;
            value["status"] = json!("expired");
            value["ok"] = json!(false);
            value["error"] = error("MINIMAX_LOGIN_EXPIRED", "Authorization code expired");
            return Ok((value, 3));
        }
        tokio::time::sleep(interval).await;
        control.check()?;
        let polled = minimax::action("login_poll", json!({"loginId":login_id}))
            .await
            .map_err(|e| Failure::operation("MINIMAX_LOGIN_FAILED", e))?;
        match polled["status"].as_str().unwrap_or_default() {
            "authorized" => {
                value["status"] = json!("authorized");
                value["account"] = polled["account"].clone();
                value["ok"] = json!(true);
                return Ok((value, 0));
            }
            "failed" | "expired" => {
                let message = polled["error"]
                    .as_str()
                    .unwrap_or("Authorization failed")
                    .to_string();
                value["status"] = json!("failed");
                value["ok"] = json!(false);
                value["error"] = error("MINIMAX_LOGIN_FAILED", &message);
                return Ok((value, 3));
            }
            _ => {
                if polled["slowDown"].as_bool() == Some(true) {
                    interval = (interval * 2).min(std::time::Duration::from_secs(30));
                }
                if let Some(wait) = polled["retryAfterMs"].as_u64() {
                    interval = std::time::Duration::from_millis(wait.max(1000));
                }
            }
        }
    }
}
