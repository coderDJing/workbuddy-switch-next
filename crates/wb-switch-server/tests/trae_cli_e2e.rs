//! Actual CLI routing with isolated homes; never switches or launches a live app.
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    process::{Command, Output},
};
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wb-trae-cli-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".wb-switch")).unwrap();
        Self(root)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_wb-switch"))
            .args(args)
            .env("WB_SWITCH_HOME", &self.0)
            .output()
            .unwrap()
    }
    fn write(&self, path: &str, value: Value) {
        let p = self.0.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, value.to_string()).unwrap();
    }
    fn call(&self, input: Value) -> Output {
        self.write("input.json", input);
        self.run(&[
            "call",
            "trae_action",
            "--input-file",
            self.0.join("input.json").to_str().unwrap(),
        ])
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn body(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn trae_never_uses_workbuddy_store_login_or_statistics() {
    let s = Sandbox::new();
    s.write(
        ".wb-switch/accounts.json",
        json!([{"id":"wb-private","uid":"wb-user","access_token":"wb-secret"}]),
    );
    for target in ["trae-desktop"] {
        let result = s.run(&["accounts", "--target", target, "--variant", "all"]);
        assert!(result.status.success());
        let v = body(&result);
        assert_eq!(v["target"], target);
        assert_eq!(v["accounts"], json!([]));
        assert!(!String::from_utf8_lossy(&result.stdout).contains("wb-private"));
        for command in ["login", "token-stats", "limits"] {
            let result = s.run(&[command, "--target", target]);
            assert_eq!(result.status.code(), Some(2));
            assert_eq!(body(&result)["error"]["code"], "UNSUPPORTED_CAPABILITY");
        }
    }
    assert!(!s.0.join(".wb-switch/trae/accounts.json").exists());
    let result = s.run(&["help", "--json"]);
    assert!(body(&result)["supportedTargets"]
        .as_array()
        .unwrap()
        .contains(&json!("trae-desktop")));
    assert!(body(&result)["capabilities"]["trae-desktop"]["commands"]
        .as_array()
        .unwrap()
        .contains(&json!("switch")));
    let result = s.call(json!({"action":"capture","input":{"unexpected":true}}));
    assert!(!result.status.success());
    let result = s.call(json!({"action":"capture","input":{"client":"legacy_work"}}));
    assert!(!result.status.success());
    assert!(!s.0.join(".wb-switch/trae/accounts.json").exists());
}
#[test]
fn trae_statistics_and_automation_use_the_isolated_platform_store() {
    let s =
        Sandbox(std::env::temp_dir().join(format!("wb-trae-management-{}", std::process::id())));
    s.write(
        "patch.json",
        json!({"refreshEnabled":true,"intervalMinutes":15}),
    );
    let result = s.run(&[
        "credit-stats",
        "--target",
        "trae-desktop",
        "--variant",
        "cn",
    ]);
    assert!(result.status.success());
    assert_eq!(body(&result)["regions"][0]["accounts"], json!([]));
    let result = s.run(&[
        "automation",
        "set",
        "--target",
        "trae-desktop",
        "--variant",
        "cn",
        "--settings-file",
        s.0.join("patch.json").to_str().unwrap(),
    ]);
    assert!(result.status.success());
    assert_eq!(
        body(&result)["automation"]["settings"]["cn"]["refreshEnabled"],
        true
    );
    assert_eq!(
        body(&result)["automation"]["settings"]["ai"]["refreshEnabled"],
        false
    );
    let result = s.run(&[
        "automation",
        "run",
        "--target",
        "trae-desktop",
        "--variant",
        "cn",
    ]);
    assert!(result.status.success());
    let result = s.run(&[
        "automation",
        "get",
        "--target",
        "trae-desktop",
        "--variant",
        "cn",
    ]);
    assert!(result.status.success());
    assert!(body(&result)["automation"]["settings"].get("ai").is_none());
}
#[cfg(windows)]
#[test]
fn explicit_capture_is_encrypted_filtered_and_dry_run_preserves_file() {
    // Separate root so this test can run alongside the empty-library test.
    let s = Sandbox(std::env::temp_dir().join(format!("wb-trae-capture-{}", std::process::id())));
    let storage = "AppData/Roaming/Trae CN/User/globalStorage/storage.json";
    s.write(storage,json!({"iCubeAuthInfo://icube.cloudide":{"userId":"fixture-user","token":"fixture-secret","account":{"username":"Fixture"}},"project.history":["keep"]}));
    let result = s.call(json!({"action":"capture","input":{"variant":"cn","client":"desktop"}}));
    assert!(result.status.success(), "capture failed");
    let store = std::fs::read_to_string(s.0.join(".wb-switch/trae/accounts.json")).unwrap();
    assert!(!store.contains("fixture-secret"));
    let result = s.run(&["accounts", "--target", "trae-desktop", "--variant", "cn"]);
    assert!(result.status.success());
    let v = body(&result);
    assert_eq!(v["accountCount"], 1);
    let id = v["accounts"][0]["id"].as_str().unwrap();
    assert_eq!(v["accounts"][0]["activeForTarget"], true);
    assert!(!String::from_utf8_lossy(&result.stdout).contains("fixture-secret"));
    let before = std::fs::read(s.0.join(storage)).unwrap();
    let result = s.run(&["switch", id, "--target", "trae-desktop", "--dry-run"]);
    assert!(result.status.success());
    assert_eq!(body(&result)["dryRun"], true);
    assert_eq!(std::fs::read(s.0.join(storage)).unwrap(), before);
    let result = s.run(&["accounts", "--target", "trae-work"]);
    assert_eq!(result.status.code(), Some(2));
    let result = s.run(&["accounts", "--target", "trae-desktop", "--variant", "ai"]);
    assert_eq!(body(&result)["accountCount"], 0);
    let result = s.call(json!({"action":"rename","input":{"accountId":id,"name":"Renamed"}}));
    assert!(result.status.success());
    let result = s.call(json!({"action":"delete","input":{"accountId":id}}));
    assert!(result.status.success());
    assert_eq!(std::fs::read(s.0.join(storage)).unwrap(), before);
}
