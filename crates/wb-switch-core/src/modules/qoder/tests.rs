use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::paths::{QoderRoots, QoderTarget};
use super::proc::QoderProcessOps;
use super::store;
use super::switch::{execute, SwitchOptions};

struct Sandbox {
    root: PathBuf,
    roots: QoderRoots,
    store: PathBuf,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn sandbox() -> Sandbox {
    let root =
        std::env::temp_dir().join(format!("wb-qoder-test-{}", uuid::Uuid::new_v4().simple()));
    let roots = QoderRoots {
        home: root.join("home"),
        app_data: root.join("appdata"),
    };
    Sandbox {
        store: root.join("store"),
        roots,
        root,
    }
}

fn put(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// 造一份桌面端登录态。`overlay` 为 None 时不写成组文件。
fn login_desktop(sb: &Sandbox, auth: &[u8], name: &str, overlay: Option<&[u8]>) {
    let dir = sb.roots.desktop_dir();
    put(&dir.join("auth.v1.dat"), auth);
    put(&dir.join("Local State"), b"{\"os_crypt\":{}}");
    put(&dir.join("auth.machine-id"), b"device");
    let overlay_path = dir.join("auth-profile-overlays.v1.dat");
    match overlay {
        Some(b) => put(&overlay_path, b),
        None => {
            let _ = std::fs::remove_file(&overlay_path);
        }
    }
    let echo = format!("{{\"logged_in\":true,\"name\":\"{name}\"}}");
    put(&sb.roots.status_echo(), echo.as_bytes());
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[derive(Default)]
struct FakeOps {
    running: Cell<bool>,
    hosted: bool,
    closed: Cell<u32>,
    launched: RefCell<Vec<Option<PathBuf>>>,
    write_on_close: RefCell<Option<(PathBuf, Vec<u8>)>>,
}

impl QoderProcessOps for FakeOps {
    fn running_pids(&self, _: QoderTarget) -> Vec<u32> {
        if self.running.get() {
            vec![42]
        } else {
            Vec::new()
        }
    }
    fn running_exe(&self, _: QoderTarget) -> Option<PathBuf> {
        self.running.get().then(|| PathBuf::from("Qoder CN.exe"))
    }
    fn close(&self, _: QoderTarget, _: Duration) -> Result<(), String> {
        self.closed.set(self.closed.get() + 1);
        self.running.set(false);
        if let Some((path, value)) = self.write_on_close.borrow_mut().take() {
            put(&path, &value);
        }
        Ok(())
    }
    fn launch(&self, _: QoderTarget, exe: Option<PathBuf>) -> Result<(), String> {
        self.launched.borrow_mut().push(exe);
        Ok(())
    }
    fn hosted_by(&self, _: QoderTarget) -> bool {
        self.hosted
    }
}

const OPEN: SwitchOptions = SwitchOptions {
    close_running: true,
    relaunch: true,
};

fn no_progress(_: &str) {}

#[test]
fn capture_requires_login() {
    let sb = sandbox();
    let err = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap_err();
    assert!(err.contains("未登录"), "{err}");
}

#[test]
fn authorized_cli_snapshot_switches_and_restores_previous_login() {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    use base64::{engine::general_purpose::STANDARD, Engine};
    let sb = sandbox();
    let auth = sb.roots.cli_dir_for(QoderTarget::Cli).join(".auth");
    let original = br#"{"token":"old-token","uid":"old-user"}"#;
    put(&auth.join("user"), original);
    put(&auth.join("machine_id"), b"0123456789abcdef-machine");
    let authorized = super::authorized::AuthorizedAccount {
        access_token: "new-token".into(),
        refresh_token: Some("new-refresh".into()),
        expires_at: None,
        refresh_expires_at: None,
        user: serde_json::json!({"id":"new-user", "name":"New"}),
    };
    let payload = super::authorized::build_snapshot(
        &sb.roots,
        QoderTarget::Cli,
        &authorized,
        "0123456789abcdef-machine",
    )
    .unwrap();
    let account =
        store::save_authorized(&sb.store, QoderTarget::Cli, "New", "test-key", payload).unwrap();
    let switched = execute(
        &sb.store,
        &sb.roots,
        &FakeOps::default(),
        &account.id,
        OPEN,
        &no_progress,
    )
    .unwrap();
    let previous = switched["autoSavedId"].as_str().unwrap();
    let mut bytes = STANDARD.decode(read(&auth.join("user"))).unwrap();
    let decrypted =
        cbc::Decryptor::<aes::Aes128>::new_from_slices(b"0123456789abcdef", b"0123456789abcdef")
            .unwrap()
            .decrypt_padded_mut::<Pkcs7>(&mut bytes)
            .unwrap();
    let current: serde_json::Value = serde_json::from_slice(decrypted).unwrap();
    assert_eq!(current["access_token"], "new-token");
    assert_eq!(current["security_oauth_token"], "new-token");
    assert_eq!(current["uid"], "new-user");
    execute(
        &sb.store,
        &sb.roots,
        &FakeOps::default(),
        previous,
        OPEN,
        &no_progress,
    )
    .unwrap();
    assert_eq!(read(&auth.join("user")), original);
}

#[test]
fn authorized_cli_can_be_added_before_its_first_login() {
    let sb = sandbox();
    let authorized = super::authorized::AuthorizedAccount {
        access_token: "first-token".into(),
        refresh_token: None,
        expires_at: None,
        refresh_expires_at: None,
        user: serde_json::json!({"id":"first-user"}),
    };
    let payload = super::authorized::build_snapshot(
        &sb.roots,
        QoderTarget::GlobalCli,
        &authorized,
        "0123456789abcdef-generated-device",
    )
    .unwrap();
    let account = store::save_authorized(
        &sb.store,
        QoderTarget::GlobalCli,
        "First",
        "test-key",
        payload,
    )
    .unwrap();
    execute(
        &sb.store,
        &sb.roots,
        &FakeOps::default(),
        &account.id,
        OPEN,
        &no_progress,
    )
    .unwrap();
    let auth = sb.roots.cli_dir_for(QoderTarget::GlobalCli).join(".auth");
    assert_eq!(
        read(&auth.join("machine_id")),
        b"0123456789abcdef-generated-device"
    );
    assert!(!read(&auth.join("user"))
        .windows(b"first-token".len())
        .any(|w| w == b"first-token"));
}

#[cfg(windows)]
#[test]
fn independent_authorization_initializes_both_clients_without_any_prior_login() {
    for (desktop, cli) in [
        (QoderTarget::Desktop, QoderTarget::Cli),
        (QoderTarget::GlobalDesktop, QoderTarget::GlobalCli),
    ] {
        let sb = sandbox();
        let account = super::authorized::AuthorizedAccount {
            access_token: "synthetic-secret".into(),
            refresh_token: Some("synthetic-refresh".into()),
            expires_at: Some("2099-01-01T00:00:00Z".into()),
            refresh_expires_at: None,
            user: serde_json::json!({"id":"synthetic-user"}),
        };
        let sealed = super::vault::seal(&account, "0123456789abcdef-device").unwrap();
        assert!(!sealed
            .windows(b"synthetic-secret".len())
            .any(|v| v == b"synthetic-secret"));
        let identity = super::authorized::identity_key(desktop, "synthetic-user");
        let save = |target| {
            store::save_authorized(
                &sb.store,
                target,
                "Synthetic",
                &identity,
                vec![(super::vault::FILE.into(), sealed.clone())],
            )
            .unwrap()
        };
        let desk_account = save(desktop);
        let cli_account = save(cli);
        assert!(!sb.roots.desktop_dir_for(desktop).exists());
        assert!(!sb.roots.cli_dir_for(cli).exists());
        execute(
            &sb.store,
            &sb.roots,
            &FakeOps::default(),
            &desk_account.id,
            OPEN,
            &no_progress,
        )
        .unwrap();
        let desktop_dir = sb.roots.desktop_dir_for(desktop);
        let bytes = read(&desktop_dir.join("auth.v1.dat"));
        let plain = crate::modules::vscode_cn_inject::decrypt_secret_payload(
            &bytes,
            &desktop_dir,
            &super::authorized::safe_storage_target(desktop),
        )
        .unwrap();
        let profile: serde_json::Value = serde_json::from_slice(&plain).unwrap();
        assert_eq!(profile["schemaVersion"], 1);
        assert_eq!(profile["token"], "synthetic-secret");
        assert_eq!(profile["refreshToken"], "synthetic-refresh");
        assert_eq!(profile["user"]["id"], "synthetic-user");
        assert_eq!(profile["expiresAt"], "2099-01-01T00:00:00Z");
        assert_eq!(
            read(&desktop_dir.join("auth.machine-id")),
            b"0123456789abcdef-device"
        );
        assert_eq!(
            super::live_identity(&sb.roots, desktop).as_deref(),
            Some(identity.as_str())
        );
        execute(
            &sb.store,
            &sb.roots,
            &FakeOps::default(),
            &cli_account.id,
            OPEN,
            &no_progress,
        )
        .unwrap();
        let auth = sb.roots.cli_dir_for(cli).join(".auth");
        assert!(auth.join("user").exists());
        assert_eq!(read(&auth.join("machine_id")), b"0123456789abcdef-device");
        assert_eq!(
            super::live_identity(&sb.roots, cli).as_deref(),
            Some(identity.as_str())
        );
    }
}

#[cfg(windows)]
#[test]
fn independent_account_uses_client_state_after_shutdown() {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    use base64::{engine::general_purpose::STANDARD, Engine};
    let sb = sandbox();
    let auth = sb.roots.cli_dir().join(".auth");
    put(&auth.join("user"), br#"{"token":"old","device":"before"}"#);
    put(&auth.join("machine_id"), b"0123456789abcdef-device");
    let account = super::authorized::AuthorizedAccount {
        access_token: "new-token".into(),
        refresh_token: None,
        expires_at: None,
        refresh_expires_at: None,
        user: serde_json::json!({"id":"new-user"}),
    };
    let encrypted = super::vault::seal(&account, "0123456789abcdef-device").unwrap();
    let saved = store::save_authorized(
        &sb.store,
        QoderTarget::Cli,
        "New",
        "identity",
        vec![(super::vault::FILE.into(), encrypted)],
    )
    .unwrap();
    let ops = FakeOps::default();
    ops.running.set(true);
    *ops.write_on_close.borrow_mut() =
        Some((auth.join("machine_id"), b"fedcba9876543210-device".to_vec()));
    let result = execute(&sb.store, &sb.roots, &ops, &saved.id, OPEN, &no_progress).unwrap();
    let previous = store::load(&sb.store, result["autoSavedId"].as_str().unwrap()).unwrap();
    assert_eq!(
        previous.primary_sha(),
        Some(store::sha256_hex(br#"{"token":"old","device":"before"}"#).as_str())
    );
    let mut ciphertext = STANDARD.decode(read(&auth.join("user"))).unwrap();
    let plain =
        cbc::Decryptor::<aes::Aes128>::new_from_slices(b"fedcba9876543210", b"fedcba9876543210")
            .unwrap()
            .decrypt_padded_mut::<Pkcs7>(&mut ciphertext)
            .unwrap();
    let profile: serde_json::Value = serde_json::from_slice(plain).unwrap();
    assert_eq!(profile["access_token"], "new-token");
    assert!(profile.get("device").is_none());
}

#[cfg(windows)]
#[test]
fn failed_desktop_preparation_reopens_running_client_without_writing_credentials() {
    let sb = sandbox();
    let account = super::authorized::AuthorizedAccount {
        access_token: "synthetic-token".into(),
        refresh_token: None,
        expires_at: None,
        refresh_expires_at: None,
        user: serde_json::json!({"id":"synthetic-user"}),
    };
    let encrypted = super::vault::seal(&account, "0123456789abcdef-device").unwrap();
    let saved = store::save_authorized(
        &sb.store,
        QoderTarget::Desktop,
        "Synthetic",
        "identity",
        vec![(super::vault::FILE.into(), encrypted)],
    )
    .unwrap();
    let ops = FakeOps::default();
    ops.running.set(true);
    let error = execute(&sb.store, &sb.roots, &ops, &saved.id, OPEN, &no_progress).unwrap_err();
    assert!(error.contains("账号已保存"));
    assert_eq!(ops.closed.get(), 1);
    assert_eq!(ops.launched.borrow().len(), 1);
    assert!(!sb.roots.desktop_dir().exists());
}

#[cfg(windows)]
#[test]
fn expired_vault_account_does_not_close_or_change_client() {
    let sb = sandbox();
    let account = super::authorized::AuthorizedAccount {
        access_token: "synthetic-token".into(),
        refresh_token: Some("refresh".into()),
        expires_at: Some("2020-01-01T00:00:00Z".into()),
        refresh_expires_at: None,
        user: serde_json::json!({"id":"synthetic-user"}),
    };
    let sealed = super::vault::seal(&account, "0123456789abcdef-device").unwrap();
    let saved = store::save_authorized(
        &sb.store,
        QoderTarget::Desktop,
        "Synthetic",
        "identity",
        vec![(super::vault::FILE.into(), sealed)],
    )
    .unwrap();
    let ops = FakeOps::default();
    ops.running.set(true);
    let error = execute(&sb.store, &sb.roots, &ops, &saved.id, OPEN, &no_progress).unwrap_err();
    assert!(error.contains("已过期"));
    assert_eq!(ops.closed.get(), 0);
    assert!(!sb.roots.desktop_dir().exists());
}

/// Explicitly run only after a user has authorized a test account. The real
/// vault is read-only; all generated files and switches use sandbox paths.
#[cfg(windows)]
#[test]
#[ignore = "requires an explicitly authorized local Qoder account"]
fn real_authorization_builds_isolated_desktop_and_cli_credentials() {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    use base64::{engine::general_purpose::STANDARD, Engine};
    let real_store = store::default_store();
    let real = store::list(&real_store, Some(QoderTarget::Desktop))
        .into_iter()
        .filter(|item| item.files.len() == 1 && item.files[0].name == super::vault::FILE)
        .max_by_key(|item| item.captured_at)
        .expect("no authorized test account");
    let cipher = std::fs::read(
        store::bundle_dir(&real_store, &real.id)
            .unwrap()
            .join(super::vault::FILE),
    )
    .unwrap();
    assert!(store::sha256_hex(&cipher) == real.files[0].sha256);
    let (account, machine) = super::vault::open(&cipher).unwrap();
    assert!(!account.access_token.is_empty());
    assert!(account
        .refresh_token
        .as_ref()
        .is_some_and(|value| !value.is_empty()));
    let sb = sandbox();
    for target in [QoderTarget::Desktop, QoderTarget::Cli] {
        let meta = store::save_authorized(
            &sb.store,
            target,
            "Test",
            "test-identity",
            vec![(super::vault::FILE.into(), cipher.clone())],
        )
        .unwrap();
        execute(
            &sb.store,
            &sb.roots,
            &FakeOps::default(),
            &meta.id,
            OPEN,
            &no_progress,
        )
        .unwrap();
    }
    let root = sb.roots.desktop_dir();
    let plaintext = crate::modules::vscode_cn_inject::decrypt_secret_payload(
        &read(&root.join("auth.v1.dat")),
        &root,
        &super::authorized::safe_storage_target(QoderTarget::Desktop),
    )
    .unwrap();
    let desktop: serde_json::Value = serde_json::from_slice(&plaintext).unwrap();
    assert!(desktop["token"].as_str() == Some(account.access_token.as_str()));
    assert!(desktop["user"]["id"] == account.user["id"]);
    let auth = sb.roots.cli_dir().join(".auth");
    let key = machine
        .trim()
        .as_bytes()
        .get(..16)
        .expect("invalid test machine id");
    let mut encrypted = STANDARD.decode(read(&auth.join("user"))).unwrap();
    let plaintext = cbc::Decryptor::<aes::Aes128>::new_from_slices(key, key)
        .unwrap()
        .decrypt_padded_mut::<Pkcs7>(&mut encrypted)
        .unwrap();
    let cli: serde_json::Value = serde_json::from_slice(plaintext).unwrap();
    assert!(cli["access_token"].as_str() == Some(account.access_token.as_str()));
    assert!(cli["uid"] == account.user["id"]);
    if std::env::var_os("WB_QODER_TEST_REAL_CLI").is_some() {
        use std::process::{Command, Stdio};
        let exe = super::paths::QoderRoots::real()
            .home
            .join(".qoder-cn")
            .join("bin")
            .join("qoderclicn")
            .join("qoderclicn.exe");
        let mut child = Command::new(exe)
            .arg("--config-dir")
            .arg(sb.roots.cli_dir())
            .args(["status", "--output", "json"])
            .current_dir(&sb.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(45);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("isolated Qoder CLI status timed out");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let output = child.wait_with_output().unwrap();
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        fn contains_value(value: &serde_json::Value, needle: &str) -> bool {
            match value {
                serde_json::Value::String(text) => text == needle,
                serde_json::Value::Array(items) => {
                    items.iter().any(|item| contains_value(item, needle))
                }
                serde_json::Value::Object(items) => {
                    items.values().any(|item| contains_value(item, needle))
                }
                _ => false,
            }
        }
        let user_id = account.user["id"].as_str().unwrap_or("");
        let user_name = account.user["name"].as_str().unwrap_or("");
        let identity_found = parsed.as_ref().is_some_and(|value| {
            (!user_id.is_empty() && contains_value(value, user_id))
                || (!user_name.is_empty() && contains_value(value, user_name))
        });
        println!(
            "CLI probe: success={}, stdout_json={}, identity_found={}, stderr_present={}",
            output.status.success(),
            parsed.is_some(),
            identity_found,
            !output.stderr.is_empty()
        );
        assert!(output.status.success(), "isolated Qoder CLI status failed");
        assert!(
            identity_found,
            "isolated Qoder CLI did not report the authorized account"
        );
    }
}

#[test]
fn four_targets_have_separate_paths_and_saved_accounts() {
    let sb = sandbox();
    let targets = [
        QoderTarget::Desktop,
        QoderTarget::Cli,
        QoderTarget::GlobalDesktop,
        QoderTarget::GlobalCli,
    ];
    let mut primary_paths = Vec::new();
    for target in targets {
        let specs = super::paths::credentials(&sb.roots, target);
        for spec in &specs {
            if spec.criticality == super::paths::Criticality::Required {
                put(&spec.path, b"same-login-bytes");
            }
        }
        if !target.is_desktop() {
            put(
                &sb.roots
                    .cli_dir_for(target)
                    .join(".auth")
                    .join("machine_id"),
                b"machine",
            );
        }
        primary_paths.push(specs[0].path.clone());
        store::capture(&sb.store, &sb.roots, target, None).unwrap();
    }
    let unique: std::collections::HashSet<_> = primary_paths.iter().collect();
    assert_eq!(unique.len(), 4);
    assert_eq!(
        store::list(&sb.store, None).len(),
        4,
        "相同密文也不能跨目标合并"
    );
    for target in targets {
        assert!(store::current_id(&sb.store, &sb.roots, target).is_some());
    }
}

#[test]
fn global_switch_does_not_touch_cn_credentials() {
    let sb = sandbox();
    let cn_specs = super::paths::credentials(&sb.roots, QoderTarget::Desktop);
    for spec in &cn_specs {
        if spec.criticality == super::paths::Criticality::Required {
            put(&spec.path, b"cn");
        }
    }
    let global_specs = super::paths::credentials(&sb.roots, QoderTarget::GlobalDesktop);
    for spec in &global_specs {
        if spec.criticality == super::paths::Criticality::Required {
            put(&spec.path, b"global-a");
        }
    }
    let a = store::capture(&sb.store, &sb.roots, QoderTarget::GlobalDesktop, None).unwrap();
    put(&global_specs[0].path, b"global-b");
    let ops = FakeOps::default();
    execute(&sb.store, &sb.roots, &ops, &a.id, OPEN, &no_progress).unwrap();
    assert_eq!(read(&global_specs[0].path), b"global-a");
    assert_eq!(read(&cn_specs[0].path), b"cn");
    assert_eq!(
        store::list(&sb.store, Some(QoderTarget::GlobalDesktop)).len(),
        2
    );
}

#[test]
fn empty_credentials_are_not_treated_as_a_login() {
    let sb = sandbox();
    let specs = super::paths::credentials(&sb.roots, QoderTarget::GlobalCli);
    put(&specs[0].path, b"");
    put(&specs[1].path, b"machine");
    assert!(store::live_primary_sha(&sb.roots, QoderTarget::GlobalCli).is_none());
    assert!(store::capture(&sb.store, &sb.roots, QoderTarget::GlobalCli, None).is_err());
}

#[test]
fn plaintext_cli_login_does_not_require_a_machine_key() {
    let sb = sandbox();
    let specs = super::paths::credentials(&sb.roots, QoderTarget::GlobalCli);
    put(&specs[0].path, br#"{"user":{"id":"test"}}"#);
    let a = store::capture(&sb.store, &sb.roots, QoderTarget::GlobalCli, None).unwrap();
    put(&specs[0].path, br#"{"user":{"id":"other"}}"#);
    execute(
        &sb.store,
        &sb.roots,
        &FakeOps::default(),
        &a.id,
        OPEN,
        &no_progress,
    )
    .unwrap();
    assert_eq!(read(&specs[0].path), br#"{"user":{"id":"test"}}"#);
}

#[test]
fn capture_labels_from_echo_and_dedups_by_hash() {
    let sb = sandbox();
    login_desktop(&sb, b"token-a", "Alice", None);
    let a = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();
    assert_eq!(a.label, "Alice");
    let again = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();
    assert_eq!(a.id, again.id);
    assert_eq!(store::list(&sb.store, None).len(), 1);
    assert_eq!(
        store::current_id(&sb.store, &sb.roots, QoderTarget::Desktop),
        Some(a.id)
    );
}

#[test]
fn capture_does_not_merge_distinct_credentials_with_same_echo_name() {
    let sb = sandbox();
    login_desktop(&sb, b"token-a1", "Alice", None);
    let first = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, Some("工作号")).unwrap();
    login_desktop(&sb, b"token-a2", "Alice", None);
    let second = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(store::list(&sb.store, None).len(), 2);
    assert_eq!(store::load(&sb.store, &first.id).unwrap().label, "工作号");
    assert_eq!(
        second.primary_sha(),
        Some(store::sha256_hex(b"token-a2").as_str())
    );
}

#[test]
fn switch_stops_if_current_login_cannot_be_saved() {
    let sb = sandbox();
    let dir = sb.roots.cli_dir().join(".auth");
    put(&dir.join("user"), b"token-a");
    put(&dir.join("machine_id"), b"machine");
    let a = store::capture(&sb.store, &sb.roots, QoderTarget::Cli, None).unwrap();
    put(&dir.join("user"), b"token-b");
    std::fs::remove_file(dir.join("machine_id")).unwrap();
    let ops = FakeOps::default();
    let err = execute(&sb.store, &sb.roots, &ops, &a.id, OPEN, &no_progress).unwrap_err();
    assert!(err.contains("已取消切换"));
    assert_eq!(read(&dir.join("user")), b"token-b");
    assert_eq!(ops.closed.get(), 0);
}

#[test]
fn switch_writes_bundle_and_removes_unpaired_overlay() {
    let sb = sandbox();
    login_desktop(&sb, b"token-a", "Alice", None);
    let a = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();
    login_desktop(&sb, b"token-b", "Bob", Some(b"overlay-b"));
    store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();

    let ops = FakeOps::default();
    let out = execute(&sb.store, &sb.roots, &ops, &a.id, OPEN, &no_progress).unwrap();
    assert_eq!(out["unchanged"], false);
    let dir = sb.roots.desktop_dir();
    assert_eq!(read(&dir.join("auth.v1.dat")), b"token-a");
    assert!(
        !dir.join("auth-profile-overlays.v1.dat").exists(),
        "A 没有成组文件，B 的那份要移除"
    );
    assert!(read(&sb.roots.status_echo())
        .windows(5)
        .any(|w| w == b"Alice"));
    assert_eq!(ops.closed.get(), 0, "目标没在运行，不应关闭");
    assert_eq!(
        store::current_id(&sb.store, &sb.roots, QoderTarget::Desktop),
        Some(a.id)
    );
}

#[test]
fn switch_auto_saves_unknown_live_login() {
    let sb = sandbox();
    login_desktop(&sb, b"token-a", "Alice", None);
    let a = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();
    login_desktop(&sb, b"token-c", "Carol", None);

    let out = execute(
        &sb.store,
        &sb.roots,
        &FakeOps::default(),
        &a.id,
        OPEN,
        &no_progress,
    )
    .unwrap();
    let saved = out["autoSavedId"].as_str().expect("现场应被自动认领");
    assert_eq!(store::load(&sb.store, saved).unwrap().label, "Carol");
    assert_eq!(store::list(&sb.store, None).len(), 2);
}

#[test]
fn switch_saves_final_credentials_written_during_client_shutdown() {
    let sb = sandbox();
    login_desktop(&sb, b"target", "Target", None);
    let target = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();
    login_desktop(&sb, b"old-before-close", "Previous", None);
    let ops = FakeOps::default();
    ops.running.set(true);
    *ops.write_on_close.borrow_mut() = Some((
        sb.roots.desktop_dir().join("auth.v1.dat"),
        b"old-after-close".to_vec(),
    ));
    let result = execute(&sb.store, &sb.roots, &ops, &target.id, OPEN, &no_progress).unwrap();
    let saved = store::load(&sb.store, result["autoSavedId"].as_str().unwrap()).unwrap();
    assert_eq!(
        saved.primary_sha(),
        Some(store::sha256_hex(b"old-after-close").as_str())
    );
    execute(
        &sb.store,
        &sb.roots,
        &FakeOps::default(),
        &saved.id,
        OPEN,
        &no_progress,
    )
    .unwrap();
    assert_eq!(
        read(&sb.roots.desktop_dir().join("auth.v1.dat")),
        b"old-after-close"
    );
}

#[test]
fn switch_to_current_is_noop() {
    let sb = sandbox();
    login_desktop(&sb, b"token-a", "Alice", None);
    let a = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();
    let ops = FakeOps::default();
    ops.running.set(true);
    let out = execute(&sb.store, &sb.roots, &ops, &a.id, OPEN, &no_progress).unwrap();
    assert_eq!(out["unchanged"], true);
    assert_eq!(ops.closed.get(), 0);
}

#[test]
fn switch_closes_and_relaunches_running_desktop() {
    let sb = sandbox();
    login_desktop(&sb, b"token-a", "Alice", None);
    let a = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();
    login_desktop(&sb, b"token-b", "Bob", None);
    store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();

    let ops = FakeOps::default();
    ops.running.set(true);
    let out = execute(&sb.store, &sb.roots, &ops, &a.id, OPEN, &no_progress).unwrap();
    assert_eq!(ops.closed.get(), 1);
    assert_eq!(
        *ops.launched.borrow(),
        vec![Some(PathBuf::from("Qoder CN.exe"))]
    );
    assert_eq!(out["relaunched"], true);
}

#[test]
fn switch_refuses_running_target_without_permission_or_when_hosted() {
    let sb = sandbox();
    login_desktop(&sb, b"token-a", "Alice", None);
    let a = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();
    login_desktop(&sb, b"token-b", "Bob", None);
    store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();

    let ops = FakeOps::default();
    ops.running.set(true);
    let manual = SwitchOptions {
        close_running: false,
        relaunch: false,
    };
    let err = execute(&sb.store, &sb.roots, &ops, &a.id, manual, &no_progress).unwrap_err();
    assert!(err.contains("正在运行"), "{err}");

    let hosted = FakeOps {
        hosted: true,
        ..FakeOps::default()
    };
    hosted.running.set(true);
    let err = execute(&sb.store, &sb.roots, &hosted, &a.id, OPEN, &no_progress).unwrap_err();
    assert!(err.contains("托管"), "{err}");
    assert_eq!(hosted.closed.get(), 0);
    assert_eq!(
        read(&sb.roots.desktop_dir().join("auth.v1.dat")),
        b"token-b",
        "拒绝时现场不能被改"
    );
}

#[test]
fn switch_rejects_tampered_bundle_without_touching_live() {
    let sb = sandbox();
    login_desktop(&sb, b"token-a", "Alice", None);
    let a = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();
    login_desktop(&sb, b"token-b", "Bob", None);
    put(
        &store::bundle_dir(&sb.store, &a.id)
            .unwrap()
            .join("auth.v1.dat"),
        b"tampered",
    );

    let err = execute(
        &sb.store,
        &sb.roots,
        &FakeOps::default(),
        &a.id,
        OPEN,
        &no_progress,
    )
    .unwrap_err();
    assert!(err.contains("校验失败"), "{err}");
    assert_eq!(
        read(&sb.roots.desktop_dir().join("auth.v1.dat")),
        b"token-b"
    );
}

#[test]
fn cli_switch_swaps_user_and_machine_id_as_a_pair() {
    let sb = sandbox();
    let auth = sb.roots.cli_dir().join(".auth");
    put(&auth.join("user"), b"cli-a");
    put(&auth.join("machine_id"), b"mid-a");
    let a = store::capture(&sb.store, &sb.roots, QoderTarget::Cli, Some("CLI A")).unwrap();
    put(&auth.join("user"), b"cli-b");
    put(&auth.join("machine_id"), b"mid-b");
    store::capture(&sb.store, &sb.roots, QoderTarget::Cli, Some("CLI B")).unwrap();

    let ops = FakeOps::default();
    ops.running.set(true);
    let out = execute(&sb.store, &sb.roots, &ops, &a.id, OPEN, &no_progress).unwrap();
    assert_eq!(read(&auth.join("user")), b"cli-a");
    assert_eq!(read(&auth.join("machine_id")), b"mid-a");
    assert!(ops.launched.borrow().is_empty(), "CLI 不自动重开");
    assert_eq!(out["warnings"].as_array().unwrap().len(), 1);
}

#[test]
fn bundle_ids_cannot_escape_store() {
    let sb = sandbox();
    assert!(store::delete(&sb.store, "../etc").is_err());
    assert!(store::load(&sb.store, "a/b").is_err());
}

#[test]
fn backups_are_pruned_per_target() {
    let sb = sandbox();
    login_desktop(&sb, b"token-a", "Alice", None);
    let a = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();
    login_desktop(&sb, b"token-b", "Bob", None);
    let b = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();
    let ops = FakeOps::default();
    for i in 0..12 {
        let id = if i % 2 == 0 { &a.id } else { &b.id };
        execute(&sb.store, &sb.roots, &ops, id, OPEN, &no_progress).unwrap();
        let count = std::fs::read_dir(sb.store.join("backups")).unwrap().count();
        assert_eq!(count, (i + 1).min(10), "each switch needs its own backup");
    }
    let count = std::fs::read_dir(sb.store.join("backups")).unwrap().count();
    assert_eq!(count, 10);
}

#[test]
fn switch_waits_for_brief_lock_contention() {
    let sb = sandbox();
    login_desktop(&sb, b"token-a", "Alice", None);
    let a = store::capture(&sb.store, &sb.roots, QoderTarget::Desktop, None).unwrap();
    login_desktop(&sb, b"token-b", "Bob", None);
    let lock_path = sb.store.join("switch.lock");
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _guard = crate::modules::state_lock::StateLock::try_at(&lock_path).unwrap();
        ready_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(150));
    });
    ready_rx.recv().unwrap();
    let result = execute(
        &sb.store,
        &sb.roots,
        &FakeOps::default(),
        &a.id,
        OPEN,
        &no_progress,
    );
    holder.join().unwrap();
    assert!(result.is_ok(), "短暂占锁后应重试: {result:?}");
    assert_eq!(
        read(&sb.roots.desktop_dir().join("auth.v1.dat")),
        b"token-a"
    );
}

#[test]
fn automatic_qoder_capture_deduplicates_verified_identity_and_keeps_regions_apart() {
    let sb = sandbox();
    for target in [QoderTarget::Cli, QoderTarget::GlobalCli] {
        let path = sb.roots.cli_dir_for(target).join(".auth").join("user");
        put(
            &path,
            br#"{"uid":"same-id","name":"Same Name","access_token":"first-token"}"#,
        );
    }
    let first = super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap();
    assert_eq!(first["added"], 2);
    assert!(first["failedTargets"].as_array().unwrap().is_empty());
    let initial = store::list(&sb.store, None);
    assert_eq!(initial.len(), 2);
    assert_ne!(initial[0].identity_key, initial[1].identity_key);
    let path = sb
        .roots
        .cli_dir_for(QoderTarget::Cli)
        .join(".auth")
        .join("user");
    put(
        &path,
        br#"{"uid":"same-id","name":"Same Name","access_token":"rotated-token"}"#,
    );
    let second = super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap();
    assert_eq!(second["added"], 0);
    assert_eq!(store::list(&sb.store, None).len(), 2);
    assert_eq!(
        read(&path),
        br#"{"uid":"same-id","name":"Same Name","access_token":"rotated-token"}"#
    );
    put(
        &path,
        br#"{"uid":"another-id","name":"Same Name","access_token":"other-token"}"#,
    );
    assert_eq!(
        super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap()["added"],
        1
    );
    assert_eq!(store::list(&sb.store, None).len(), 3);
}

#[test]
fn automatic_qoder_capture_respects_deletion_even_after_credentials_rotate() {
    let sb = sandbox();
    let target = QoderTarget::Cli;
    let path = sb.roots.cli_dir_for(target).join(".auth").join("user");
    put(
        &path,
        br#"{"uid":"user-a","name":"Alice","access_token":"first"}"#,
    );
    super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap();
    let meta = store::list(&sb.store, Some(target)).pop().unwrap();
    super::auto::record_deleted(&sb.store, &sb.roots, &meta).unwrap();
    store::delete(&sb.store, &meta.id).unwrap();
    put(
        &path,
        br#"{"uid":"user-a","name":"Alice","access_token":"rotated"}"#,
    );
    assert!(super::auto::is_current_suppressed(
        &sb.store, &sb.roots, target
    ));
    assert_eq!(
        super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap()["added"],
        0
    );
    assert!(store::list(&sb.store, None).is_empty());
    super::auto::allow(&sb.store, meta.identity_key.as_deref(), Some(target)).unwrap();
    assert_eq!(
        super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap()["added"],
        1
    );
}

#[test]
fn automatic_qoder_capture_does_not_read_desktop_keychain_without_user_action() {
    let sb = sandbox();
    login_desktop(&sb, b"unreadable-desktop-ciphertext", "Desktop", None);
    let path = sb
        .roots
        .cli_dir_for(QoderTarget::Cli)
        .join(".auth")
        .join("user");
    put(&path, br#"{"uid":"cli-user","access_token":"test"}"#);
    let result = super::auto::reconcile_with(&sb.store, &sb.roots, false).unwrap();
    assert_eq!(result["added"], 1);
    assert_eq!(
        result["authorizationRequired"],
        serde_json::json!(["desktop"])
    );
    assert!(result["failedTargets"].as_array().unwrap().is_empty());
    assert_eq!(store::list(&sb.store, Some(QoderTarget::Cli)).len(), 1);
    assert!(store::list(&sb.store, Some(QoderTarget::Desktop)).is_empty());
}

#[test]
fn automatic_qoder_capture_refuses_wrong_identity_and_invalid_credentials() {
    let sb = sandbox();
    let path = sb
        .roots
        .cli_dir_for(QoderTarget::Cli)
        .join(".auth")
        .join("user");
    put(&path, br#"{"uid":"user-a","access_token":"valid"}"#);
    assert!(store::capture_verified(
        &sb.store,
        &sb.roots,
        QoderTarget::Cli,
        None,
        "wrong-identity"
    )
    .is_err());
    assert!(store::list(&sb.store, None).is_empty());
    put(&path, br#"{"uid":"user-a"}"#);
    let result = super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap();
    assert_eq!(result["failedTargets"], serde_json::json!(["cli"]));
    assert!(store::list(&sb.store, None).is_empty());
}

#[test]
fn unidentified_deleted_qoder_login_stays_suppressed_until_logout() {
    let sb = sandbox();
    let target = QoderTarget::Cli;
    let path = sb.roots.cli_dir_for(target).join(".auth").join("user");
    put(&path, br#"{"token":"unidentified"}"#);
    let meta = store::capture(&sb.store, &sb.roots, target, None).unwrap();
    assert!(meta.identity_key.is_none());
    super::auto::record_deleted(&sb.store, &sb.roots, &meta).unwrap();
    store::delete(&sb.store, &meta.id).unwrap();
    let suppressed = super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap();
    assert!(suppressed["failedTargets"].as_array().unwrap().is_empty());
    std::fs::remove_file(&path).unwrap();
    super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap();
    put(&path, br#"{"uid":"new-user","access_token":"new"}"#);
    assert_eq!(
        super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap()["added"],
        1
    );
}

#[cfg(windows)]
#[test]
fn automatic_qoder_capture_merges_desktop_and_cli_by_verified_identity() {
    let sb = sandbox();
    let account = super::authorized::AuthorizedAccount {
        access_token: "synthetic-token".into(),
        refresh_token: Some("synthetic-refresh".into()),
        expires_at: Some("2099-01-01T00:00:00Z".into()),
        refresh_expires_at: None,
        user: serde_json::json!({"id":"same-user", "name":"Same User"}),
    };
    for target in [QoderTarget::Desktop, QoderTarget::Cli] {
        let payload = super::authorized::build_snapshot(
            &sb.roots,
            target,
            &account,
            "0123456789abcdef-device",
        )
        .unwrap();
        let specs = super::paths::credentials(&sb.roots, target);
        for (name, bytes) in payload {
            let spec = specs.iter().find(|spec| spec.name == name).unwrap();
            put(&spec.path, &bytes);
        }
    }
    let before: Vec<_> = [QoderTarget::Desktop, QoderTarget::Cli]
        .into_iter()
        .map(|target| store::live_primary_sha(&sb.roots, target).unwrap())
        .collect();
    let result = super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap();
    assert_eq!(result["added"], 2);
    let saved = store::list(&sb.store, None);
    assert_eq!(saved.len(), 2);
    assert!(saved[0].identity_key.is_some());
    assert_eq!(saved[0].identity_key, saved[1].identity_key);
    let after: Vec<_> = [QoderTarget::Desktop, QoderTarget::Cli]
        .into_iter()
        .map(|target| store::live_primary_sha(&sb.roots, target).unwrap())
        .collect();
    assert_eq!(before, after);
    assert_eq!(
        super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap()["added"],
        0
    );
}

#[cfg(windows)]
#[test]
#[ignore = "requires explicitly authorized local Qoder client logins"]
fn real_qoder_logins_are_auto_detected_without_changing_clients() {
    let sb = sandbox();
    let real = QoderRoots::real();
    let targets = [QoderTarget::Desktop, QoderTarget::Cli];
    let mut source_hashes = Vec::new();
    for target in targets {
        let source = super::paths::credentials(&real, target);
        let destination = super::paths::credentials(&sb.roots, target);
        for spec in &source {
            if let Ok(bytes) = std::fs::read(&spec.path) {
                let dest = destination
                    .iter()
                    .find(|item| item.name == spec.name)
                    .unwrap();
                put(&dest.path, &bytes);
            }
        }
        if let Some(hash) = store::live_primary_sha(&real, target) {
            source_hashes.push((target, hash));
        }
    }
    assert!(!source_hashes.is_empty(), "no logged-in Qoder client");
    let first = super::auto::reconcile_with(&sb.store, &sb.roots, true)
        .expect("Qoder automatic recognition failed");
    assert!(
        first["failedTargets"].as_array().unwrap().is_empty(),
        "Qoder credentials were not recognized"
    );
    assert_eq!(first["added"].as_u64(), Some(source_hashes.len() as u64));
    let again = super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap();
    assert_eq!(again["added"], 0);
    for (target, hash) in source_hashes {
        assert!(
            store::live_primary_sha(&real, target).as_deref() == Some(hash.as_str()),
            "original client credentials changed during test"
        );
    }
}

#[test]
fn switching_does_not_recreate_a_deliberately_deleted_current_account() {
    let sb = sandbox();
    let target = QoderTarget::Cli;
    let path = sb.roots.cli_dir_for(target).join(".auth").join("user");
    put(&path, br#"{"uid":"delete-me","access_token":"token-a"}"#);
    super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap();
    let removed = store::list(&sb.store, Some(target)).pop().unwrap();
    super::auto::record_deleted(&sb.store, &sb.roots, &removed).unwrap();
    store::delete(&sb.store, &removed.id).unwrap();
    put(&path, br#"{"uid":"keep-me","access_token":"token-b"}"#);
    let next = store::capture(&sb.store, &sb.roots, target, Some("Next")).unwrap();
    put(&path, br#"{"uid":"delete-me","access_token":"rotated-a"}"#);
    let result = execute(
        &sb.store,
        &sb.roots,
        &FakeOps::default(),
        &next.id,
        OPEN,
        &no_progress,
    )
    .unwrap();
    assert!(result["autoSavedId"].is_null());
    assert_eq!(store::list(&sb.store, Some(target)).len(), 1);
    assert_eq!(store::list(&sb.store, Some(target))[0].id, next.id);
}

#[test]
fn automatic_qoder_capture_defers_while_switch_is_in_progress() {
    let sb = sandbox();
    let path = sb
        .roots
        .cli_dir_for(QoderTarget::Cli)
        .join(".auth")
        .join("user");
    put(&path, br#"{"uid":"user-a","access_token":"valid"}"#);
    let guard =
        crate::modules::state_lock::StateLock::try_at(&sb.store.join("switch.lock")).unwrap();
    let result = super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap();
    assert_eq!(result["deferred"], true);
    assert!(store::list(&sb.store, None).is_empty());
    drop(guard);
    assert_eq!(
        super::auto::reconcile_with(&sb.store, &sb.roots, true).unwrap()["added"],
        1
    );
}
