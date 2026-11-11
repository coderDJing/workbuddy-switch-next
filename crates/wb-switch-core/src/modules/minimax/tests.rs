//! MiniMax 接入的离线测试：临时目录 + 假进程 + 明文「加密」，不碰真实客户端。
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};

use super::paths::{Paths, Region};
use super::proc::Processes;
use super::protocol::{record_key, Payload};
use super::vault::Vault;
use super::Engine;

struct PlainVault;
impl Vault for PlainVault {
    fn seal(&self, plain: &[u8]) -> Result<String, String> {
        Ok(STANDARD.encode(plain))
    }
    fn open(&self, sealed: &str) -> Result<Vec<u8>, String> {
        STANDARD.decode(sealed).map_err(|_| "密文损坏".to_string())
    }
}

#[derive(Default)]
struct Counters {
    closes: AtomicUsize,
    launches: AtomicUsize,
}

#[derive(Default)]
struct FakeProcesses {
    running: AtomicBool,
    cli: AtomicBool,
    counters: Arc<Counters>,
}
impl Processes for FakeProcesses {
    fn running(&self, _region: Region) -> Result<bool, String> {
        Ok(self.running.load(Ordering::SeqCst))
    }
    fn cli_running(&self) -> Result<bool, String> {
        Ok(self.cli.load(Ordering::SeqCst))
    }
    fn running_exe(&self) -> Option<PathBuf> {
        None
    }
    fn close(&self, _region: Region, _timeout: Duration) -> Result<(), String> {
        self.counters.closes.fetch_add(1, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
    fn launch(&self, _paths: &Paths, _region: Region) -> Result<(), String> {
        self.counters.launches.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn temp_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wb-switch-minimax-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn engine(root: &Path) -> Engine {
    Engine {
        paths: Paths {
            home: root.to_path_buf(),
            app_data: root.join("AppData/Roaming"),
            store: root.join(".wb-switch/minimax"),
        },
        vault: Box::new(PlainVault),
        processes: Box::new(FakeProcesses::default()),
    }
}

fn engine_with(root: &Path, processes: FakeProcesses) -> Engine {
    Engine {
        paths: Paths {
            home: root.to_path_buf(),
            app_data: root.join("AppData/Roaming"),
            store: root.join(".wb-switch/minimax"),
        },
        vault: Box::new(PlainVault),
        processes: Box::new(processes),
    }
}

fn live_payload(region: Region, epoch: &str, generation: i64) -> Payload {
    let key = format!("{}\u{0}account", region.namespace_service());
    let mut records = serde_json::Map::new();
    records.insert(
        key,
        json!({
            "schemaVersion": 1,
            "accessToken": format!("mmoat_{epoch}"),
            "refreshToken": format!("mmort_{epoch}"),
            "tokenType": "Bearer",
            "clientId": "mcode-public",
            "scopes": ["agent.default"],
            "audience": "agent-backend",
            "expiresAtMs": 1_900_000_000_000i64,
            "generation": generation,
            "loginEpoch": epoch,
        }),
    );
    Payload {
        auth: json!({"schemaVersion": 1, "records": Value::Object(records)}),
        state: json!({
            "schemaVersion": 2,
            "status": "authenticated",
            "storeKind": "file",
            "clientId": "mcode-public",
            "scopes": ["agent.default"],
            "audience": "agent-backend",
            "buildEnv": "prod",
            "region": region.text(),
            "expiresAtMs": 1_900_000_000_000i64,
            "generation": generation,
        }),
    }
}

fn write_live(root: &Path, region: Region, payload: &Payload) {
    let dir = root
        .join(".minimax/auth/prod")
        .join(region.text())
        .join("mcode-public");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("auth.json"), payload.auth_bytes().unwrap()).unwrap();
    std::fs::write(dir.join("auth-state.json"), payload.state_bytes().unwrap()).unwrap();
}

fn read_live(root: &Path, region: Region) -> (Value, Value) {
    let dir = root
        .join(".minimax/auth/prod")
        .join(region.text())
        .join("mcode-public");
    (
        serde_json::from_slice(&std::fs::read(dir.join("auth.json")).unwrap()).unwrap(),
        serde_json::from_slice(&std::fs::read(dir.join("auth-state.json")).unwrap()).unwrap(),
    )
}

fn write_identity(root: &Path, name: &str) {
    write_identity_uid(root, name, "576404468350857218");
}

fn write_identity_uid(root: &Path, name: &str, real_uid: &str) {
    let dir = root.join("AppData/Roaming/MiniMax");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("minimax-agent-cn-config.json"),
        serde_json::to_vec(&json!({
            "user": {},
            "sharedUser": {
                "userID": "uid-1",
                "realUserID": real_uid,
                "userName": "MiniMax395701",
                "subUserName": name,
                "deviceID": "dev-1"
            }
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn capture_reads_live_login_and_marks_it_active() {
    let root = temp_root();
    write_identity(&root, "coder");
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-a", 1));
    let engine = engine(&root);

    let captured = engine.capture(Region::Cn).unwrap();
    assert_eq!(captured["name"], "coder");
    assert_eq!(captured["active"], true);
    assert_eq!(captured["needsLogin"], false);

    let status = engine.status().unwrap();
    assert_eq!(status["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(status["sharedLogin"], true);
    let cn = &status["clients"][0];
    assert_eq!(cn["configured"], true);
    assert_eq!(cn["activeAccountId"], captured["id"]);
    assert_eq!(cn["identity"]["uid"], "576404468350857218");
    // 账号包里不能出现明文令牌。
    let store = std::fs::read_to_string(root.join(".wb-switch/minimax/accounts.json")).unwrap();
    assert!(!store.contains("mmoat_"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn recapturing_the_same_login_updates_one_account() {
    let root = temp_root();
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-a", 3));
    let engine = engine(&root);
    let first = engine.capture(Region::Cn).unwrap();
    let second = engine.capture(Region::Cn).unwrap();
    assert_eq!(first["id"], second["id"]);
    assert_eq!(
        engine.status().unwrap()["accounts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn recapture_after_relogin_replaces_the_same_user() {
    let root = temp_root();
    write_identity(&root, "coder");
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-a", 1));
    let engine = engine(&root);
    let first = engine.capture(Region::Cn).unwrap();

    // 客户端重新登录：`loginEpoch` 变了，平台用户 id 不变。
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-b", 1));
    let second = engine.capture(Region::Cn).unwrap();

    assert_eq!(first["id"], second["id"]);
    let status = engine.status().unwrap();
    let accounts = status["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["active"], true);
    // 旧登录的凭据已被替换掉，账号指纹回到平台用户 id。
    assert_eq!(accounts[0]["uid"], "576404468350857218");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn switch_writes_both_files_with_new_generation_and_backs_up() {
    let root = temp_root();
    let engine = engine(&root);
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-a", 4));
    let a = engine.capture(Region::Cn).unwrap();
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-b", 6));
    engine.capture(Region::Cn).unwrap();

    let result = engine
        .switch(Region::Cn, a["id"].as_str().unwrap(), false, false, false)
        .unwrap();
    assert_eq!(result["switched"], true);
    assert_eq!(result["label"], a["name"]);

    let (auth_value, state_value) = read_live(&root, Region::Cn);
    assert!(
        std::fs::read_to_string(root.join(".minimax/auth/prod/cn/mcode-public/auth.json"))
            .unwrap()
            .contains("mmoat_epoch-a")
    );
    assert_eq!(state_value["region"], "cn");
    assert_eq!(state_value["status"], "authenticated");
    assert!(state_value["generation"].as_i64().unwrap() > 6);
    let key = record_key(&auth_value, Region::Cn).unwrap();
    assert_eq!(
        auth_value["records"][key.as_str()]["generation"],
        state_value["generation"]
    );
    assert!(state_value.get("refresh").is_none());
    // 现场切换前已自动保存，切回原账号不会丢。
    assert_eq!(
        engine.status().unwrap()["accounts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let backups = std::fs::read_dir(root.join(".wb-switch/minimax/backups"))
        .unwrap()
        .count();
    assert_eq!(backups, 1);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn switch_refuses_running_clients_unless_allowed() {
    let root = temp_root();
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-a", 1));
    let a = engine(&root).capture(Region::Cn).unwrap();
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-b", 1));

    let fake = FakeProcesses {
        running: AtomicBool::new(true),
        ..FakeProcesses::default()
    };
    let engine = engine_with(&root, fake);
    let error = engine
        .switch(Region::Cn, a["id"].as_str().unwrap(), false, false, false)
        .unwrap_err();
    assert!(error.contains("正在运行"));

    let fake = FakeProcesses {
        cli: AtomicBool::new(true),
        ..FakeProcesses::default()
    };
    let engine = engine_with(&root, fake);
    let error = engine
        .switch(Region::Cn, a["id"].as_str().unwrap(), true, false, false)
        .unwrap_err();
    assert!(error.contains("CLI"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn switch_can_close_and_relaunch_the_desktop_client() {
    let root = temp_root();
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-a", 1));
    let a = engine(&root).capture(Region::Cn).unwrap();
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-b", 1));

    let counters = Arc::new(Counters::default());
    let fake = FakeProcesses {
        running: AtomicBool::new(true),
        counters: counters.clone(),
        ..FakeProcesses::default()
    };
    let engine = engine_with(&root, fake);
    let result = engine
        .switch(Region::Cn, a["id"].as_str().unwrap(), true, true, false)
        .unwrap();
    assert_eq!(result["closed"], true);
    assert_eq!(counters.closes.load(Ordering::SeqCst), 1);
    assert_eq!(counters.launches.load(Ordering::SeqCst), 1);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn rename_keeps_custom_name_and_delete_removes_account() {
    let root = temp_root();
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-a", 1));
    let engine = engine(&root);
    let account = engine.capture(Region::Cn).unwrap();
    let id = account["id"].as_str().unwrap();

    let renamed = engine.rename(id, "工作号").unwrap();
    assert_eq!(renamed["name"], "工作号");
    // 再次识别同一登录不会覆盖自定义名称。
    let again = engine.capture(Region::Cn).unwrap();
    assert_eq!(again["name"], "工作号");

    engine.delete(id).unwrap();
    assert!(engine.status().unwrap()["accounts"]
        .as_array()
        .unwrap()
        .is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn capture_without_login_reports_a_clear_error() {
    let root = temp_root();
    let engine = engine(&root);
    let error = engine.capture(Region::Cn).unwrap_err();
    assert!(error.contains("未检测到"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn status_does_not_leak_tokens() {
    let root = temp_root();
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-a", 1));
    let engine = engine(&root);
    engine.capture(Region::Cn).unwrap();
    let text = serde_json::to_string(&engine.status().unwrap()).unwrap();
    assert!(!text.contains("mmoat_"));
    assert!(!text.contains("mmort_"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn switch_is_idempotent_for_the_active_account() {
    let root = temp_root();
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-a", 1));
    let engine = engine(&root);
    let a = engine.capture(Region::Cn).unwrap();
    let result = engine
        .switch(Region::Cn, a["id"].as_str().unwrap(), false, false, false)
        .unwrap();
    assert_eq!(result["alreadyActive"], true);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn duplicate_cards_for_one_user_collapse_once_the_identity_is_known() {
    // 早期版本登录时拿不到平台 uid，只能按 `epoch:` 建卡，同一个账号会留下两张卡。
    let root = temp_root();
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-a", 1));
    let engine = engine(&root);
    let live = engine.capture(Region::Cn).unwrap();
    let payload = live_payload(Region::Cn, "epoch-b", 1);
    let duplicate = {
        let mut store = engine.load().unwrap();
        let other = super::Live {
            credential: super::protocol::parse(&payload, Region::Cn).unwrap(),
            payload,
        };
        // 没有身份可比时只能按代际建卡，正是旧版本留下的重复卡片。
        let account = engine.upsert(&mut store, Region::Cn, &other, None).unwrap();
        engine.save(&store).unwrap();
        account.meta(false)
    };
    assert_ne!(live["id"], duplicate["id"]);

    let mut store = engine.load().unwrap();
    assert_eq!(store.accounts.len(), 2);
    // 现场那张优先保留：客户端的「当前账号」指向它。
    for account in store.accounts.iter_mut() {
        account.uid = "576404468350857218".to_string();
    }
    let uid = "576404468350857218";
    assert_eq!(
        super::drop_same_uid(&mut store, Region::Cn, uid, live["id"].as_str().unwrap()),
        1
    );
    assert_eq!(store.accounts.len(), 1);
    assert_eq!(store.accounts[0].id, live["id"]);
    // 另一个区域不受影响。
    let mut store2 = engine.load().unwrap();
    for account in store2.accounts.iter_mut() {
        account.region = Region::En;
    }
    assert_eq!(
        super::drop_same_uid(&mut store2, Region::Cn, uid, live["id"].as_str().unwrap()),
        0
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn relogin_keeps_the_card_current_even_before_switching() {
    // 客户端重新登录后现场代际变了、账号没变：这张卡仍要显示为当前，
    // 否则界面会同时出现「本机当前登录：尚未添加」和一张同名卡片。
    let root = temp_root();
    write_identity(&root, "coder");
    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-a", 1));
    let engine = engine(&root);
    let card = engine.capture(Region::Cn).unwrap();

    write_live(&root, Region::Cn, &live_payload(Region::Cn, "epoch-b", 2));
    let status = engine.status().unwrap();
    assert_eq!(status["clients"][0]["activeAccountId"], card["id"]);
    assert_eq!(status["accounts"][0]["active"], true);

    // 现场换成另一个人的登录：uid 对不上，就不能再把这算成当前账号。
    write_identity_uid(&root, "someone-else", "999999999");
    let status = engine.status().unwrap();
    assert_eq!(status["clients"][0]["activeAccountId"], Value::Null);
    assert_eq!(status["accounts"][0]["active"], false);
    std::fs::remove_dir_all(root).ok();
}
