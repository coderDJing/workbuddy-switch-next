//! Allowlisted Chromium login snapshots. Never read browser history, downloads or projects.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const MAX_SNAPSHOT: usize = 64 * 1024 * 1024;
const PROFILE_ITEMS: &[&str] = &[
    "Network/Cookies",
    "Network/Cookies-journal",
    "Network/Cookies-wal",
    "Network/Cookies-shm",
    "Cookies",
    "Cookies-journal",
    "Cookies-wal",
    "Cookies-shm",
    "Local Storage/leveldb",
    "Session Storage",
    "DoubaoStorage",
    "Preferences",
    "saman_shell_db_storage",
];
const ROOT_ITEMS: &[&str] = &["Local State", "saman_app_state", "saman_shell_db_storage"];

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    pub files: BTreeMap<String, String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Identity {
    pub uid: String,
    pub name: String,
    pub profile: String,
    pub session: String,
}
pub(super) fn profile_name(name: &str) -> bool {
    name == "Default"
        || name
            .strip_prefix("Profile ")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}
fn safe_relative(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['\\', ':', '\0'])
        && name
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
}
fn root_of(name: &str) -> Option<String> {
    if !safe_relative(name) {
        return None;
    }
    for item in ROOT_ITEMS {
        if name == *item
            || (*item == "saman_shell_db_storage" && name.starts_with(&format!("{item}/")))
        {
            return Some(item.to_string());
        }
    }
    let (p, tail) = name.split_once('/')?;
    if !profile_name(p) {
        return None;
    }
    for item in PROFILE_ITEMS {
        if tail == *item
            || (matches!(
                *item,
                "Local Storage/leveldb"
                    | "Session Storage"
                    | "DoubaoStorage"
                    | "saman_shell_db_storage"
            ) && tail.starts_with(&format!("{item}/")))
        {
            return Some(format!("{p}/{item}"));
        }
    }
    None
}
fn no_links(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut p = root.to_path_buf();
    if p.symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("豆包目录含链接，已停止操作".into());
    }
    for part in relative.split('/') {
        p.push(part);
        if p.symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err("豆包登录目录含链接，已停止操作".into());
        }
    }
    Ok(p)
}
fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = std::fs::read(path).map_err(|_| "无法读取豆包登录资料，请退出客户端后重试")?;
    if bytes.len() > MAX_SNAPSHOT {
        return Err("豆包登录资料超过读取上限".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "豆包登录资料格式无效".into())
}
fn percent_decode(text: &str) -> Result<String, String> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let pair = b.get(i + 1..i + 3).ok_or("豆包会话编码无效")?;
            let value = u8::from_str_radix(
                std::str::from_utf8(pair).map_err(|_| "豆包会话编码无效")?,
                16,
            )
            .map_err(|_| "豆包会话编码无效")?;
            out.push(value);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "豆包会话编码无效".into())
}
pub(super) fn identity(state: &Value, preferences: &Value) -> Result<Option<Identity>, String> {
    let name = state
        .pointer("/profile/last_used")
        .and_then(Value::as_str)
        .unwrap_or("Default");
    if !profile_name(name) {
        return Err("豆包当前 Profile 名称无效".into());
    }
    let info = &state["profile"]["info_cache"][name]["saman"];
    let last = &preferences["saman"]["last_user"];
    let uid = info["user_id"]
        .as_str()
        .filter(|v| !v.is_empty())
        .or_else(|| last["user_id"].as_str())
        .unwrap_or("");
    if uid.is_empty() {
        return Ok(None);
    }
    if !uid.bytes().all(|b| b.is_ascii_digit()) || uid.len() > 64 {
        return Err("豆包账号身份无效".into());
    }
    // The current profile is authoritative. Preferences.last_user can lag after a same-profile relogin.
    let enterprise: Value = state
        .pointer("/saman/local_storage_app_for_web/enterprise")
        .and_then(Value::as_str)
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| "豆包会话资料格式无效")?
        .unwrap_or(Value::Null);
    let sids = percent_decode(enterprise["x-tt-multi-sids"].as_str().unwrap_or(""))?;
    let session = sids
        .split('|')
        .filter_map(|p| p.split_once(':'))
        .find(|(id, _)| *id == uid)
        .map(|(_, sid)| sid)
        .unwrap_or("");
    if session.is_empty() {
        return Ok(None);
    }
    if session.len() > 512
        || !session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("豆包会话格式不受支持，请重新登录后识别".into());
    }
    let display = if last["user_id"].as_str() == Some(uid) {
        last["name"].as_str().or_else(|| last["user_name"].as_str())
    } else {
        None
    }
    .or_else(|| info["user_name"].as_str())
    .filter(|v| !v.trim().is_empty())
    .unwrap_or("豆包账号");
    Ok(Some(Identity {
        uid: uid.into(),
        name: display.chars().take(80).collect(),
        profile: name.into(),
        session: session.into(),
    }))
}
pub(super) fn live(root: &Path) -> Result<Option<Identity>, String> {
    let state_path = no_links(root, "Local State")?;
    if !state_path.exists() {
        return Ok(None);
    }
    let state = read_json(&state_path)?;
    let name = state
        .pointer("/profile/last_used")
        .and_then(Value::as_str)
        .unwrap_or("Default");
    if !profile_name(name) {
        return Err("豆包当前 Profile 名称无效".into());
    }
    let p = no_links(root, &format!("{name}/Preferences"))?;
    let prefs = if p.exists() {
        read_json(&p)?
    } else {
        Value::Null
    };
    identity(&state, &prefs)
}
fn roots(root: &Path) -> Result<Vec<String>, String> {
    let mut out = ROOT_ITEMS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if !root.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(root).map_err(|_| "无法枚举豆包登录目录")? {
        let entry = entry.map_err(|_| "无法枚举豆包登录目录")?;
        let name = entry.file_name().to_string_lossy().to_string();
        if profile_name(&name) {
            no_links(root, &name)?;
            for item in PROFILE_ITEMS {
                out.push(format!("{name}/{item}"));
            }
        }
    }
    Ok(out)
}
pub(super) fn capture(root: &Path) -> Result<Snapshot, String> {
    fn collect(
        root: &Path,
        key: &str,
        files: &mut BTreeMap<String, String>,
        size: &mut usize,
    ) -> Result<(), String> {
        let p = no_links(root, key)?;
        if !p.exists() {
            return Ok(());
        }
        if p.is_dir() {
            for e in std::fs::read_dir(p).map_err(|_| "无法读取豆包登录目录")? {
                let e = e.map_err(|_| "无法读取豆包登录目录")?;
                let n = e.file_name().to_string_lossy().to_string();
                if n == "LOCK" || n == "LOG" || n == "LOG.old" {
                    continue;
                }
                collect(root, &format!("{key}/{n}"), files, size)?;
            }
        } else {
            let len = p.metadata().map_err(|_| "无法读取豆包登录文件")?.len();
            if len > MAX_SNAPSHOT as u64
                || *size + len as usize > MAX_SNAPSHOT
                || files.len() >= 10000
            {
                return Err("豆包登录快照过大，已停止保存".into());
            }
            let b = std::fs::read(p).map_err(|_| "豆包登录文件被占用，请完全退出客户端后重试")?;
            *size += b.len();
            files.insert(key.into(), STANDARD.encode(b));
        }
        Ok(())
    }
    let mut snapshot = Snapshot {
        files: BTreeMap::new(),
    };
    let mut size = 0;
    for item in roots(root)? {
        collect(root, &item, &mut snapshot.files, &mut size)?;
    }
    validate(&snapshot, false)?;
    Ok(snapshot)
}
pub(super) fn validate(snapshot: &Snapshot, require_login: bool) -> Result<(), String> {
    if snapshot.files.len() > 10000 {
        return Err("豆包快照文件过多".into());
    }
    let mut size = 0;
    for (key, value) in &snapshot.files {
        if root_of(key).is_none() {
            return Err("豆包快照含不允许恢复的路径".into());
        }
        let bytes = STANDARD.decode(value).map_err(|_| "豆包快照编码无效")?;
        size += bytes.len();
        if size > MAX_SNAPSHOT {
            return Err("豆包快照超过大小上限".into());
        }
        if key.ends_with("/Local Storage/leveldb/CURRENT") {
            let manifest = std::str::from_utf8(&bytes)
                .map_err(|_| "豆包登录数据库损坏")?
                .trim();
            if !manifest.starts_with("MANIFEST-")
                || !safe_relative(manifest)
                || manifest.contains('/')
                || !snapshot.files.contains_key(&format!(
                    "{}{manifest}",
                    key.strip_suffix("CURRENT").unwrap()
                ))
            {
                return Err("豆包登录数据库快照不完整".into());
            }
        }
    }
    if require_login {
        let id = snapshot_identity(snapshot)?.ok_or("快照没有完整的豆包登录态")?;
        if !snapshot
            .files
            .contains_key(&format!("{}/Network/Cookies", id.profile))
            && !snapshot
                .files
                .contains_key(&format!("{}/Cookies", id.profile))
        {
            return Err("豆包快照缺少当前账号 Cookie 数据库".into());
        }
    }
    Ok(())
}
pub(super) fn snapshot_identity(s: &Snapshot) -> Result<Option<Identity>, String> {
    let json = |k: &str| -> Result<Value, String> {
        let Some(v) = s.files.get(k) else {
            return Ok(Value::Null);
        };
        serde_json::from_slice(&STANDARD.decode(v).map_err(|_| "豆包快照编码无效")?)
            .map_err(|_| "豆包快照登录资料无效".into())
    };
    let state = json("Local State")?;
    let name = state
        .pointer("/profile/last_used")
        .and_then(Value::as_str)
        .unwrap_or("Default");
    identity(&state, &json(&format!("{name}/Preferences"))?)
}
/// Stage first, replace allowlisted roots, then remove old files. The engine retains an encrypted recovery journal until verification succeeds.
pub(super) fn restore(root: &Path, snapshot: &Snapshot) -> Result<(), String> {
    validate(snapshot, false)?;
    let mut targets: BTreeSet<String> = roots(root)?.into_iter().collect();
    for key in snapshot.files.keys() {
        targets.insert(root_of(key).ok_or("豆包快照路径无效")?);
    }
    for key in &targets {
        no_links(root, key)?;
    }
    let stage = root.join(format!(".wb-switch-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&stage).map_err(|_| "无法创建豆包切换暂存目录")?;
    let result = (|| {
        for (key, value) in &snapshot.files {
            let path = stage.join(key);
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|_| "无法准备豆包切换文件")?;
            std::fs::write(
                path,
                STANDARD.decode(value).map_err(|_| "豆包快照编码无效")?,
            )
            .map_err(|_| "无法准备豆包切换文件")?;
        }
        for key in targets {
            let dest = no_links(root, &key)?;
            if dest.is_dir() {
                std::fs::remove_dir_all(&dest).map_err(|_| "无法替换豆包登录目录")?;
            } else if dest.exists() {
                std::fs::remove_file(&dest).map_err(|_| "无法替换豆包登录文件")?;
            }
            let source = stage.join(&key);
            if source.exists() {
                std::fs::create_dir_all(dest.parent().unwrap())
                    .map_err(|_| "无法创建豆包登录目录")?;
                std::fs::rename(source, dest).map_err(|_| "无法替换豆包登录状态")?;
            }
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(stage);
    result
}
