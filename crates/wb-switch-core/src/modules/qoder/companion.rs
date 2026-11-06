//! Qoder's documented CLI hooks -> Companion custom source. No prompt/tool contents cross IPC.
use super::{store, QoderRoots, QoderTarget};
use crate::modules::{config, state_lock::StateLock};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const OWNER: &str = "wb-switch-qoder-companion-v1";
const EVENTS: &[&str] = &[
    "UserPromptSubmit",
    "PermissionRequest",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionDenied",
    "Stop",
    "StopFailure",
    "SessionEnd",
];
pub fn template(global: bool) -> Value {
    json!({"schemaVersion":1,"id":if global {"wb-qoder-ai-v1"} else {"wb-qoder-cn-v1"},
        "name":if global {"Qoder CLI"} else {"Qoder CN CLI"},"transport":"hook",
        "mapping":{"event":"/event","sessionId":"/sessionId","requestId":"/requestId","cwd":"/cwd"},
        "events":{"UserPromptSubmit":{"action":"start"},"PermissionRequest":{"action":"wait","reason":"permission"},
            "PostToolUse":{"action":"resume"},"PostToolUseFailure":{"action":"resume"},"PermissionDenied":{"action":"resume"},
            "Stop":{"action":"finish","status":"done"},"StopFailure":{"action":"finish","status":"error"},"SessionEnd":{"action":"close"}}})
}
fn settings_path(global: bool) -> std::path::PathBuf {
    QoderRoots::real()
        .cli_dir_for(if global {
            QoderTarget::GlobalCli
        } else {
            QoderTarget::Cli
        })
        .join("settings.json")
}
fn load(path: &Path) -> Result<Value, String> {
    match std::fs::read(path) {
        Ok(data) => serde_json::from_slice::<Value>(&data)
            .ok()
            .filter(Value::is_object)
            .ok_or("Qoder settings.json 无效，已保留原文件".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(_) => Err("无法读取 Qoder settings.json".into()),
    }
}
fn installed(settings: &Value) -> bool {
    EVENTS.iter().all(|event| {
        settings["hooks"][event].as_array().is_some_and(|groups| {
            groups.iter().any(|g| {
                g["hooks"]
                    .as_array()
                    .is_some_and(|hooks| hooks.iter().any(|h| h["name"] == OWNER))
            })
        })
    })
}
pub fn status(global: bool) -> Result<Value, String> {
    Ok(json!({"installed":installed(&load(&settings_path(global))?),"template":template(global)}))
}
fn edit(settings: &mut Value, command: &str, enable: bool) -> Result<(), String> {
    if settings.get("hooks").is_none() {
        settings["hooks"] = json!({});
    }
    let hooks = settings["hooks"]
        .as_object_mut()
        .ok_or("Qoder hooks 配置无效，未修改")?;
    for event in EVENTS {
        if !hooks.contains_key(*event) {
            if !enable {
                continue;
            }
            hooks.insert(event.to_string(), json!([]));
        }
        let groups = hooks
            .get_mut(*event)
            .and_then(Value::as_array_mut)
            .ok_or("Qoder Hook 分组无效，未修改")?;
        for group in groups.iter_mut() {
            let entries = group["hooks"]
                .as_array_mut()
                .ok_or("Qoder Hook 条目无效，未修改")?;
            entries.retain(|h| h["name"] != OWNER);
        }
        groups.retain(|g| g["hooks"].as_array().is_some_and(|h| !h.is_empty()));
        if enable {
            groups.push(json!({"matcher":"*","hooks":[{"type":"command","name":OWNER,"command":command,"timeout":3}]}));
        }
    }
    Ok(())
}
pub fn configure(global: bool, enable: bool) -> Result<Value, String> {
    let _lock = StateLock::try_at(&store::default_store().join("hooks.lock"))
        .map_err(|_| "Qoder Hooks 配置正在修改")?;
    let path = settings_path(global);
    if enable {
        std::fs::create_dir_all(path.parent().ok_or("Qoder 配置路径无效")?)
            .map_err(|_| "无法创建 Qoder 配置目录")?;
    } else if !path.exists() {
        return status(global);
    }
    let before = std::fs::read(&path).ok();
    let mut settings = load(&path)?;
    let exe = std::env::current_exe().map_err(|_| "无法确定 Hooks 程序位置")?;
    let exe = exe.to_str().ok_or("Hooks 程序路径不是有效文本")?;
    if exe.contains(['\n', '\r', '"', '%', '!', '$', '`']) {
        return Err("Hooks 程序路径含不支持的字符".into());
    }
    let quoted = if cfg!(windows) {
        format!("\"{}\"", exe.replace('\\', "/"))
    } else {
        format!("'{}'", exe.replace('\'', "'\\''"))
    };
    let command = format!(
        "{quoted} qoder-companion-hook {}",
        if global { "ai" } else { "cn" }
    );
    edit(&mut settings, &command, enable)?;
    if std::fs::read(&path).ok() != before {
        return Err("Qoder 配置已变化，请重试".into());
    }
    config::atomic_write(
        &path,
        &serde_json::to_string_pretty(&settings).map_err(|_| "无法生成 Qoder 配置")?,
    )
    .map_err(|_| "无法写入 Qoder 配置")?;
    status(global)
}
fn sanitized(raw: &Value) -> Option<Value> {
    if ["parent_session_id", "parentSessionId"]
        .iter()
        .any(|k| raw.get(k).is_some_and(|v| !v.is_null() && v != ""))
    {
        return None;
    }
    let event = raw["hook_event_name"].as_str()?;
    if !EVENTS.contains(&event) {
        return None;
    }
    let session = raw["session_id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)?;
    let mut result = json!({"event":event,"sessionId":session});
    if let Some(cwd) = raw["cwd"].as_str().filter(|s| s.len() <= 2048) {
        result["cwd"] = json!(cwd);
    }
    if matches!(
        event,
        "PermissionRequest" | "PermissionDenied" | "PostToolUse" | "PostToolUseFailure"
    ) {
        // Without a matching tool id a tool completion cannot safely clear a permission wait.
        result["requestId"] = json!(raw["tool_use_id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256)?);
    }
    Some(result)
}
/// Called before either host initializes windows, server, logging or background jobs.
/// Errors never affect the agent; stdout and stderr stay empty.
pub fn run_hook(global: bool) {
    let mut input = Vec::new();
    if std::io::stdin()
        .take(1_048_577)
        .read_to_end(&mut input)
        .is_err()
        || input.len() > 1_048_576
    {
        return;
    }
    let Some(payload) = serde_json::from_slice::<Value>(&input)
        .ok()
        .and_then(|v| sanitized(&v))
    else {
        return;
    };
    let home = config::home_dir();
    let runtime = home.join(".agent-studio/bin").join(if cfg!(windows) {
        "agent-studio-runtime-v1.exe"
    } else {
        "agent-studio-runtime-v1"
    });
    let mut command = Command::new(runtime);
    command
        .arg("custom-hook")
        .arg("--home")
        .arg(&home)
        .arg("--integration")
        .arg(if global {
            "wb-qoder-ai-v1"
        } else {
            "wb-qoder-cn-v1"
        })
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let Ok(mut child) = command.spawn() else {
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.to_string().as_bytes());
    }
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_foreign_hooks_and_removes_only_owned_entries() {
        let original = json!({"model":"keep","hooks":{"Stop":[{"matcher":"x","hooks":[{"type":"command","command":"echo keep"}]}],"Other":[{"hooks":[{"command":"keep"}]}]}});
        let mut value = original.clone();
        edit(&mut value, "host qoder-companion-hook cn", true).unwrap();
        assert!(installed(&value));
        edit(&mut value, "host qoder-companion-hook cn", true).unwrap();
        assert_eq!(value["hooks"]["Stop"].as_array().unwrap().len(), 2);
        edit(&mut value, "", false).unwrap();
        assert_eq!(value["hooks"]["Stop"], original["hooks"]["Stop"]);
        assert_eq!(value["hooks"]["Other"], original["hooks"]["Other"]);
        assert_eq!(value["model"], "keep");
    }
    #[test]
    fn never_forwards_prompt_tool_contents_or_unmatched_resume() {
        let raw = json!({"hook_event_name":"UserPromptSubmit","session_id":"s","cwd":"project","prompt":"SECRET","tool_input":{"token":"SECRET"}});
        assert_eq!(
            sanitized(&raw),
            Some(json!({"event":"UserPromptSubmit","sessionId":"s","cwd":"project"}))
        );
        assert!(sanitized(&json!({"hook_event_name":"PostToolUse","session_id":"s"})).is_none());
        assert!(sanitized(
            &json!({"hook_event_name":"Stop","session_id":"s","parent_session_id":"parent"})
        )
        .is_none());
    }
}
