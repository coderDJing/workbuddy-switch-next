//! Reconcile local Qoder logins without changing the clients themselves.
//! Persist only hashes identifying user-deleted accounts, never credentials.
use super::{
    authorized,
    paths::{credentials, QoderRoots, QoderTarget},
    store,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

const FILE: &str = "auto-capture.json";
const TARGETS: [QoderTarget; 4] = [
    QoderTarget::Desktop,
    QoderTarget::Cli,
    QoderTarget::GlobalDesktop,
    QoderTarget::GlobalCli,
];

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Suppressed {
    #[serde(default)]
    identities: Vec<String>,
    #[serde(default)]
    unidentified_targets: Vec<QoderTarget>,
}
fn read(root: &Path) -> Result<Suppressed, String> {
    match std::fs::read(root.join(FILE)) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "无法读取 Qoder 自动识别设置".into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Suppressed::default()),
        Err(_) => Err("无法读取 Qoder 自动识别设置".into()),
    }
}
fn write(root: &Path, state: &Suppressed) -> Result<(), String> {
    let text = serde_json::to_string(state).map_err(|_| "无法保存 Qoder 自动识别设置")?;
    crate::modules::config::atomic_write(&root.join(FILE), &text)
        .map_err(|_| "无法保存 Qoder 自动识别设置".into())
}

/// Caller holds switch.lock. Delete suppression is recorded before removing a bundle.
pub(super) fn record_deleted(
    root: &Path,
    roots: &QoderRoots,
    meta: &store::BundleMeta,
) -> Result<(), String> {
    let mut state = read(root)?;
    if let Some(identity) = &meta.identity_key {
        if !state.identities.contains(identity) {
            state.identities.push(identity.clone());
        }
    }
    let live = store::live_primary_sha(roots, meta.target);
    if (meta.identity_key.is_none() || (meta.target.is_desktop() && !cfg!(windows)))
        && live
            .as_deref()
            .is_some_and(|sha| Some(sha) == meta.primary_sha())
        && !state.unidentified_targets.contains(&meta.target)
    {
        state.unidentified_targets.push(meta.target);
    }
    write(root, &state)
}

/// Explicit reauthorization, backup import, or user-requested recognition permits the account again.
/// Caller holds switch.lock.
pub(super) fn allow(
    root: &Path,
    identity: Option<&str>,
    target: Option<QoderTarget>,
) -> Result<(), String> {
    let mut state = read(root)?;
    let before = (state.identities.len(), state.unidentified_targets.len());
    if let Some(identity) = identity {
        state.identities.retain(|item| item != identity);
    }
    if let Some(target) = target {
        state.unidentified_targets.retain(|item| *item != target);
    }
    if before != (state.identities.len(), state.unidentified_targets.len()) {
        write(root, &state)?;
    }
    Ok(())
}

fn suppressed(state: &Suppressed, target: QoderTarget, identity: Option<&str>) -> bool {
    state.unidentified_targets.contains(&target)
        || identity.is_some_and(|id| state.identities.iter().any(|item| item == id))
}

/// Used by status and switch. On failure, do not silently resurrect a deleted account.
pub(super) fn is_current_suppressed(root: &Path, roots: &QoderRoots, target: QoderTarget) -> bool {
    let identity = super::live_identity(roots, target);
    read(root)
        .map(|state| suppressed(&state, target, identity.as_deref()))
        .unwrap_or(true)
}

pub(super) fn verified_live_identity(
    roots: &QoderRoots,
    target: QoderTarget,
) -> Option<(String, Option<String>)> {
    let mut payload = Vec::new();
    for spec in credentials(roots, target) {
        if let Ok(bytes) = std::fs::read(&spec.path) {
            payload.push((spec.name.to_string(), bytes));
        }
    }
    let profile = authorized::snapshot_profile(&roots.desktop_dir_for(target), target, &payload)?;
    let account = super::backup::profile_account(&profile)?;
    let identity = authorized::identity_from_json(target, &json!({"user": account.user}))?;
    let label = profile
        .get("user")
        .and_then(|u| u.get("name"))
        .or_else(|| profile.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.chars().take(128).collect());
    Some((identity, label))
}

pub fn reconcile() -> Result<Value, String> {
    reconcile_with(&store::default_store(), &QoderRoots::real(), cfg!(windows))
}

pub(super) fn reconcile_with(
    root: &Path,
    roots: &QoderRoots,
    automatic_desktop_identity: bool,
) -> Result<Value, String> {
    let _guard = match crate::modules::state_lock::StateLock::wait_at(
        &root.join("switch.lock"),
        Duration::from_secs(2),
    ) {
        Ok(guard) => guard,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            return Ok(
                json!({"added":0,"failedTargets":[],"authorizationRequired":[],"deferred":true}),
            );
        }
        Err(_) => return Err("无法访问 Qoder 自动识别设置".into()),
    };
    let mut state = read(root)?;
    let before = state.unidentified_targets.len();
    state.unidentified_targets.retain(|target| {
        let Some(spec) = credentials(roots, *target).into_iter().next() else { return true; };
        !matches!(std::fs::metadata(spec.path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    });
    if before != state.unidentified_targets.len() {
        write(root, &state)?;
    }
    let mut failures = Vec::new();
    let mut authorization_required = Vec::new();
    let mut added = 0;
    for target in TARGETS {
        if store::live_primary_sha(roots, target).is_none()
            || state.unidentified_targets.contains(&target)
        {
            continue;
        }
        let current =
            store::current_id(root, roots, target).and_then(|id| store::load(root, &id).ok());
        // Desktop keychain access on macOS/Linux happens only in direct response to user action.
        if target.is_desktop() && !automatic_desktop_identity {
            #[allow(unused_mut)]
            let mut needs_authorization = current
                .as_ref()
                .is_none_or(|meta| meta.identity_key.is_none());
            #[cfg(not(windows))]
            if let Some(identity) = current
                .as_ref()
                .and_then(|meta| meta.identity_key.as_deref())
            {
                let other_target = if target.is_global() {
                    QoderTarget::GlobalCli
                } else {
                    QoderTarget::Cli
                };
                let has_cli = store::list(root, Some(other_target))
                    .iter()
                    .any(|meta| meta.identity_key.as_deref() == Some(identity));
                let primary = credentials(roots, target)
                    .into_iter()
                    .next()
                    .and_then(|spec| std::fs::read(spec.path).ok());
                let cached = primary
                    .as_deref()
                    .and_then(|bytes| authorized::cached_desktop_profile(target, bytes))
                    .is_some();
                needs_authorization |= !has_cli && !cached;
            }
            if needs_authorization {
                authorization_required.push(target);
            }
            continue;
        }
        let Some((identity, detected_label)) = verified_live_identity(roots, target) else {
            failures.push(target);
            continue;
        };
        if suppressed(&state, target, Some(&identity)) {
            continue;
        }
        let all = store::list(root, None);
        if all.iter().any(|meta| {
            meta.target == target && meta.identity_key.as_deref() == Some(identity.as_str())
        }) {
            continue;
        }
        let matching_label = all
            .iter()
            .rev()
            .find(|meta| meta.identity_key.as_deref() == Some(identity.as_str()))
            .map(|meta| meta.label.clone());
        let label = matching_label.or(detected_label);
        match store::capture_verified(root, roots, target, label.as_deref(), &identity) {
            Ok(meta) if meta.identity_key.as_deref() == Some(identity.as_str()) => added += 1,
            _ => failures.push(target),
        }
    }
    Ok(
        json!({"added":added,"failedTargets":failures,"authorizationRequired":authorization_required}),
    )
}
