//! Complete named-operation access to the same backend used by the WebUI.
//! Requests run in-process: no listener, background scheduler, or HTTP client.
use super::{
    args::{Command, Options},
    control::Control,
    output::progress,
    protocol::{response, Failure},
};
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    time::{Duration, Instant},
};
use tower::ServiceExt;
use wb_switch_core::modules::{account, config, oauth, qoder, variant::WbVariant};

const CATALOG: &str = include_str!("../../../../src/lib/operations.json");
const MAX_INPUT: u64 = 64 * 1024 * 1024;

struct Destination {
    file: Option<std::fs::File>,
    path: std::path::PathBuf,
    committed: bool,
}
impl Destination {
    fn create(path: &str) -> Result<Self, Failure> {
        let path = std::path::absolute(path).map_err(|_| Failure::usage("Invalid output path"))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|_| {
            Failure::usage("Cannot create --output-file; destination must not exist")
        })?;
        Ok(Self {
            file: Some(file),
            path,
            committed: false,
        })
    }
}
impl Drop for Destination {
    fn drop(&mut self) {
        self.file.take();
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn workflow(name: &str) -> bool {
    matches!(
        name,
        "oauth_start"
            | "oauth_status"
            | "qoder_oauth_start"
            | "qoder_oauth_poll"
            | "qoder_oauth_cancel"
            | "switch_progress"
    )
}
fn exports_credentials(name: &str) -> bool {
    matches!(
        name,
        "export_accounts" | "export_all_accounts" | "export_qoder_accounts"
    )
}
fn read_only(name: &str) -> bool {
    !matches!(
        name,
        "get_credit_expiry"
            | "get_qoder_credits"
            | "get_checkin_status"
            | "get_credit_statistics"
            | "get_qoder_credit_statistics"
    ) && (name.starts_with("get_")
        || name.starts_with("list_")
        || name.starts_with("preview_")
        || name.contains("_preview")
        || matches!(name, "rotate_status" | "webdav_info"))
}
pub(super) fn catalog() -> Value {
    let mut value: Value = serde_json::from_str(CATALOG).expect("bundled operation catalog");
    for (name, entry) in value.as_object_mut().unwrap() {
        entry["readOnly"] = json!(read_only(name));
        entry["requiresOutputFile"] = json!(exports_credentials(name));
        entry["oneShot"] = json!(!workflow(name));
        if workflow(name) {
            entry["note"] = json!("Process-local workflow: use login, or the synchronous switch command; split polling calls do not survive process exit");
        }
        entry["inputTransport"] = json!(
            "JSON object via --input-file PATH (or - for stdin); GET accepts scalar values only"
        );
        if name.starts_with("switch_") {
            entry["note"] = json!("May interrupt running tasks when restart/closeRunning is explicitly true. Prefer switch --target TARGET for process policy and dry-run.");
        }
    }
    value
}

fn validate_input(entry: &Value, input: &Value) -> Result<(), Failure> {
    let parameters = entry["parameters"]
        .as_object()
        .expect("operation input contract");
    for key in input.as_object().unwrap().keys() {
        if !parameters.contains_key(key) {
            return Err(Failure::usage(format!(
                "Unknown input field {key}; run operations OPERATION"
            )));
        }
    }
    for (key, spec) in parameters {
        let Some(value) = input.get(key) else {
            if spec["required"] == true {
                return Err(Failure::usage(format!("Missing input field {key}")));
            }
            continue;
        };
        let kind = spec["type"].as_str().unwrap_or("");
        let valid = if value.is_null() {
            kind.contains("null")
        } else if let Some(values) = spec["enum"].as_array() {
            values.contains(value)
        } else if kind.starts_with('{') || kind.starts_with("Record<") {
            value.is_object()
        } else if kind.contains("[]") {
            value.is_array()
        } else if kind == "string" || kind.starts_with("string |") {
            value.is_string()
        } else if kind == "number" || kind.starts_with("number |") {
            value.is_number()
        } else if kind == "boolean" || kind.starts_with("boolean |") {
            value.is_boolean()
        } else {
            true
        };
        if !valid {
            return Err(Failure::usage(format!("Invalid input type for {key}")));
        }
    }
    Ok(())
}

fn read_input(file: Option<&str>) -> Result<Value, Failure> {
    let Some(file) = file else {
        return Ok(json!({}));
    };
    let mut bytes = Vec::new();
    let reader: Box<dyn Read> = if file == "-" {
        Box::new(std::io::stdin())
    } else {
        Box::new(std::fs::File::open(file).map_err(|_| Failure::usage("Cannot open --input-file"))?)
    };
    reader
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::usage("Cannot read --input-file"))?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err(Failure::usage("Input exceeds 64 MiB"));
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    let input: Value = serde_json::from_slice(bytes)
        .map_err(|_| Failure::usage("Input must be valid JSON (contents omitted)"))?;
    if !input.is_object() {
        return Err(Failure::usage("Input must be a JSON object"));
    }
    Ok(input)
}

// Preserve preview tokens used to approve a specific session-sync plan. Login
// credentials and saved service passwords must never reach ordinary stdout.
pub(super) fn redact(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let key = key.replace(['_', '-'], "").to_ascii_lowercase();
                if key != "previewtoken"
                    && !(key.starts_with("has") && child.is_boolean())
                    && (key.ends_with("token")
                        || key.contains("password")
                        || key.contains("passphrase")
                        || matches!(
                            key.as_str(),
                            "authorization"
                                | "cookie"
                                | "cookies"
                                | "secret"
                                | "clientsecret"
                                | "credential"
                                | "credentials"
                                | "apikey"
                                | "sessionid"
                                | "sidguard"
                                | "encryptedkey"
                                | "envelope"
                                | "authraw"
                                | "authdata"
                                | "sessionjson"
                                | "secretkey"
                                | "privatekey"
                        ))
                {
                    *child = if child.is_null() {
                        Value::Null
                    } else {
                        json!("[REDACTED]")
                    };
                } else if key == "error" && child.is_string() && child != "" {
                    *child = json!("Operation failed (raw error omitted)");
                } else {
                    redact(child);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact),
        _ => {}
    }
}

fn percent(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

pub(super) async fn request(method: &str, path: &str, input: Value) -> Result<Value, Failure> {
    let mut uri = path.to_string();
    if method == "GET" {
        let mut query = Vec::new();
        for (key, value) in input
            .as_object()
            .ok_or_else(|| Failure::usage("Input must be an object"))?
        {
            if value.is_null() {
                continue;
            }
            if value.is_object() || value.is_array() {
                return Err(Failure::usage("GET operation inputs must be scalar"));
            }
            let value = value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string());
            query.push(format!("{}={}", percent(key), percent(&value)));
        }
        if !query.is_empty() {
            uri.push('?');
            uri.push_str(&query.join("&"));
        }
    }
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(if method == "POST" {
            Body::from(input.to_string())
        } else {
            Body::empty()
        })
        .map_err(|_| Failure::usage("Cannot build operation request"))?;
    let res = crate::api::router()
        .oneshot(req)
        .await
        .map_err(|_| Failure::operation("OPERATION_FAILED", "Backend dispatch failed"))?;
    let status = res.status();
    let bytes = to_bytes(res.into_body(), MAX_INPUT as usize)
        .await
        .map_err(|_| Failure::operation("OPERATION_FAILED", "Cannot read backend result"))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        Failure::operation("OPERATION_FAILED", "Backend returned a non-JSON result")
    })?;
    if !status.is_success() {
        // Some older handlers include raw remote errors. Do not echo them or
        // submitted credentials in the CLI's structured error envelope.
        return Err(Failure::operation(
            "OPERATION_FAILED",
            format!(
                "Backend rejected operation (HTTP {}); no input or remote response is included",
                status.as_u16()
            ),
        ));
    }
    Ok(value)
}

fn result_exit(result: &Value) -> u8 {
    let mut child_success = false;
    let mut child_failure = false;
    let mut child_partial = false;
    for key in ["accounts", "results"] {
        if let Some(items) = result[key].as_array() {
            for item in items {
                match result_exit(item) {
                    0 => child_success = true,
                    3 => {
                        child_partial = true;
                        child_success = true;
                        child_failure = true;
                    }
                    _ => child_failure = true,
                }
            }
        }
    }
    let progress = ["copied", "imported", "succeeded", "successCount", "synced"]
        .iter()
        .any(|key| result[*key].as_u64().unwrap_or(0) > 0);
    if result.get("ok") == Some(&json!(false))
        || result.get("success") == Some(&json!(false))
        || result.get("result") == Some(&json!("error"))
        || result.get("error").is_some_and(|v| !v.is_null() && v != "")
    {
        return if progress || child_success { 3 } else { 1 };
    }
    let failures = result["failed"].as_u64().unwrap_or(0)
        + result["errors"].as_array().map_or(0, |v| v.len() as u64);
    if failures > 0 {
        return if result["ok"] == true || progress || child_success {
            3
        } else {
            1
        };
    }
    if child_failure {
        return if child_partial || child_success { 3 } else { 1 };
    }
    // Some batch adapters wrap each account's actual outcome in result.
    if result["result"].is_object() {
        return result_exit(&result["result"]);
    }
    0
}

fn finish(options: &Options, mut result: Value) -> Result<(Value, u8), Failure> {
    let exit = result_exit(&result);
    redact(&mut result);
    let mut value = response(options.command.as_str(), options.target);
    value["target"] = Value::Null;
    value["operation"] = json!(options.action);
    value["result"] = result;
    value["ok"] = json!(exit == 0);
    if exit != 0 {
        value["error"] = super::protocol::error(
            if exit == 3 {
                "PARTIAL_RESULT"
            } else {
                "OPERATION_FAILED"
            },
            "Operation returned a failure; inspect result for status",
        );
    }
    Ok((value, exit))
}

pub(super) async fn execute(
    options: &Options,
    started: Instant,
    control: &Control,
) -> Result<(Value, u8), Failure> {
    if options.command == Command::Login {
        return login(options, started, control).await;
    }
    let entries = catalog();
    if options.command == Command::Operations {
        let result = if options.action.is_empty() {
            entries
        } else {
            entries
                .get(&options.action)
                .cloned()
                .ok_or_else(|| Failure::usage("Unknown operation; run operations"))?
        };
        let mut value = response("operations", options.target);
        value["target"] = Value::Null;
        value["operation"] = json!(options.action);
        value["result"] = result;
        return Ok((value, 0));
    }
    let entry = entries
        .get(&options.action)
        .ok_or_else(|| Failure::usage("Unknown operation; run operations"))?;
    if workflow(&options.action) {
        return Err(Failure::usage("Use login for authorization, or synchronous switch; this operation requires process-local workflow state"));
    }
    if exports_credentials(&options.action) && options.output_file.is_none() {
        return Err(Failure::usage(
            "Credential exports require --output-file PATH; stdout only reports the file path",
        ));
    }
    let mut input = read_input(options.input_file.as_deref())?;
    validate_input(entry, &input)?;
    if entry["method"] == "POST"
        && !read_only(&options.action)
        && options.input_file.is_none()
        && entry["parameters"]
            .as_object()
            .is_some_and(|p| !p.is_empty())
    {
        return Err(Failure::usage(
            "This operation requires explicit --input-file (use operations OPERATION for inputs)",
        ));
    }
    if options.action == "switch_qoder_account" {
        if input.get("closeRunning").is_none() {
            input["closeRunning"] = json!(false);
        }
        if input.get("relaunch").is_none() {
            input["relaunch"] = json!(false);
        }
    }
    if matches!(
        options.action.as_str(),
        "switch_account"
            | "switch_codebuddy_cn_ide_account"
            | "switch_codebuddy_ide_account"
            | "switch_vscode_ext_account"
            | "switch_jetbrains_account"
    ) {
        use super::target::Target;
        let target = match options.action.as_str() {
            "switch_account" => Target::Workbuddy,
            "switch_vscode_ext_account" => Target::Vscode,
            "switch_jetbrains_account" => Target::Jetbrains,
            _ => Target::CodebuddyIde,
        };
        let id = input["accountId"].as_str().unwrap_or("");
        let account = super::accounts::load_accounts()?
            .into_iter()
            .find(|a| a["id"] == id)
            .ok_or_else(|| Failure::usage("Account not found"))?;
        let variant = account::variant_of(&account);
        let expected = match options.action.as_str() {
            "switch_codebuddy_ide_account" => Some(WbVariant::Ai),
            "switch_codebuddy_cn_ide_account"
            | "switch_vscode_ext_account"
            | "switch_jetbrains_account" => Some(WbVariant::Cn),
            _ => None,
        };
        if expected.is_some_and(|expected| expected != variant) {
            return Err(Failure::usage(
                "Account region does not match this client operation",
            ));
        }
        if input.get("restart").is_none() {
            input["restart"] = json!(false);
        }
        if input["restart"] != true && target.status_for(Some(variant))["running"] == true {
            return Err(Failure {
                code: "CLIENT_RUNNING",
                message: "Close the client first, or explicitly set restart=true".into(),
                exit: 2,
            });
        }
    }
    control.check()?;
    // Reserve the destination before any business action. Never overwrite a
    // backup, and never perform an action only to discover its output is lost.
    let mut output = options
        .output_file
        .as_ref()
        .map(|path| Destination::create(path))
        .transpose()?;
    let result = if options.action == "save_auto_travel_config" {
        let submitted = input.get("config").unwrap_or(&input);
        config::save_travel_config(submitted)
            .map_err(|_| Failure::usage("Invalid travel configuration"))?;
        let saved = config::load_travel_config();
        // The WebUI starts these asynchronously. A one-shot process must drain
        // them before exiting, or an enabled task would be silently cancelled.
        if saved["enabled"] == true {
            let _ = wb_switch_core::modules::travel::run_travel_cycle().await;
            let _ = wb_switch_core::modules::travel::run_travel_claim_cycle().await;
        }
        saved
    } else {
        request(
            entry["method"].as_str().unwrap(),
            entry["path"].as_str().unwrap(),
            input,
        )
        .await?
    };
    if let Some(destination) = output.as_mut() {
        let file = destination.file.as_mut().unwrap();
        let mut result = result;
        if !exports_credentials(&options.action) {
            redact(&mut result);
        }
        serde_json::to_writer_pretty(&mut *file, &result)
            .map_err(|_| Failure::operation("OUTPUT_WRITE_FAILED", "Cannot write output file"))?;
        file.write_all(b"\n")
            .and_then(|_| file.sync_all())
            .map_err(|_| Failure::operation("OUTPUT_WRITE_FAILED", "Cannot finish output file"))?;
        destination.committed = true;
        let exit = result_exit(&result);
        let mut value = response("call", options.target);
        value["target"] = Value::Null;
        value["operation"] = json!(options.action);
        value["outputFile"] = json!(options.output_file);
        value["containsCredentials"] = json!(exports_credentials(&options.action));
        value["ok"] = json!(exit == 0);
        if exit != 0 {
            value["error"] = super::protocol::error(
                if exit == 3 {
                    "PARTIAL_RESULT"
                } else {
                    "OPERATION_FAILED"
                },
                "Operation returned a failure; inspect the output file",
            );
        }
        return Ok((value, exit));
    }
    finish(options, result)
}

async fn login(
    options: &Options,
    started: Instant,
    control: &Control,
) -> Result<(Value, u8), Failure> {
    let variant = options.variant.unwrap_or(WbVariant::Cn);
    let qoder_target = options.target.qoder_target(variant == WbVariant::Ai);
    let start = if options.target.is_qoder() {
        qoder::oauth::start(qoder_target)
            .map_err(|_| Failure::operation("LOGIN_FAILED", "Cannot start Qoder login"))?
    } else {
        oauth::oauth_start(variant).await.map_err(|_| {
            Failure::operation("LOGIN_FAILED", "Cannot start WorkBuddy/CodeBuddy login")
        })?
    };
    // Only public authorization instructions are emitted. Keep the original
    // OAuth session and any pending token exchange in this process.
    progress(
        options,
        "login_required",
        "在官方页面完成登录；此命令会等待授权并保存账号，不切换客户端",
        started,
        Some(json!({"verificationUri":start["verificationUri"],"expiresIn":start["expiresIn"]})),
    );
    let id = start["loginId"]
        .as_str()
        .ok_or_else(|| Failure::operation("LOGIN_FAILED", "Login session was not created"))?;
    loop {
        if let Err(error) = control.check() {
            if options.target.is_qoder() {
                qoder::oauth::cancel(id);
            }
            return Err(error);
        }
        let mut result = if options.target.is_qoder() {
            qoder::oauth::poll(id).await
        } else {
            oauth::oauth_poll(id).await
        };
        if result["done"] == true {
            if result["error"].is_string() {
                return Err(Failure::operation(
                    "LOGIN_FAILED",
                    "Authorization failed or expired; start login again",
                ));
            }
            redact(&mut result);
            let mut value = response("login", options.target);
            value["result"] = result;
            return Ok((value, 0));
        }
        tokio::select! {
            _ = control.cancelled() => {},
            _ = tokio::time::sleep(Duration::from_secs(2)) => {},
        }
    }
}

pub(super) async fn platform_command(
    options: &Options,
    selected: &[Value],
    control: &Control,
) -> Result<Option<(Value, u8)>, Failure> {
    let (path, input) = match options.command {
        Command::TokenStats => ("/api/token-stats", json!({})),
        Command::CreditStats => ("/api/credits/stats", json!({"refresh":options.refresh})),
        Command::Rewards => {
            let mut results = Vec::new();
            for item in selected {
                if control.is_interrupted() {
                    break;
                }
                let mut result = match request(
                    "POST",
                    "/api/checkin",
                    json!({"accountId":item["id"]}),
                )
                .await
                {
                    Ok(result) => result,
                    Err(error) => {
                        json!({"ok":false,"error":super::protocol::error(error.code, &error.message)})
                    }
                };
                redact(&mut result);
                results.push(json!({"accountId":item["id"],"result":result}));
            }
            let failed = results.iter().filter(|v| result_exit(v) != 0).count();
            let mut value = response("rewards", options.target);
            value["total"] = json!(selected.len());
            value["attempted"] = json!(results.len());
            value["accounts"] = json!(results);
            value["ok"] = json!(failed == 0);
            let exit = if failed == 0 {
                0
            } else if failed == selected.len() {
                1
            } else {
                3
            };
            if exit != 0 {
                value["error"] = super::protocol::error(
                    if exit == 3 {
                        "PARTIAL_RESULT"
                    } else {
                        "OPERATION_FAILED"
                    },
                    "Some daily check-ins failed; inspect per-account results",
                );
            }
            return Ok(Some((value, exit)));
        }
        _ => return Ok(None),
    };
    let mut value = response(options.command.as_str(), options.target);
    value["statistics"] = request("GET", path, input).await?;
    let exit = result_exit(&value["statistics"]);
    value["ok"] = json!(exit == 0);
    if exit != 0 {
        value["error"] = super::protocol::error(
            "OPERATION_FAILED",
            "Statistics returned incomplete or failed results",
        );
    }
    value["scope"] = json!("platform");
    Ok(Some((value, exit)))
}

pub(super) async fn desktop_switch(
    options: &Options,
    selected: &Value,
) -> Result<(Value, u8), Failure> {
    use super::target::Target;
    let variant = account::variant_of(selected);
    if matches!(options.target, Target::Vscode | Target::Jetbrains) && variant == WbVariant::Ai {
        return Err(Failure::usage(
            "This plugin target only accepts domestic accounts",
        ));
    }
    let target = options.target;
    let status = tokio::task::spawn_blocking(move || target.status_for(Some(variant)))
        .await
        .map_err(|_| Failure::operation("PROCESS_SCAN_FAILED", "Cannot read client state"))?;
    let mut value = response("switch", options.target);
    value["account"] = account::account_meta(selected);
    value["dryRun"] = json!(options.dry_run);
    value["impact"] = json!({"clientStatus":status,"willCloseRunning":options.close_running,
        "willRelaunch":options.relaunch,"activityKnown":false});
    if options.dry_run {
        value["note"] = json!("Read-only process/status snapshot; credentials and client configuration are not validated");
        return Ok((value, 0));
    }
    if status["running"] == true && !options.close_running {
        return Err(Failure {
            code: "CLIENT_RUNNING",
            message: "Close the client first, or explicitly pass --close-running --relaunch".into(),
            exit: 2,
        });
    }
    let path = match options.target {
        Target::Workbuddy => "/api/switch",
        Target::CodebuddyIde if variant == WbVariant::Ai => "/api/codebuddy-ide/switch",
        Target::CodebuddyIde => "/api/codebuddy-cn-ide/switch",
        Target::Vscode => "/api/vscode-ext/switch",
        Target::Jetbrains => "/api/jetbrains/switch",
        _ => unreachable!(),
    };
    let mut result = request(
        "POST",
        path,
        json!({"accountId":selected["id"],"restart":options.relaunch}),
    )
    .await?;
    redact(&mut result);
    value["result"] = result;
    Ok((value, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outcome_codes_distinguish_skips_failures_and_partial_batches() {
        assert_eq!(
            result_exit(&json!({"ok":false,"results":[{"ok":true},{"ok":false}]})),
            3
        );
        assert_eq!(
            result_exit(&json!({"result":"skipped","reason":"unsupported_variant"})),
            0
        );
        assert_eq!(result_exit(&json!({"result":"error"})), 1);
        assert_eq!(
            result_exit(&json!({"ok":true,"copied":1,"errors":[{"error":"failed"}]})),
            3
        );
        assert_eq!(
            result_exit(
                &json!({"accounts":[{"result":{"result":"already"}},{"result":{"result":"error"}}]})
            ),
            3
        );
        assert_eq!(
            result_exit(&json!({"accounts":[{"result":{"result":"error"}}]})),
            1
        );
    }
    #[test]
    fn ordinary_results_hide_credentials_and_raw_errors_but_keep_preview_tokens() {
        let mut value = json!({"access_token":"secret", "auth":{"refreshToken":"secret"},
            "config":{"password":"secret","apiKey":"secret"},"error":"raw remote secret",
            "previewToken":"approved-plan", "snapshotId":"snapshot", "hasPassword":true,
            "auth_raw":"{\"access_token\":\"secret\"}"});
        redact(&mut value);
        assert!(!value.to_string().contains("secret"));
        assert_eq!(value["previewToken"], "approved-plan");
        assert_eq!(value["snapshotId"], "snapshot");
        assert_eq!(value["hasPassword"], true);
    }
    #[test]
    fn contracts_reject_unknown_missing_and_wrong_region_inputs_before_dispatch() {
        let catalog = catalog();
        assert!(validate_input(
            &catalog["update_account_display"],
            &json!({"accountId":"a"})
        )
        .is_err());
        assert!(validate_input(
            &catalog["update_account_display"],
            &json!({"accountId":"a","patch":{"note":"b"}})
        )
        .is_ok());
        assert!(validate_input(&catalog["get_status"], &json!({"variant":"typo"})).is_err());
        assert!(validate_input(&catalog["get_status"], &json!({"variant":"ai"})).is_ok());
        assert!(validate_input(&catalog["get_qoder_companion"], &json!({"enabled":true})).is_err());
        assert!(validate_input(
            &catalog["switch_account"],
            &json!({"accountId":"a","restart":"false"})
        )
        .is_err());
    }
}
