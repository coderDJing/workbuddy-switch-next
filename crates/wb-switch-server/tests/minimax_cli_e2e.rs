//! Exercise the CLI and its in-process WebUI adapter in an isolated home.
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    process::{Command, Output},
};
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "wb-minimax-cli-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
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
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, value.to_string()).unwrap();
    }
    fn call(&self, action: &str, input: Value) -> Output {
        self.write("input.json", json!({"action":action,"input":input}));
        self.run(&[
            "call",
            "minimax_action",
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
fn body(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&o.stderr)))
}

#[test]
fn discovery_statistics_and_settings_are_independent_of_other_platforms() {
    let s = Sandbox::new();
    s.write(
        ".wb-switch/accounts.json",
        json!([{"id":"other-platform","access_token":"secret"}]),
    );
    let help = s.run(&["help", "--json"]);
    let commands = &body(&help)["capabilities"]["minimax-desktop"]["commands"];
    for command in ["rewards", "automation", "token-stats", "credit-stats"] {
        assert!(commands.as_array().unwrap().contains(&json!(command)));
    }
    for command in ["token-stats", "credit-stats", "accounts"] {
        let r = s.run(&[command, "--target", "minimax-desktop"]);
        assert!(r.status.success(), "{}", body(&r));
        assert!(!String::from_utf8_lossy(&r.stdout).contains("secret"));
    }
    let r = s.run(&["token-stats", "--target", "minimax-desktop"]);
    assert_eq!(body(&r)["statistics"]["available"], false);
    assert_eq!(body(&r)["statistics"]["accountAttributionAvailable"], false);
    s.write(
        "settings.json",
        json!({"refreshEnabled":true,"intervalMinutes":15}),
    );
    let r = s.run(&[
        "automation",
        "set",
        "--target",
        "minimax-desktop",
        "--variant",
        "ai",
        "--settings-file",
        s.0.join("settings.json").to_str().unwrap(),
    ]);
    assert!(r.status.success(), "{}", body(&r));
    assert_eq!(
        body(&r)["automation"]["settings"]["en"]["refreshEnabled"],
        true
    );
    assert_eq!(
        body(&r)["automation"]["settings"]["cn"]["refreshEnabled"],
        false
    );
    let r = s.run(&[
        "automation",
        "get",
        "--target",
        "minimax-desktop",
        "--variant",
        "ai",
    ]);
    assert!(r.status.success());
    assert!(body(&r)["automation"]["settings"].get("cn").is_none());
    let r = s.run(&[
        "automation",
        "run",
        "--target",
        "minimax-desktop",
        "--variant",
        "ai",
    ]);
    assert!(r.status.success(), "{}", body(&r));
    let r = s.call("get_settings", json!({"unexpected":true}));
    assert!(!r.status.success());
}

#[test]
fn rewards_require_explicit_selection_and_unknown_ids_are_not_successful() {
    let s = Sandbox::new();
    let r = s.run(&["rewards", "--target", "minimax-desktop"]);
    assert_eq!(r.status.code(), Some(2));
    let r = s.run(&["rewards", "missing", "--target", "minimax-desktop"]);
    assert_eq!(r.status.code(), Some(2));
    assert_eq!(body(&r)["error"]["code"], "ACCOUNT_NOT_FOUND");
    let r = s.run(&["rewards", "--all", "--target", "minimax-desktop"]);
    assert!(r.status.success());
    assert_eq!(body(&r)["rewards"], json!([]));
    let r = s.run(&["operations", "minimax_action"]);
    assert!(r.status.success());
    let text = String::from_utf8_lossy(&r.stdout);
    assert!(
        text.contains("backup_import") && text.contains("checkin") && text.contains("token_stats")
    );
}

#[test]
fn token_statistics_read_canonical_transcripts_without_returning_messages() {
    let s = Sandbox::new();
    let record = json!({"message_id":"message-1","message":{"role":"assistant","timestamp":1735689600000i64,"model":"MiniMax","content":"private-dialogue","usage":{"input":12,"output":3,"cacheRead":4,"cacheWrite":1,"totalTokens":20}}});
    s.write(
        ".minimax/v2/sessions/2025/01/01/session/messages.jsonl",
        record,
    );
    let r = s.run(&["token-stats", "--target", "minimax-desktop"]);
    assert!(r.status.success(), "{}", body(&r));
    assert_eq!(body(&r)["statistics"]["totals"]["total"], 20);
    assert!(!String::from_utf8_lossy(&r.stdout).contains("private-dialogue"));
    let r = s.call("token_stats", json!({}));
    assert!(r.status.success(), "{}", body(&r));
}
