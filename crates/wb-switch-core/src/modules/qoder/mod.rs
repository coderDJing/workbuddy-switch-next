//! Qoder 国内版和国际版桌面端与 CLI 的账号切换。
//!
//! Qoder 账号与 WorkBuddy / CodeBuddy 账号库互不相通：这里存的是「账号包」
//! （整组加密凭据文件的副本），不解密、不入 `accounts.json`、不参与 WebDAV 同步。
//! 凭据布局与切换行为参考 NextAgentX/qoder-trae-workbuddy-switch 和 JIUSHIQINGSHAN/qoder-switch。
mod authorized;
mod auto;
pub mod backup;
pub mod credits;
pub mod oauth;
pub mod paths;
pub mod proc;
pub mod store;
pub mod switch;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod vault;

use serde_json::{json, Value};

pub use paths::{QoderRoots, QoderTarget};
use proc::{QoderProcessOps, RealProcessOps};
pub use switch::SwitchOptions;

/// Recover only the identity of a live credential after the client rewrites it.
/// Automatic desktop reads are restricted to Windows so a status refresh never
/// opens macOS Keychain or Linux keyring authorization prompts.
fn live_identity(roots: &QoderRoots, target: QoderTarget) -> Option<String> {
    if target.is_desktop() && !cfg!(windows) {
        return None;
    }
    let specs = paths::credentials(roots, target);
    let primary = specs.first()?;
    let mut payload = vec![(primary.name.to_string(), std::fs::read(&primary.path).ok()?)];
    if !target.is_desktop() {
        if let Some(machine) = specs.iter().find(|spec| spec.name == "machine_id") {
            if let Ok(bytes) = std::fs::read(&machine.path) {
                payload.push((machine.name.to_string(), bytes));
            }
        }
    }
    authorized::snapshot_identity(roots, target, &payload)
}

fn target_status(roots: &QoderRoots, target: QoderTarget) -> Value {
    let store = store::default_store();
    let installed = match target {
        QoderTarget::Desktop | QoderTarget::GlobalDesktop => roots.desktop_dir_for(target).is_dir(),
        QoderTarget::Cli | QoderTarget::GlobalCli => roots.cli_dir_for(target).is_dir(),
    };
    let current_id = store::current_id(&store, roots, target);
    let current_meta = current_id
        .as_deref()
        .and_then(|id| store::load(&store, id).ok());
    let current_identity = current_meta
        .as_ref()
        .and_then(|meta| meta.identity_key.clone())
        .or_else(|| {
            store::live_primary_sha(roots, target)
                .and_then(|sha| authorized::cached_live_identity(target, &sha))
                .or_else(|| live_identity(roots, target))
        });
    let current_label = current_meta.map(|m| m.label).or_else(|| {
        let identity = current_identity.as_deref()?;
        store::list(&store, Some(target))
            .into_iter()
            .rev()
            .find(|item| item.identity_key.as_deref() == Some(identity))
            .map(|item| item.label)
    });
    json!({
        "target": target,
        "label": target.label(),
        "installed": installed,
        "loggedIn": store::live_primary_sha(roots, target).is_some(),
        "running": !RealProcessOps.running_pids(target).is_empty(),
        "currentId": current_id,
        "currentIdentityKey": current_identity,
        "autoSuppressed": auto::is_current_suppressed(&store, roots, target),
        "currentLabel": current_label,
        "echoName": match target {
            QoderTarget::Desktop | QoderTarget::GlobalDesktop => store::read_echo_name(roots, target),
            QoderTarget::Cli | QoderTarget::GlobalCli => None,
        },
    })
}

/// GET 状态：两个目标的现场情况 + 全部账号包元数据（不含任何凭据内容）。
pub fn status() -> Value {
    let roots = QoderRoots::real();
    json!({
        "desktop": target_status(&roots, QoderTarget::Desktop),
        "cli": target_status(&roots, QoderTarget::Cli),
        "globalDesktop": target_status(&roots, QoderTarget::GlobalDesktop),
        "globalCli": target_status(&roots, QoderTarget::GlobalCli),
        "bundles": store::list(&store::default_store(), None),
    })
}

pub fn capture(target: QoderTarget, label: Option<&str>) -> Result<Value, String> {
    let root = store::default_store();
    let _guard = crate::modules::state_lock::StateLock::wait_at(
        &root.join("switch.lock"),
        std::time::Duration::from_secs(2),
    )
    .map_err(|_| "Qoder 账号正忙，请稍后重试")?;
    let roots = QoderRoots::real();
    let (identity, detected_label) = auto::verified_live_identity(&roots, target)
        .ok_or("无法识别账号，请允许系统访问 Qoder 登录信息后重试")?;
    let meta = store::capture_verified(
        &root,
        &roots,
        target,
        label.or(detected_label.as_deref()),
        &identity,
    )?;
    auto::allow(&root, meta.identity_key.as_deref(), Some(target))?;
    serde_json::to_value(meta).map_err(|e| e.to_string())
}

pub fn auto_capture() -> Result<Value, String> {
    auto::reconcile()
}

pub fn rename(id: &str, label: &str) -> Result<Value, String> {
    let meta = store::rename(&store::default_store(), id, label)?;
    serde_json::to_value(meta).map_err(|e| e.to_string())
}

pub fn delete(id: &str) -> Result<(), String> {
    let root = store::default_store();
    let _guard = crate::modules::state_lock::StateLock::wait_at(
        &root.join("switch.lock"),
        std::time::Duration::from_secs(2),
    )
    .map_err(|_| "Qoder 账号正忙，请稍后重试")?;
    let meta = store::load(&root, id)?;
    auto::record_deleted(&root, &QoderRoots::real(), &meta)?;
    store::delete(&root, id)?;
    #[cfg(not(windows))]
    authorized::forget_desktop_profile(meta.target, meta.primary_sha());
    Ok(())
}

pub fn switch_to(id: &str, opts: SwitchOptions, progress: &dyn Fn(&str)) -> Result<Value, String> {
    let ops: &dyn QoderProcessOps = &RealProcessOps;
    switch::execute(
        &store::default_store(),
        &QoderRoots::real(),
        ops,
        id,
        opts,
        progress,
    )
}
