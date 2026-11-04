//! Adapter for the bundled Daily task engine. Tokens travel through a private
//! stdin pipe only; the task process never owns or rotates refresh tokens.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn executable() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "daily-runner.exe"
    } else {
        "daily-runner"
    };
    let current = std::env::current_exe().ok()?;
    let adjacent = current.parent()?.join(name);
    if adjacent.is_file() {
        return Some(adjacent);
    }
    // In a development checkout the Tauri sidecar is target-suffixed.
    #[cfg(debug_assertions)]
    {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/binaries");
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let host = option_env!("WB_DAILY_BUILD_TARGET").unwrap_or("");
        let target = directory.join(format!("daily-runner-{host}{suffix}"));
        if target.is_file() {
            return Some(target);
        }
        let direct = directory.join(name);
        if direct.is_file() {
            return Some(direct);
        }
    }
    None
}

fn invoke(program: &Path, request: Value) -> Value {
    let mut command = Command::new(program);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return json!({"ok":false,"message":"无法启动每日任务组件"}),
    };
    let input = match serde_json::to_vec(&request) {
        Ok(input) => input,
        Err(_) => return json!({"ok":false,"message":"无法准备每日任务"}),
    };
    if child.stdin.take().is_none_or(|mut stdin| {
        stdin
            .write_all(&input)
            .and_then(|_| stdin.write_all(b"\n"))
            .is_err()
    }) {
        let _ = child.kill();
        let _ = child.wait();
        return json!({"ok":false,"message":"无法向每日任务组件发送账号"});
    }
    let mut output = Vec::new();
    let read_ok = child
        .stdout
        .take()
        .is_some_and(|stdout| stdout.take(65537).read_to_end(&mut output).is_ok());
    if !read_ok || output.len() > 65536 {
        let _ = child.kill();
        let _ = child.wait();
        return json!({"ok":false,"message":"每日任务组件返回过多数据"});
    }
    let status = child.wait();
    if !status.is_ok_and(|status| status.success()) {
        return json!({"ok":false,"message":"每日任务组件异常退出"});
    }
    let result: Value = serde_json::from_slice(&output).unwrap_or(Value::Null);
    if !result.is_object() || !result.get("ok").is_some_and(Value::is_boolean) {
        return json!({"ok":false,"message":"每日任务组件返回无效结果"});
    }
    result
}

pub async fn run(
    mode: &'static str,
    access_token: String,
    uid: String,
    skip_checkin: bool,
) -> Value {
    let Some(program) = executable() else {
        return json!({"ok":false,"message":"未找到每日任务组件，请安装完整桌面版"});
    };
    let state = crate::modules::config::store_dir().join("daily");
    let request = json!({"mode":mode,"accessToken":access_token,"uid":uid,"stateDir":state,"skipCheckin":skip_checkin});
    tokio::task::spawn_blocking(move || {
        let lock_path = crate::modules::config::store_dir().join("daily.lock");
        let Ok(_guard) = crate::modules::state_lock::StateLock::try_at(&lock_path) else {
            return json!({"ok":false,"message":"另一个每日任务正在运行，请稍后重试"});
        };
        invoke(&program, request)
    })
    .await
    .unwrap_or_else(|_| json!({"ok":false,"message":"每日任务执行失败"}))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn ipc_sends_account_only_on_stdin_and_handles_invalid_output() {
        let dir = std::env::temp_dir().join(format!("daily-ipc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("runner");
        std::fs::write(&script, "#!/bin/sh\nread input\ncase \"$input\" in *test-secret*) printf '{\"ok\":true,\"message\":\"done\"}';; *) exit 1;; esac\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let result = invoke(&script, json!({"accessToken":"test-secret"}));
        assert_eq!(result, json!({"ok":true,"message":"done"}));
        std::fs::write(&script, "#!/bin/sh\nprintf invalid\n").unwrap();
        let result = invoke(&script, json!({"accessToken":"test-secret"}));
        assert_eq!(result["ok"], false);
        assert!(!result.to_string().contains("test-secret"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
