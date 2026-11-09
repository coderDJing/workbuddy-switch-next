//! Real binary coverage with fixture-only account data. Never switches a live client.
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
        let root = std::env::temp_dir().join(format!(
            "wb-operations-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join(".wb-switch")).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, value: Value) -> String {
        let dest = self.0.join(path);
        std::fs::write(&dest, value.to_string()).unwrap();
        dest.to_string_lossy().into_owned()
    }
    fn run(&self, args: &[&str]) -> (Output, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_wb-switch"))
            .args(args)
            .env("WB_SWITCH_HOME", &self.0)
            .output()
            .unwrap();
        let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "stdout: {} stderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output, value)
    }
    fn accounts(&self) {
        self.write(".wb-switch/accounts.json", json!([
            {"id":"fixture:中文 &+","nickname":"Before","access_token":"sensitive-fixture-token","refresh_token":"sensitive-fixture-refresh"},
            {"id":"ai-fixture","variant":"ai","nickname":"International"}
        ]));
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn discovery_covers_all_shared_operations_without_redacting_contracts() {
    let s = Sandbox::new();
    let (output, value) = s.run(&["operations"]);
    assert!(output.status.success());
    let catalog: Value =
        serde_json::from_str(include_str!("../../../src/lib/operations.json")).unwrap();
    assert_eq!(
        value["result"].as_object().unwrap().len(),
        catalog.as_object().unwrap().len()
    );
    assert!(value["result"]["refresh_account_token"].is_object());
    assert_eq!(
        value["result"]["export_all_accounts"]["requiresOutputFile"],
        true
    );
    assert_eq!(value["result"]["qoder_oauth_start"]["oneShot"], false);
    assert!(output.stderr.is_empty());
    let (_, help) = s.run(&["help", "--json"]);
    for target in ["workbuddy", "codebuddy-ide", "vscode", "jetbrains"] {
        assert!(help["supportedTargets"]
            .as_array()
            .unwrap()
            .contains(&json!(target)));
        assert!(help["capabilities"][target]["commands"]
            .as_array()
            .unwrap()
            .contains(&json!("switch")));
    }
}

#[test]
fn named_account_edits_and_encoded_get_queries_reuse_backend() {
    let s = Sandbox::new();
    s.accounts();
    let input = s.write(
        "edit.json",
        json!({"accountId":"fixture:中文 &+", "patch":{"note":"changed"}}),
    );
    let (output, value) = s.run(&["call", "update_account_display", "--input-file", &input]);
    assert!(output.status.success(), "{value}");
    let (output, value) = s.run(&["call", "get_accounts"]);
    assert!(output.status.success());
    assert_eq!(value["result"]["accounts"][0]["note"], "changed");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("sensitive-fixture"));
    let input = s.write(
        "query.json",
        json!({"accountId":"fixture:中文 &+", "client":"workbuddy"}),
    );
    let (output, value) = s.run(&["call", "list_account_sessions", "--input-file", &input]);
    assert!(output.status.success(), "{value}");
    let input = s.write("delete.json", json!({"accountId":"ai-fixture"}));
    let (output, value) = s.run(&["call", "delete_account", "--input-file", &input]);
    assert!(output.status.success(), "{value}");
    let (_, value) = s.run(&["accounts"]);
    assert_eq!(value["accounts"].as_array().unwrap().len(), 1);
}

#[test]
fn exports_require_explicit_new_file_and_never_print_credentials() {
    let s = Sandbox::new();
    s.accounts();
    let (output, _) = s.run(&["call", "export_all_accounts"]);
    assert_eq!(output.status.code(), Some(2));
    let dest = s.0.join("backup.json").to_string_lossy().into_owned();
    let (output, value) = s.run(&[
        "call",
        "export_all_accounts",
        "--input-file",
        &s.write("empty.json", json!({})),
        "--output-file",
        &dest,
    ]);
    assert!(output.status.success(), "{value}");
    assert_eq!(value["containsCredentials"], true);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("sensitive-fixture"));
    let original = std::fs::read(&dest).unwrap();
    assert!(String::from_utf8_lossy(&original).contains("sensitive-fixture-token"));
    let (output, _) = s.run(&[
        "call",
        "export_all_accounts",
        "--input-file",
        &s.0.join("empty.json").to_string_lossy(),
        "--output-file",
        &dest,
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(std::fs::read(&dest).unwrap(), original);
}

#[test]
fn invalid_workflows_inputs_and_backend_errors_fail_without_secret_output_or_partial_file() {
    let s = Sandbox::new();
    for command in ["oauth_start", "qoder_oauth_poll", "not_an_operation"] {
        let (output, _) = s.run(&["call", command]);
        assert_eq!(output.status.code(), Some(2));
    }
    let input = s.write("bad.json", json!({"accountId":"sensitive-fixture-token"}));
    let dest = s.0.join("failed.json").to_string_lossy().into_owned();
    let (output, _) = s.run(&[
        "call",
        "delete_account",
        "--input-file",
        &input,
        "--output-file",
        &dest,
    ]);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("sensitive-fixture-token"));
    assert!(!std::path::Path::new(&dest).exists());
    let (output, _) = s.run(&["call", "delete_account"]);
    assert_eq!(output.status.code(), Some(2));
    let input = s.write("array.json", json!([]));
    let (output, _) = s.run(&["call", "get_accounts", "--input-file", &input]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn workbuddy_dry_run_never_writes_auth_or_switches_clients_and_plugin_filters_regions() {
    let s = Sandbox::new();
    s.accounts();
    let before = std::fs::read(s.0.join(".wb-switch/accounts.json")).unwrap();
    let (output, value) = s.run(&[
        "switch",
        "fixture:中文 &+",
        "--target",
        "workbuddy",
        "--dry-run",
    ]);
    assert!(output.status.success(), "{value}");
    assert_eq!(value["dryRun"], true);
    assert_eq!(
        std::fs::read(s.0.join(".wb-switch/accounts.json")).unwrap(),
        before
    );
    assert!(!s.0.join(".workbuddy").exists());
    let (output, value) = s.run(&["accounts", "--target", "vscode"]);
    assert!(output.status.success());
    assert_eq!(value["accounts"].as_array().unwrap().len(), 1);
    assert!(value.get("activeCliAccountId").is_none());
}

#[test]
fn named_switch_rejects_cross_region_accounts_before_touching_a_client() {
    let s = Sandbox::new();
    s.accounts();
    for (operation, id) in [
        ("switch_codebuddy_ide_account", "fixture:中文 &+"),
        ("switch_codebuddy_cn_ide_account", "ai-fixture"),
        ("switch_vscode_ext_account", "ai-fixture"),
        ("switch_jetbrains_account", "ai-fixture"),
    ] {
        let input = s.write("switch.json", json!({"accountId":id,"restart":true}));
        let (output, value) = s.run(&["call", operation, "--input-file", &input]);
        assert_eq!(output.status.code(), Some(2), "{value}");
        assert!(value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("region"));
    }
}
