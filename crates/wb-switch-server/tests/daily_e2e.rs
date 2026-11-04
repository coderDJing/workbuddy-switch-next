//! The Web UI's daily endpoint uses the bundled worker and account filters.
//! No real credentials, upstream services, or live user configuration are used.
#![cfg(unix)]

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn post(port: u16, path: &str, body: &Value) -> (u16, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let body = body.to_string();
    write!(stream, "POST {} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", path, body.len(), body).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap())
}

#[test]
fn daily_endpoint_runs_only_requested_nonexcluded_account() {
    let root = std::env::temp_dir().join(format!("daily-e2e-{}", std::process::id()));
    std::fs::create_dir_all(root.join("home/.wb-switch")).unwrap();
    let store = root.join("home/.wb-switch");
    std::fs::write(
        store.join("accounts.json"),
        json!([
            {"id":"ai-1", "uid":"uid-1", "variant":"ai", "access_token":"test-secret"},
            {"id":"ai-disabled", "uid":"uid-2", "variant":"ai", "access_token":"disabled-secret"},
            {"id":"cn-1", "uid":"uid-cn", "variant":"cn", "access_token":"cn-secret"}
        ])
        .to_string(),
    )
    .unwrap();
    std::fs::write(store.join("auto_checkin_config.json"), json!({"enabled":false,"excluded_account_ids":["ai-disabled"],"checkin_start":"","checkin_end":""}).to_string()).unwrap();
    for (directory, token) in [
        (".qoder-cn", "qoder-cn-secret"),
        (".qoder", "qoder-global-secret"),
    ] {
        let auth = root.join("home").join(directory).join(".auth");
        std::fs::create_dir_all(&auth).unwrap();
        std::fs::write(
            auth.join("user"),
            json!({"uid":"qoder-user", "access_token":token}).to_string(),
        )
        .unwrap();
    }
    let binary = root.join("wb-switch");
    std::fs::copy(env!("CARGO_BIN_EXE_wb-switch"), &binary).unwrap();
    let runner = root.join("daily-runner");
    std::fs::write(&runner, "#!/bin/sh\nread input\ncase \"$input\" in *test-secret*workbuddy-global*|*cn-secret*workbuddy-cn*|*qoder-cn-secret*qoder-cn*|*qoder-global-secret*qoder-global*) case \"$input\" in *'\"skipCheckin\":true'*) printf '{\"ok\":true,\"message\":\"outside fixture\"}';; *) printf '{\"ok\":true,\"message\":\"fixture completed\"}';; esac;; *) exit 1;; esac\n").unwrap();
    std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o700)).unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let child = Command::new(binary)
        .args(["serve", "--no-open", "--port", &port.to_string()])
        .env("WB_SWITCH_HOME", root.join("home"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = ChildGuard(child);
    let mut ready = false;
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            ready = true;
            break;
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "server stopped early"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(ready, "server did not start");
    let (status, value) = post(port, "/api/daily/run", &json!({"variant":"ai"}));
    assert_eq!(status, 200);
    let rows = value["accounts"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["rewards"]["message"], "fixture completed");
    assert_eq!(rows[1]["reason"], "auto_checkin_disabled");
    assert!(!value.to_string().contains("secret"));
    let (start, end) = if chrono::Local::now().format("%H").to_string() == "12" {
        ("13:00", "13:01")
    } else {
        ("12:00", "12:01")
    };
    std::fs::write(store.join("auto_checkin_config.json"), json!({"enabled":false,"excluded_account_ids":["ai-disabled"],"checkin_start":start,"checkin_end":end}).to_string()).unwrap();
    let (_, outside) = post(port, "/api/daily/run", &json!({"variant":"ai"}));
    assert_eq!(outside["accounts"][0]["reason"], "outside_checkin_window");
    assert_eq!(
        outside["accounts"][0]["rewards"]["message"],
        "outside fixture"
    );
    assert_eq!(outside["accounts"][1]["reason"], "auto_checkin_disabled");
    let (_, outside_cn) = post(port, "/api/daily/run", &json!({"variant":"cn"}));
    assert_eq!(
        outside_cn["accounts"][0]["reason"],
        "outside_checkin_window"
    );
    assert_eq!(
        outside_cn["accounts"][0]["rewards"]["message"],
        "outside fixture"
    );
    assert_eq!(
        post(port, "/api/daily/run", &json!({"variant":"wrong"})).0,
        400
    );
    for target in ["cli", "global_cli"] {
        let (status, result) = post(
            port,
            "/api/qoder/rewards",
            &json!({"bundleIds":[], "liveTarget":target}),
        );
        assert_eq!(status, 200);
        assert_eq!(result["message"], "fixture completed");
        assert!(!result.to_string().contains("secret"));
    }
    drop(child);
    std::fs::remove_dir_all(root).unwrap();
}
