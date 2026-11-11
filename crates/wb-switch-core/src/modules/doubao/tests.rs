use super::*;
use std::sync::Mutex;
struct TestVault;
impl Vault for TestVault {
    fn seal(&self, b: &[u8]) -> Result<String, String> {
        Ok(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            b,
        ))
    }
    fn open(&self, s: &str) -> Result<Vec<u8>, String> {
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s)
            .map_err(|_| "invalid".into())
    }
}
struct TestProcesses {
    running: Mutex<bool>,
}
impl Processes for TestProcesses {
    fn running(&self) -> Result<bool, String> {
        Ok(*self.running.lock().unwrap())
    }
    fn close(&self) -> Result<(), String> {
        *self.running.lock().unwrap() = false;
        Ok(())
    }
    fn launch(&self, _: &Paths) -> Result<(), String> {
        Ok(())
    }
}
fn fixture(root: &Path, uid: &str) -> Identity {
    std::fs::create_dir_all(root.join("Default/Network")).unwrap();
    let sid = format!("sid{uid}abcdefgh");
    let enterprise = json!({"x-tt-multi-sids":format!("{uid}:{sid}")}).to_string();
    write(&root.join("Local State"),&json!({"profile":{"last_used":"Default","info_cache":{"Default":{"saman":{"user_id":uid,"user_name":uid}}}},"saman":{"local_storage_app_for_web":{"enterprise":enterprise}}})).unwrap();
    write(
        &root.join("Default/Preferences"),
        &json!({"saman":{"last_user":{"user_id":uid,"name":uid}}}),
    )
    .unwrap();
    std::fs::write(root.join("Default/Network/Cookies"), b"cookie-data").unwrap();
    profile::live(root).unwrap().unwrap()
}
fn engine(root: &Path) -> Engine {
    Engine {
        paths: Paths {
            data: root.join("User Data"),
            store: root.join("store"),
            home: root.into(),
            isolated: true,
        },
        vault: Box::new(TestVault),
        processes: Box::new(TestProcesses {
            running: Mutex::new(false),
        }),
    }
}
#[test]
fn switch_preserves_previous_and_rejects_mismatched_snapshot() {
    let root = std::env::temp_dir().join(format!("wb-doubao-test-{}", uuid::Uuid::new_v4()));
    let e = engine(&root);
    let a = fixture(&e.paths.data, "11111111");
    e.capture(false, false).unwrap();
    let b = fixture(&e.paths.data, "22222222");
    e.capture(false, false).unwrap();
    assert_eq!(
        e.switch(&format!("doubao-{}", a.uid), false, false, false, None)
            .unwrap()["verified"],
        true
    );
    assert_eq!(profile::live(&e.paths.data).unwrap().unwrap().uid, a.uid);
    let s = e.load().unwrap();
    assert!(s.accounts.iter().any(|v| v.uid == b.uid));
    let mut altered = s.accounts.iter().find(|v| v.uid == a.uid).unwrap().clone();
    altered.uid = "33333333".into();
    assert!(e.read_snapshot(&altered).is_err());
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn rejects_snapshot_traversal_and_incomplete_leveldb() {
    let mut s = Snapshot {
        files: Default::default(),
    };
    s.files.insert("Default/../../other".into(), "YQ==".into());
    assert!(profile::validate(&s, false).is_err());
    s.files.clear();
    s.files.insert(
        "Default/Local Storage/leveldb/CURRENT".into(),
        "TUFOSUZFU1QtMDAwMDAxCg==".into(),
    );
    assert!(profile::validate(&s, false).is_err());
}
#[test]
fn quota_without_windows_is_unknown_and_checkin_requires_explicit_state() {
    let v = json!({"code":0,"data":{"current_subscription":{"display":{"product_name":"个人订阅"},"status":0},"window_limit_section":{}}});
    let q = network::parse_credits(&v).unwrap();
    assert_eq!(q["available"], false);
    assert_eq!(q["remaining"], Value::Null);
    assert!(network::parse_detail(
        &json!({"data":{"tasks":[{"task_kind":"daily_signin","daily_checkin":{}}]}})
    )
    .is_err());
}
