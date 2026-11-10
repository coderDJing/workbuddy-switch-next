use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
struct TestVault;
impl Vault for TestVault {
    fn seal(&self, v: &[u8]) -> Result<String, String> {
        Ok(STANDARD.encode(v))
    }
    fn open(&self, v: &str) -> Result<Vec<u8>, String> {
        STANDARD.decode(v).map_err(|_| "bad vault".into())
    }
}
struct TestProcesses {
    running: Arc<AtomicBool>,
    fail_scan: bool,
    refuse_close: bool,
}
impl Processes for TestProcesses {
    fn running(&self, _: Region, _: Client) -> Result<bool, String> {
        if self.fail_scan {
            Err("scan failed".into())
        } else {
            Ok(self.running.load(Ordering::SeqCst))
        }
    }
    fn close(&self, _: Region, _: Client) -> Result<(), String> {
        if self.refuse_close {
            return Err("unsaved".into());
        }
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
    fn launch(&self, _: &Paths, _: Region, _: Client) -> Result<(), String> {
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }
}
struct Fixture {
    engine: Engine,
    root: std::path::PathBuf,
    running: Arc<AtomicBool>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wb-trae-test-{}", uuid::Uuid::new_v4()));
        let running = Arc::new(AtomicBool::new(false));
        let paths = Paths {
            home: root.clone(),
            app_data: root.join("data"),
            store: root.join("store"),
        };
        let engine = Engine {
            paths,
            vault: Box::new(TestVault),
            processes: Box::new(TestProcesses {
                running: running.clone(),
                fail_scan: false,
                refuse_close: false,
            }),
        };
        Self {
            engine,
            root,
            running,
        }
    }
    fn write(&self, uid: &str) {
        let p = self.engine.paths.storage(Region::Cn, Client::Desktop);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p,serde_json::to_vec(&json!({"iCubeAuthInfo://icube.cloudide":{"userId":uid,"token":"test-token","account":{"username":format!("User {uid}")}},"project.history":["keep"],"telemetry.machineId":"keep-machine"})).unwrap()).unwrap();
    }
    fn args(&self, id: Option<&str>) -> Args {
        serde_json::from_value(json!({"accountId":id})).unwrap()
    }
    fn capture(&self) -> String {
        self.engine.capture(&self.args(None)).unwrap()["id"]
            .as_str()
            .unwrap()
            .into()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[test]
fn capture_and_switch_preserve_projects_and_save_displaced_account() {
    let f = Fixture::new();
    f.write("A");
    let a = f.capture();
    f.write("B");
    assert_eq!(
        f.engine.switch(&f.args(Some(&a))).unwrap()["switched"],
        true
    );
    let root = f.engine.storage(Region::Cn, Client::Desktop).unwrap();
    assert_eq!(root["project.history"], json!(["keep"]));
    assert_eq!(root["telemetry.machineId"], "keep-machine");
    assert_eq!(
        protocol::identity(&protocol::auth_keys(&root).unwrap())
            .unwrap()
            .uid,
        "A"
    );
    assert_eq!(f.engine.load().unwrap().accounts.len(), 2);
    assert!(f.engine.paths.store.join("last-switch-backup.enc").exists());
    let status = f.engine.status().unwrap();
    let text = status.to_string();
    assert!(!text.contains("test-token"));
    assert!(!text.contains("payload"));
}
#[test]
fn running_and_dry_run_never_write_or_close() {
    let f = Fixture::new();
    f.write("A");
    let a = f.capture();
    f.write("B");
    f.running.store(true, Ordering::SeqCst);
    let before = std::fs::read(f.engine.paths.storage(Region::Cn, Client::Desktop)).unwrap();
    assert!(f.engine.switch(&f.args(Some(&a))).is_err());
    let mut args = f.args(Some(&a));
    args.dry_run = true;
    assert_eq!(f.engine.switch(&args).unwrap()["requiresClose"], true);
    assert!(f.running.load(Ordering::SeqCst));
    assert_eq!(
        before,
        std::fs::read(f.engine.paths.storage(Region::Cn, Client::Desktop)).unwrap()
    );
}
#[test]
fn refuses_cross_region_corrupt_destination_and_scan_failure() {
    let mut f = Fixture::new();
    f.write("A");
    let a = f.capture();
    let mut args = f.args(Some(&a));
    args.variant = Region::Ai;
    assert!(f.engine.switch(&args).is_err());
    args.variant = Region::Cn;
    f.engine.processes = Box::new(TestProcesses {
        running: f.running.clone(),
        fail_scan: true,
        refuse_close: false,
    });
    assert!(f.engine.switch(&args).is_err());
    std::fs::write(
        f.engine.paths.storage(Region::Cn, Client::Desktop),
        b"broken",
    )
    .unwrap();
    assert!(f.engine.capture(&args).is_err());
    assert_eq!(f.engine.load().unwrap().accounts.len(), 1);
}
#[test]
fn close_refusal_preserves_destination() {
    let mut f = Fixture::new();
    f.write("A");
    let a = f.capture();
    f.write("B");
    f.running.store(true, Ordering::SeqCst);
    f.engine.processes = Box::new(TestProcesses {
        running: f.running.clone(),
        fail_scan: false,
        refuse_close: true,
    });
    let mut args = f.args(Some(&a));
    args.close_running = true;
    assert!(f.engine.switch(&args).is_err());
    assert_eq!(
        protocol::identity(
            &protocol::auth_keys(&f.engine.storage(Region::Cn, Client::Desktop).unwrap()).unwrap()
        )
        .unwrap()
        .uid,
        "B"
    );
}
#[test]
fn corrupt_store_is_never_silently_replaced() {
    let f = Fixture::new();
    f.write("A");
    f.capture();
    let path = f.engine.paths.store.join("accounts.json");
    std::fs::write(&path, "broken").unwrap();
    assert!(f.engine.capture(&f.args(None)).is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "broken");
}
#[test]
fn portable_backups_encrypt_credentials_validate_password_and_preserve_existing_accounts() {
    let source = Fixture::new();
    source.write("A");
    source.capture();
    let exported = source
        .engine
        .export_backup("test-passphrase-123", None)
        .unwrap();
    let data = exported["data"].as_str().unwrap();
    assert!(!STANDARD
        .decode(data)
        .unwrap()
        .windows(b"test-token".len())
        .any(|v| v == b"test-token"));
    let target = Fixture::new();
    assert!(target
        .engine
        .preview_backup(data, "wrong-password")
        .is_err());
    let preview = target
        .engine
        .preview_backup(data, "test-passphrase-123")
        .unwrap();
    assert!(!preview.to_string().contains("test-token"));
    assert!(target
        .engine
        .import_backup(data, "test-passphrase-123", "changed")
        .is_err());
    assert!(target.engine.load().unwrap().accounts.is_empty());
    let result = target
        .engine
        .import_backup(
            data,
            "test-passphrase-123",
            preview["snapshotId"].as_str().unwrap(),
        )
        .unwrap();
    assert_eq!(result["imported"], 1);
    let existing = target.engine.load().unwrap().accounts[0].payload.clone();
    let result = target
        .engine
        .import_backup(
            data,
            "test-passphrase-123",
            preview["snapshotId"].as_str().unwrap(),
        )
        .unwrap();
    assert_eq!(result["skipped"], 1);
    assert_eq!(target.engine.load().unwrap().accounts[0].payload, existing);
    assert!(!target
        .engine
        .paths
        .storage(Region::Cn, Client::Desktop)
        .exists());
}
#[test]
fn cloud_merge_keeps_local_credentials_and_rejects_other_platform_payloads() {
    let f = Fixture::new();
    f.write("A");
    f.capture();
    let local = f.engine.backup_payload().unwrap();
    let mut remote = local.clone();
    remote["accounts"][0]["name"] = json!("Remote name");
    let merged = backup::merge(local.clone(), remote).unwrap();
    assert_eq!(merged, local);
    assert!(backup::merge(local, json!({"format":"wb-switch-qoder","accounts":[]})).is_err());
}
#[tokio::test]
async fn statistics_keep_regions_and_snapshots_isolated_and_defaults_never_run_jobs() {
    let f = Fixture::new();
    f.write("A");
    f.capture();
    let a = f.engine.load().unwrap().accounts[0].clone();
    let credit = json!({"remaining":12.5,"available":true,"unit":"credits","checkedAt":chrono::Utc::now().timestamp()});
    f.engine.cache_credit(&a, &credit).unwrap();
    let cn = f.engine.statistics(Region::Cn, false).await.unwrap();
    assert_eq!(cn["snapshots"].as_array().unwrap().len(), 1);
    assert!(!cn.to_string().contains("test-token"));
    assert!(
        f.engine.statistics(Region::Ai, false).await.unwrap()["snapshots"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        automation::run(&f.engine, true, None).await.unwrap()["ran"],
        false
    );
    assert!(!f.engine.paths.store.join("automation-state.json").exists());
}
#[test]
fn automatic_rotation_defers_running_clients_and_switches_only_after_exit() {
    let f = Fixture::new();
    f.write("A");
    let a = f.capture();
    f.write("B");
    let b = f.capture();
    f.write("A");
    let now = chrono::Utc::now().timestamp();
    let list = f.engine.load().unwrap().accounts;
    for account in &list {
        f.engine.cache_credit(account,&json!({"available":true,"unlimited":false,"remaining":if account.id==a{1.0}else{100.0},"checkedAt":now,"unit":"credits"})).unwrap();
    }
    let accounts = f.engine.load().unwrap().accounts;
    let config = automation::RegionSettings::default();
    let mut state = automation::State::default();
    f.running.store(true, Ordering::SeqCst);
    automation::rotate(&f.engine, &accounts, Region::Cn, &config, &mut state, now).unwrap();
    assert_eq!(
        protocol::identity(
            &protocol::auth_keys(&f.engine.storage(Region::Cn, Client::Desktop).unwrap()).unwrap()
        )
        .unwrap()
        .uid,
        "A"
    );
    f.running.store(false, Ordering::SeqCst);
    automation::rotate(&f.engine, &accounts, Region::Cn, &config, &mut state, now).unwrap();
    assert_eq!(
        protocol::identity(
            &protocol::auth_keys(&f.engine.storage(Region::Cn, Client::Desktop).unwrap()).unwrap()
        )
        .unwrap()
        .uid,
        "B"
    );
    assert!(f.engine.load().unwrap().accounts.iter().any(|a| a.id == b));
}
#[test]
fn credit_cache_preserves_concurrent_metadata_and_never_resurrects_deleted_accounts() {
    let f = Fixture::new();
    f.write("A");
    f.capture();
    let snapshot = f.engine.load().unwrap().accounts[0].clone();
    let mut changed = f.engine.load().unwrap();
    changed.accounts[0].name = "Renamed while querying".into();
    f.engine.save(&changed).unwrap();
    f.engine
        .cache_credit(&snapshot, &json!({"remaining":12.5}))
        .unwrap();
    let saved = f.engine.load().unwrap();
    assert_eq!(saved.accounts[0].name, "Renamed while querying");
    assert_eq!(
        saved.accounts[0].credits.as_ref().unwrap()["remaining"],
        12.5
    );
    let mut deleted = saved;
    deleted.accounts.clear();
    f.engine.save(&deleted).unwrap();
    assert!(f
        .engine
        .cache_credit(&snapshot, &json!({"remaining":99}))
        .is_err());
    assert!(f.engine.load().unwrap().accounts.is_empty());
}
#[test]
fn credit_cache_rejects_a_replaced_login_snapshot() {
    let f = Fixture::new();
    f.write("A");
    f.capture();
    let snapshot = f.engine.load().unwrap().accounts[0].clone();
    let mut changed = f.engine.load().unwrap();
    changed.accounts[0].payload = "replacement".into();
    f.engine.save(&changed).unwrap();
    assert!(f
        .engine
        .cache_credit(&snapshot, &json!({"remaining":99}))
        .is_err());
    assert!(f.engine.load().unwrap().accounts[0].credits.is_none());
}
#[test]
fn credits_keep_fractional_unknown_and_unlimited_distinct() {
    let pack = |limit, used, kind| json!({"entitlement_base_info":{"product_type":kind,"quota":{"credits_limit":limit}},"usage":{"credits_amount":used}});
    let payload = |packs| json!({"code":0,"data":{"is_credits_billing":true,"user_entitlement_pack_list":packs}});
    let v = credits::summarize(&payload(vec![pack(100.5, 25.25, 0), pack(999.0, 0.0, 3)])).unwrap();
    assert_eq!(v["remaining"], 75.25);
    assert_eq!(
        credits::summarize(&payload(vec![pack(-1.0, 2.0, 0)])).unwrap()["remaining"],
        Value::Null
    );
    assert_eq!(
        credits::summarize(&payload(vec![json!({})])).unwrap()["available"],
        false
    );
    assert!(credits::summarize(&json!({"code":401})).is_err());
    // Exhausted buckets cannot consume the remaining balance of another pack.
    assert_eq!(
        credits::summarize(&payload(vec![pack(10.0, 20.0, 0), pack(5.0, 0.0, 0)])).unwrap()
            ["remaining"],
        5.0
    );
    let mut hidden = pack(100.0, 0.0, 0);
    hidden["is_hide"] = json!(true);
    let mut expired = pack(100.0, 0.0, 0);
    expired["entitlement_base_info"]["end_time"] = json!(1);
    assert_eq!(
        credits::summarize(&payload(vec![pack(10.0, 2.0, 0), hidden, expired])).unwrap()
            ["remaining"],
        8.0
    );
    let dollar = json!({"data":{"is_dollar_usage_billing":true,"user_entitlement_pack_list":[{"entitlement_base_info":{"quota":{"basic_usage_limit":10.5,"bonus_usage_limit":2.5}},"usage":{"basic_usage_amount":2.25,"bonus_usage_amount":0.5}}]}});
    let balance = credits::summarize(&dollar).unwrap();
    assert_eq!(balance["unit"], "usd");
    assert_eq!(balance["remaining"], 10.25);
    // Zero-filled legacy credit columns don't prove this is credit billing.
    assert_eq!(
        credits::summarize(
            &json!({"is_dollar_usage_billing":false,"user_entitlement_pack_list":[pack(0.0,0.0,0)]})
        )
        .unwrap()["available"],
        false
    );
    assert_eq!(
        credits::summarize(&payload(vec![pack(0.0, 0.0, 0)])).unwrap()["remaining"],
        0.0
    );
    // 国内版菜单余额包含 status 0 的正数积分包，加速包仍然排除。
    let future = chrono::Utc::now().timestamp() + 86_400;
    let cn = json!({"is_credits_billing":true,"is_dollar_usage_billing":false,"user_entitlement_pack_list":[
        {"status":0,"display_desc":"新用户福利","entitlement_base_info":{"product_type":2,"end_time":future,"quota":{"credits_limit":2000.0}},"usage":{"credits_amount":0.0}},
        {"status":1,"display_desc":"每月登录赠送","entitlement_base_info":{"product_type":2,"end_time":future,"quota":{"credits_limit":700.0}},"usage":{"credits_amount":5.0}},
        {"status":1,"display_desc":"免费","entitlement_base_info":{"product_type":0,"end_time":future,"quota":{"credits_limit":0.0}},"usage":{"credits_amount":0.0}},
        {"status":1,"display_desc":"<script>","entitlement_base_info":{"product_type":3,"end_time":future,"quota":{"credits_limit":999.0}},"usage":{"credits_amount":0.0}}
    ]});
    let menu = credits::summarize(&cn).unwrap();
    assert_eq!(menu["available"], true);
    assert_eq!(menu["unit"], "credits");
    assert_eq!(menu["remaining"], 2695.0);
    assert_eq!(menu["total"], 2700.0);
    assert_eq!(menu["packs"][0]["name"], "新用户福利");
    assert_eq!(menu["packs"][0]["remaining"], 2000.0);
    assert_eq!(menu["packs"][1]["name"], "每月登录赠送");
    assert_eq!(menu["packs"][1]["remaining"], 695.0);
    assert_eq!(menu["packs"].as_array().unwrap().len(), 2);
    // 未消耗包可以没有 credits_amount，按 0 计算，不能把同账号的其他余额变成未知。
    let missing_used = json!({"is_credits_billing":true,"is_dollar_usage_billing":false,"user_entitlement_pack_list":[
        {"status":0,"entitlement_base_info":{"product_type":2,"end_time":future,"quota":{"credits_limit":2000}},"usage":{}},
        {"status":1,"entitlement_base_info":{"product_type":2,"end_time":future,"quota":{"credits_limit":700}},"usage":{"credits_amount":5}}
    ]});
    assert_eq!(credits::summarize(&missing_used).unwrap()["remaining"], 2695.0);
}
#[test]
fn expired_login_is_rejected_before_closing_a_client() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let f = Fixture::new();
    f.write("A");
    let path = f.engine.paths.storage(Region::Cn, Client::Desktop);
    let mut root = f.engine.storage(Region::Cn, Client::Desktop).unwrap();
    root["iCubeAuthInfo://icube.cloudide"]["token"] = json!(format!(
        "e30.{}.x",
        URL_SAFE_NO_PAD.encode(br#"{"data":{"id":"A"},"exp":1}"#)
    ));
    std::fs::write(&path, root.to_string()).unwrap();
    let a = f.capture();
    f.running.store(true, Ordering::SeqCst);
    let mut args = f.args(Some(&a));
    args.close_running = true;
    assert!(f.engine.switch(&args).unwrap_err().contains("过期"));
    assert!(f.running.load(Ordering::SeqCst));
}
#[test]
fn mismatched_jwt_identity_and_non_auth_keys_rejected() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let token = format!(
        "e30.{}.x",
        URL_SAFE_NO_PAD.encode(br#"{"data":{"id":"B"}}"#)
    );
    let keys = serde_json::from_value(
        json!({"iCubeAuthInfo://icube.cloudide":{"userId":"A","token":token}}),
    )
    .unwrap();
    assert!(protocol::identity(&keys).is_err());
    let f = Fixture::new();
    f.write("A");
    let mut keys =
        protocol::auth_keys(&f.engine.storage(Region::Cn, Client::Desktop).unwrap()).unwrap();
    keys.insert("project.history".into(), json!([]));
    assert!(protocol::patch(&json!({}), &keys).is_err());
}
#[test]
fn client_envelope_integrity_and_both_headers() {
    use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    use sha2::Sha512;
    for (header, salt) in [
        ([116, 99, 5, 16, 0, 0], include_salt(false)),
        ([18, 57, 32, 32, 2, 3], include_salt(true)),
    ] {
        let key = [7u8; 32];
        let mut material = Sha512::digest(key).to_vec();
        material.extend(salt);
        let derived = Sha512::digest(material);
        let payload = br#"{"token":"fixture","userId":"test"}"#;
        let mut plain = Sha512::digest(payload).to_vec();
        plain.extend(payload);
        let len = plain.len();
        plain.resize(len + 16, 0);
        let cipher =
            cbc::Encryptor::<aes::Aes128>::new_from_slices(&derived[..16], &derived[16..32])
                .unwrap()
                .encrypt_padded_mut::<Pkcs7>(&mut plain, len)
                .unwrap();
        let mut raw = header.to_vec();
        raw.extend(key);
        raw.extend(cipher);
        let value = json!(STANDARD.encode(&raw));
        assert_eq!(protocol::decode(&value).unwrap()["userId"], "test");
        raw[40] ^= 1;
        assert!(protocol::decode(&json!(STANDARD.encode(raw))).is_err());
    }
}
mod constants {
    include!("crypto_constants.rs");
    pub fn salt(private: bool) -> [u8; 64] {
        if private {
            SALT_PRIVATE
        } else {
            SALT_AES
        }
    }
}
fn include_salt(private: bool) -> [u8; 64] {
    constants::salt(private)
}
#[test]
fn failed_readback_rolls_back_exact_original_bytes() {
    let f = Fixture::new();
    f.write("A");
    let path = f.engine.paths.storage(Region::Cn, Client::Desktop);
    let original = std::fs::read(&path).unwrap();
    assert!(replace_and_verify(&path, &original, "{}", || Err(
        "injected verification failure".into()
    ))
    .is_err());
    assert_eq!(std::fs::read(path).unwrap(), original);
}
#[tokio::test]
#[ignore = "Explicit local read-only smoke: requires an already logged-in TRAE CN installation"]
async fn local_cn_readonly_smoke() {
    let engine = Engine::real();
    let root = engine
        .storage(Region::Cn, Client::Desktop)
        .expect("local TRAE storage unavailable");
    let auth = protocol::identity(&protocol::auth_keys(&root).unwrap())
        .expect("local TRAE auth schema unsupported");
    let balance = credits::query(Region::Cn, &auth, 30)
        .await
        .expect("official TRAE usage query failed");
    println!(
        "local auth integrity: passed; credits schema recognized: {}",
        balance["available"]
    );
}
#[cfg(windows)]
#[test]
fn system_vault_does_not_store_plaintext_and_detects_tampering() {
    let vault = SystemVault;
    let secret = b"fixture-not-a-real-token";
    let cipher = vault.seal(secret).unwrap();
    assert!(!cipher.contains("fixture"));
    assert_eq!(vault.open(&cipher).unwrap(), secret);
    let mut bytes = STANDARD.decode(cipher).unwrap();
    let n = bytes.len();
    bytes[n - 1] ^= 1;
    assert!(vault.open(&STANDARD.encode(bytes)).is_err());
}
