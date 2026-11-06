//! Real child-process protocol, with synthetic accounts and an isolated home; no remote authentication.
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "wb-qoder-ai-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn write(&self, p: &str, v: &Value) {
        let p = self.0.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, v.to_string()).unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_wb-switch"))
            .args(args)
            .env("WB_SWITCH_HOME", &self.0)
            .output()
            .unwrap()
    }
    fn bundle(&self, id: &str, target: &str, key: &str, at: i64) {
        self.write(&format!(".wb-switch/qoder/bundles/{id}/bundle.json"),&json!({"id":id,"target":target,"label":"测试📷","identityKey":key,"capturedAt":at,"files":[]}));
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn result(o: &Output) -> Value {
    let value: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| {
        panic!(
            "Invalid stdout JSON: {e}; {}",
            String::from_utf8_lossy(&o.stdout)
        )
    });
    assert_eq!(value["exitCode"].as_i64(), o.status.code().map(i64::from));
    assert_eq!(value["schemaVersion"], 1);
    assert!(!String::from_utf8_lossy(&o.stdout).contains("NEVER_PRINT_SECRET"));
    assert!(!String::from_utf8_lossy(&o.stderr).contains("NEVER_PRINT_SECRET"));
    for line in String::from_utf8_lossy(&o.stderr).lines() {
        let progress: Value = serde_json::from_str(line).unwrap();
        assert_eq!(progress["type"], "progress");
    }
    value
}

#[test]
fn qoder_cli_switch_uses_its_own_lock_backup_and_credentials() {
    use wb_switch_core::modules::qoder::{store, QoderRoots, QoderTarget};
    let s = Sandbox::new();
    let root = s.0.join(".wb-switch/qoder");
    let roots = QoderRoots {
        home: s.0.clone(),
        app_data: s.0.join("isolated-appdata"),
    };
    s.write(
        ".qoder-cn/.auth/user",
        &json!({"uid":"qa-old","token":"NEVER_PRINT_SECRET_OLD"}),
    );
    let old_bytes = std::fs::read(s.0.join(".qoder-cn/.auth/user")).unwrap();
    let old = store::capture(&root, &roots, QoderTarget::Cli, Some("Original fixture")).unwrap();
    s.write(
        ".qoder-cn/.auth/user",
        &json!({"uid":"qa-next","token":"NEVER_PRINT_SECRET_NEXT"}),
    );
    let next_bytes = std::fs::read(s.0.join(".qoder-cn/.auth/user")).unwrap();
    let next = store::capture(&root, &roots, QoderTarget::Cli, Some("Next fixture")).unwrap();
    std::fs::write(s.0.join(".qoder-cn/.auth/user"), &old_bytes).unwrap();
    s.write(".qoder/.auth/user", &json!({"uid":"global-untouched"}));
    let untouched = std::fs::read(s.0.join(".qoder/.auth/user")).unwrap();
    std::fs::write(
        s.0.join(".wb-switch/accounts.json"),
        b"invalid shared library NEVER_PRINT_SECRET",
    )
    .unwrap();
    let preview = result(&s.run(&[
        "switch",
        &next.id,
        "--target=qoder-cli",
        "--dry-run",
        "--quiet",
    ]));
    assert_eq!(preview["dryRun"], true);
    assert_eq!(
        std::fs::read(s.0.join(".qoder-cn/.auth/user")).unwrap(),
        old_bytes
    );
    assert!(!root.join("backups").exists());
    // Never close a user's real Qoder process even when these tests run inside Qoder.
    if preview["impact"]["idleConfirmed"] != true {
        return;
    }
    let o = s.run(&["switch", &next.id, "--target=qoder-cli", "--quiet"]);
    let v = result(&o);
    if v["error"]["code"] == "CLIENT_RUNNING" || v["error"]["code"] == "PROCESS_SCAN_FAILED" {
        assert_eq!(
            std::fs::read(s.0.join(".qoder-cn/.auth/user")).unwrap(),
            old_bytes
        );
        return;
    }
    assert!(o.status.success(), "{v}");
    assert_eq!(
        std::fs::read(s.0.join(".qoder-cn/.auth/user")).unwrap(),
        next_bytes
    );
    assert!(root.join("backups").is_dir());
    assert_eq!(
        std::fs::read(s.0.join(".qoder/.auth/user")).unwrap(),
        untouched
    );
    let status = result(&s.run(&["status", "--target=qoder-cli", "--variant=cn"]));
    assert_eq!(
        status["clients"][0]["activeAccountId"],
        format!("qoder-cn:{}", next.identity_key.unwrap())
    );
    let o = s.run(&["switch", &old.id, "--target=qoder-cli", "--quiet"]);
    assert!(o.status.success(), "{}", result(&o));
    assert_eq!(
        std::fs::read(s.0.join(".qoder-cn/.auth/user")).unwrap(),
        old_bytes
    );
}
#[test]
fn qoder_discovery_regions_dedup_and_failures_use_the_real_protocol() {
    let s = Sandbox::new();
    s.write(
        ".wb-switch/accounts.json",
        &json!([{"id":"workbuddy-only","access_token":"NEVER_PRINT_SECRET"}]),
    );
    s.bundle("old", "cli", "same", 1);
    s.bundle("new", "cli", "same", 3);
    s.bundle("desktop", "desktop", "same", 2);
    s.bundle("global", "global_cli", "same", 1);
    let help = result(&s.run(&["help", "--json"]));
    assert!(help["supportedTargets"]
        .as_array()
        .unwrap()
        .contains(&json!("qoder-cli")));
    assert_eq!(help["capabilities"]["qoder-cli"]["modelLimits"], false);
    let o = s.run(&["accounts", "--target=qoder-cli", "--ascii-json"]);
    let all = result(&o);
    assert!(o.stdout.is_ascii());
    assert_eq!(all["accounts"].as_array().unwrap().len(), 2);
    let o = s.run(&["accounts", "--target=qoder-cli", "--variant=cn"]);
    let cn = result(&o);
    assert_eq!(cn["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(cn["accounts"][0]["bundleId"], "new");
    assert_eq!(cn["accounts"][0]["id"], "qoder-cn:same");
    assert!(cn["accounts"][0].get("files").is_none());
    assert_eq!(
        result(&s.run(&["status", "--target=qoder-cli", "--variant=ai"]))["clients"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let missing = s.run(&[
        "switch",
        "qoder-cn:same",
        "--variant=ai",
        "--target=qoder-cli",
        "--dry-run",
        "--quiet",
    ]);
    assert_eq!(result(&missing)["error"]["code"], "ACCOUNT_NOT_FOUND");
    assert_eq!(missing.status.code(), Some(2));
    let dry = s.run(&[
        "switch",
        "qoder-cn:same",
        "--target=qoder-cli",
        "--dry-run",
        "--quiet",
    ]);
    assert_eq!(result(&dry)["dryRun"], true);
    assert_eq!(result(&dry)["willCloseRunningClient"], false);
    assert!(!s.0.join(".qoder-cn").exists());
    for command in ["credits", "inspect"] {
        let o = s.run(&[
            command,
            "--target=qoder-cli",
            "--variant=cn",
            "--summary",
            "--quiet",
        ]);
        let v = result(&o);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(v["summary"]["accountCount"], 1);
        assert_eq!(v["summary"]["complete"], false);
        if command == "inspect" {
            assert_eq!(v["availabilityKnown"], false);
            assert_eq!(v["limitsSupported"], false);
            assert_eq!(v["accounts"][0]["credits"]["ok"], false);
        }
    }
    let limits = s.run(&["limits", "--target=qoder-cli"]);
    assert_eq!(result(&limits)["error"]["code"], "UNSUPPORTED_CAPABILITY");
    assert_eq!(limits.status.code(), Some(2));
    let rewards = s.run(&[
        "rewards",
        "--all",
        "--target=qoder-cli",
        "--variant=cn",
        "--quiet",
    ]);
    assert_eq!(result(&rewards)["summary"]["failedAccounts"], 1);
    assert_eq!(rewards.status.code(), Some(1));
    assert!(!s.0.join(".codebuddy").exists());
    assert!(!s.0.join(".qoder-cn").exists());
    std::fs::write(
        s.0.join(".wb-switch/qoder/bundles/new/bundle.json"),
        b"broken NEVER_PRINT_SECRET",
    )
    .unwrap();
    assert_eq!(
        result(&s.run(&["accounts", "--target=qoder-cli"]))["error"]["code"],
        "ACCOUNT_STORE_INVALID"
    );
}
#[test]
fn qoder_automation_patches_only_selected_region_and_does_not_start_a_daemon() {
    let s = Sandbox::new();
    s.write("patch.json", &json!({"dailyTime":"10:45","expiryDays":9}));
    let patch = s.0.join("patch.json");
    let patch = patch.to_str().unwrap();
    let o = s.run(&[
        "automation",
        "set",
        "--target=qoder-cli",
        "--variant=cn",
        "--settings-file",
        patch,
    ]);
    assert!(o.status.success(), "{}", result(&o));
    let config = result(&s.run(&["automation", "get", "--target=qoder-cli"]));
    assert_eq!(config["result"]["settings"]["cn"]["dailyTime"], "10:45");
    assert_eq!(config["result"]["settings"]["ai"]["dailyTime"], "09:00");
    let saved = std::fs::read(s.0.join(".wb-switch/qoder/automation-config.json")).unwrap();
    s.write(
        "patch.json",
        &json!({"dailyEnabled":true,"unknownField":"NEVER_PRINT_SECRET"}),
    );
    assert!(!s
        .run(&[
            "automation",
            "set",
            "--target=qoder-cli",
            "--variant=cn",
            "--settings-file",
            patch
        ])
        .status
        .success());
    assert_eq!(
        saved,
        std::fs::read(s.0.join(".wb-switch/qoder/automation-config.json")).unwrap()
    );
    s.write("patch.json", &json!({"alertsEnabled":true}));
    assert!(s
        .run(&[
            "automation",
            "set",
            "--target=qoder-cli",
            "--variant=ai",
            "--settings-file",
            patch
        ])
        .status
        .success());
    let o = s.run(&["automation", "run", "--target=qoder-cli", "--variant=cn"]);
    assert!(o.status.success(), "{}", result(&o));
    let state: Value = serde_json::from_slice(
        &std::fs::read(s.0.join(".wb-switch/qoder/automation-state.json")).unwrap(),
    )
    .unwrap();
    assert!(state["checked"].get("ai").is_none());
    let cn = result(&s.run(&["automation", "get", "--target=qoder-cli", "--variant=cn"]));
    assert!(cn["result"]["settings"].get("ai").is_none());
    assert!(!s.0.join(".qoder-cn").exists());
}
#[test]
fn qoder_stats_do_not_invent_tokens_or_mix_regions_and_lock_contention_is_explicit() {
    let s = Sandbox::new();
    let at = chrono::Utc::now().to_rfc3339();
    let record = json!({"type":"assistant","timestamp":at,"sessionId":"synthetic-session","cwd":"synthetic-project","message":{"id":"request","model":"test-model","content":"NEVER_PRINT_SECRET","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"credits":2.5,"billable":true}}});
    s.write(".qoder-cn/projects/fixture/session.jsonl", &record);
    s.write(".qoder/projects/fixture/session.jsonl", &record);
    let o = s.run(&["token-stats", "--target=qoder-cli", "--variant=cn"]);
    let v = result(&o);
    assert!(o.status.success());
    assert_eq!(v["sources"].as_array().unwrap().len(), 1);
    assert_eq!(v["sources"][0]["unavailableTokenRecords"], 1);
    assert_eq!(v["accountAttributionAvailable"], false);
    let o = s.run(&["credit-stats", "--target=qoder-cli", "--variant=ai"]);
    let v = result(&o);
    assert!(o.status.success());
    assert_eq!(v["localCredits"].as_array().unwrap().len(), 1);
    assert_eq!(v["localCredits"][0]["summary"]["credits"], 2.5);
    assert!(v.get("refresh").is_none());
    let guard = wb_switch_core::modules::state_lock::StateLock::try_at(
        &s.0.join(".wb-switch/qoder/switch.lock"),
    )
    .unwrap();
    let o = s.run(&["accounts", "--target=qoder-cli", "--wait=0", "--quiet"]);
    assert_eq!(result(&o)["error"]["code"], "AUTH_STATE_BUSY");
    assert_eq!(o.status.code(), Some(1));
    drop(guard);
    let o = s.run(&[
        "credit-stats",
        "--target=qoder-cli",
        "--variant=cn",
        "--refresh",
        "--quiet",
    ]);
    assert!(o.status.success(), "{}", result(&o));
    assert_eq!(result(&o)["refresh"]["summary"]["accountCount"], 0);
}
