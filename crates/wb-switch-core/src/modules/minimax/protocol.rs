//! `auth.json` / `auth-state.json` 的读写约定。
//!
//! 官方客户端的硬约束（`packages/oauth-core/src/auth-core.ts`）：
//! - `credential.generation` 必须等于 `auth-state.generation`，否则状态直接判为 `error`；
//! - `loginEpoch` 在一次登录内保持不变，交互式重新登录才更换；
//! - `auth-state` 不允许出现令牌类字段，刷新事务用 `refresh` 字段表示。
//!
//! 因此切换时两份文件必须**成组**写入：同一代际、清掉刷新事务、状态回到 authenticated。
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::paths::Region;

/// 目前观察到的凭据文件版本。
const AUTH_SCHEMA_VERSION: i64 = 1;
const STATE_SCHEMA_VERSION: i64 = 2;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Payload {
    pub auth: Value,
    pub state: Value,
}

impl Payload {
    pub fn from_bytes(auth: &[u8], state: &[u8]) -> Result<Self, String> {
        let auth = serde_json::from_slice(auth).map_err(|_| "MiniMax auth.json 不是有效 JSON")?;
        let state =
            serde_json::from_slice(state).map_err(|_| "MiniMax auth-state.json 不是有效 JSON")?;
        Ok(Self { auth, state })
    }

    pub fn auth_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec_pretty(&self.auth).map_err(|_| "无法编码 MiniMax 凭据".to_string())
    }

    pub fn state_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec_pretty(&self.state).map_err(|_| "无法编码 MiniMax 登录状态".to_string())
    }
}

#[derive(Clone, Debug)]
pub struct Credential {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_ms: i64,
    pub generation: i64,
    pub login_epoch: Option<String>,
}

impl Credential {
    /// 账号身份：`loginEpoch` 在同一次登录内不变，刷新不会更换它。
    /// 老凭据没有该字段时退化为 refresh token 的摘要。
    pub fn epoch(&self) -> String {
        match self.login_epoch.as_deref() {
            Some(epoch) if !epoch.trim().is_empty() => epoch.to_string(),
            _ => format!("sha256:{}", token_hash(&self.refresh_token)),
        }
    }

    pub fn expired(&self, now_ms: i64) -> bool {
        self.expires_at_ms <= now_ms
    }
}

pub fn token_hash(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

fn records_map(auth: &Value) -> Result<&Map<String, Value>, String> {
    if auth.get("schemaVersion").and_then(Value::as_i64) != Some(AUTH_SCHEMA_VERSION) {
        return Err("MiniMax 凭据版本不受支持，拒绝读写".into());
    }
    auth.get("records")
        .and_then(Value::as_object)
        .ok_or_else(|| "MiniMax 凭据缺少 records".to_string())
}

/// 找到该区域的记录键。键形如 `com.minimax.mcode.oauth.prod.cn\0<sha256>`。
pub fn record_key(auth: &Value, region: Region) -> Result<String, String> {
    let service = region.namespace_service();
    records_map(auth)?
        .keys()
        .find(|key| {
            key.strip_prefix(&service)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('\0'))
        })
        .cloned()
        .ok_or_else(|| {
            format!(
                "MiniMax {} 凭据里没有本区域登录记录，请先在客户端登录",
                region.label()
            )
        })
}

fn record(auth: &Value, region: Region) -> Result<&Map<String, Value>, String> {
    let key = record_key(auth, region)?;
    records_map(auth)?
        .get(&key)
        .and_then(Value::as_object)
        .ok_or_else(|| "MiniMax 凭据记录损坏".to_string())
}

fn record_mut(auth: &mut Value, region: Region) -> Result<&mut Map<String, Value>, String> {
    let key = record_key(auth, region)?;
    auth.get_mut("records")
        .and_then(Value::as_object_mut)
        .and_then(|records| records.get_mut(&key))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "MiniMax 凭据记录损坏".to_string())
}

fn non_empty(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn state_object(state: &Value, region: Region) -> Result<&Map<String, Value>, String> {
    let state = state
        .as_object()
        .ok_or_else(|| "MiniMax 登录状态不是对象".to_string())?;
    let version = state
        .get("schemaVersion")
        .and_then(Value::as_i64)
        .ok_or_else(|| "MiniMax 登录状态缺少版本".to_string())?;
    if !(1..=STATE_SCHEMA_VERSION).contains(&version) {
        return Err("MiniMax 登录状态版本不受支持，拒绝读写".into());
    }
    if state.get("storeKind").and_then(Value::as_str) != Some("file") {
        return Err("MiniMax 登录状态不是文件存储，拒绝改写".into());
    }
    if let Some(value) = state.get("region").and_then(Value::as_str) {
        if value != region.text() {
            return Err("MiniMax 登录状态区域与目标不一致".into());
        }
    }
    if let Some(value) = state.get("buildEnv").and_then(Value::as_str) {
        if value != "prod" {
            return Err("MiniMax 登录状态不是正式环境，拒绝改写".into());
        }
    }
    Ok(state)
}

/// 读取并校验一份账号快照。
pub fn parse(payload: &Payload, region: Region) -> Result<Credential, String> {
    let record = record(&payload.auth, region)?;
    if record.get("schemaVersion").and_then(Value::as_i64) != Some(AUTH_SCHEMA_VERSION) {
        return Err("MiniMax 凭据记录版本不受支持".into());
    }
    let access_token = non_empty(record, "accessToken").ok_or("MiniMax 凭据缺少访问令牌")?;
    let refresh_token =
        non_empty(record, "refreshToken").ok_or("MiniMax 凭据缺少刷新令牌，请重新登录后再保存")?;
    let expires_at_ms = record
        .get("expiresAtMs")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or("MiniMax 凭据缺少有效期")?;
    let generation = record
        .get("generation")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let login_epoch = non_empty(record, "loginEpoch");

    let state = state_object(&payload.state, region)?;
    match state.get("status").and_then(Value::as_str) {
        Some("authenticated") | Some("expired") | Some("refreshing") => {}
        Some("anonymous") | Some("authorizing") => {
            return Err("MiniMax 账号快照不是有效登录，请先在客户端完成登录".into())
        }
        Some("scope_upgrade_required") => {
            return Err("MiniMax 账号需要升级授权范围，请在客户端重新登录".into())
        }
        Some("logging_out") | Some("logout_pending") => {
            return Err("MiniMax 账号正在退出登录，请稍后重新保存".into())
        }
        Some("error") => return Err("MiniMax 客户端把该登录判为错误状态，请重新登录".into()),
        _ => return Err("MiniMax 登录状态缺少 status".into()),
    }
    Ok(Credential {
        access_token,
        refresh_token,
        expires_at_ms,
        generation,
        login_epoch,
    })
}

/// 代际取「账号包、现场、当前最大」的下一号，保证旧代际永远不会再被刷新。
fn next_generation(payload: &Payload, region: Region, live_generation: i64) -> Result<i64, String> {
    let record = record(&payload.auth, region)?;
    let credential_generation = record
        .get("generation")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let state_generation = payload
        .state
        .get("generation")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    Ok(credential_generation
        .max(state_generation)
        .max(live_generation)
        .max(0)
        + 1)
}

fn sync_state(
    payload: &mut Payload,
    region: Region,
    generation: i64,
    expires_at_ms: i64,
) -> Result<(), String> {
    let state = payload
        .state
        .as_object_mut()
        .ok_or_else(|| "MiniMax 登录状态不是对象".to_string())?;
    let scopes = record(&payload.auth, region)?
        .get("scopes")
        .cloned()
        .unwrap_or_else(|| json!(["agent.default"]));
    state.insert("schemaVersion".into(), json!(STATE_SCHEMA_VERSION));
    state.insert("status".into(), json!("authenticated"));
    state.insert("storeKind".into(), json!("file"));
    state.insert("clientId".into(), json!("mcode-public"));
    state.insert("scopes".into(), scopes);
    state.insert("audience".into(), json!("agent-backend"));
    state.insert("buildEnv".into(), json!("prod"));
    state.insert("region".into(), json!(region.text()));
    state.insert("expiresAtMs".into(), json!(expires_at_ms));
    state.insert("generation".into(), json!(generation));
    // 刷新事务属于上一个进程，代际一变就必须作废，否则客户端会等一个不存在的提交。
    state.remove("refresh");
    state.remove("authorization");
    Ok(())
}

/// 生成可写入现场的凭据组：提升代际、清事务、状态归位。
pub fn prepare(
    payload: &Payload,
    region: Region,
    live_generation: i64,
) -> Result<(Payload, Credential), String> {
    let credential = parse(payload, region)?;
    let generation = next_generation(payload, region, live_generation)?;
    let mut out = payload.clone();
    {
        let record = record_mut(&mut out.auth, region)?;
        record.insert("generation".into(), json!(generation));
        // 老快照可能缺少该字段；写入时补齐，保证客户端能区分新旧登录。
        if credential.login_epoch.is_none() {
            record.insert("loginEpoch".into(), json!(uuid::Uuid::new_v4().to_string()));
        }
    }
    sync_state(&mut out, region, generation, credential.expires_at_ms)?;
    let credential = parse(&out, region)?;
    Ok((out, credential))
}

/// 把刷新结果写回账号包（只用于非现场的账号副本）。
pub fn apply_grant(
    payload: &Payload,
    region: Region,
    access_token: &str,
    refresh_token: &str,
    expires_at_ms: i64,
    live_generation: i64,
) -> Result<(Payload, Credential), String> {
    let mut out = payload.clone();
    let generation = next_generation(payload, region, live_generation)?;
    {
        let record = record_mut(&mut out.auth, region)?;
        record.insert("accessToken".into(), json!(access_token));
        record.insert("refreshToken".into(), json!(refresh_token));
        record.insert("expiresAtMs".into(), json!(expires_at_ms));
        record.insert("generation".into(), json!(generation));
    }
    sync_state(&mut out, region, generation, expires_at_ms)?;
    let credential = parse(&out, region)?;
    Ok((out, credential))
}

/// 交互式登录（工具内设备码授权）得到的全新凭据组。
///
/// 代际从 1 开始、`loginEpoch` 是新生成的，两份文件同代际，符合客户端「一次登录一组」的约定。
/// `key` 必须是 [`super::paths::Paths::credential_key`] 算出的键，否则客户端会认为本区域没有登录。
pub fn fresh(
    region: Region,
    key: &str,
    access_token: &str,
    refresh_token: &str,
    expires_at_ms: i64,
    login_epoch: &str,
) -> Payload {
    let mut records = Map::new();
    records.insert(
        key.to_string(),
        json!({
            "schemaVersion": AUTH_SCHEMA_VERSION,
            "accessToken": access_token,
            "refreshToken": refresh_token,
            "tokenType": "Bearer",
            "clientId": "mcode-public",
            "scopes": ["agent.default"],
            "audience": "agent-backend",
            "expiresAtMs": expires_at_ms,
            "generation": 1,
            "loginEpoch": login_epoch,
        }),
    );
    Payload {
        auth: json!({"schemaVersion": AUTH_SCHEMA_VERSION, "records": records}),
        state: json!({
            "schemaVersion": STATE_SCHEMA_VERSION,
            "status": "authenticated",
            "storeKind": "file",
            "clientId": "mcode-public",
            "scopes": ["agent.default"],
            "audience": "agent-backend",
            "buildEnv": "prod",
            "region": region.text(),
            "expiresAtMs": expires_at_ms,
            "generation": 1,
        }),
    }
}

/// 校验写入后的两份文件确实属于同一代际。
pub fn verify_written(auth: &Value, state: &Value, region: Region) -> Result<i64, String> {
    let credential = parse(
        &Payload {
            auth: auth.clone(),
            state: state.clone(),
        },
        region,
    )?;
    let state_generation = state
        .get("generation")
        .and_then(Value::as_i64)
        .ok_or("写入后的登录状态缺少代际")?;
    if credential.generation != state_generation {
        return Err("写入后的凭据代际不一致，已回滚".into());
    }
    Ok(state_generation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(region: Region, generation: i64, status: &str) -> Payload {
        let key = format!("{}\u{0}abc", region.namespace_service());
        let mut records = Map::new();
        records.insert(
            key,
            json!({
                "schemaVersion": 1,
                "accessToken": "mmoat_token",
                "refreshToken": "mmort_token",
                "tokenType": "Bearer",
                "clientId": "mcode-public",
                "scopes": ["agent.default"],
                "audience": "agent-backend",
                "expiresAtMs": 1_800_000_000_000i64,
                "generation": generation,
                "loginEpoch": "epoch-1"
            }),
        );
        Payload {
            auth: json!({
                "schemaVersion": 1,
                "records": Value::Object(records)
            }),
            state: json!({
                "schemaVersion": 2,
                "status": status,
                "storeKind": "file",
                "clientId": "mcode-public",
                "scopes": ["agent.default"],
                "audience": "agent-backend",
                "buildEnv": "prod",
                "region": region.text(),
                "expiresAtMs": 1_800_000_000_000i64,
                "generation": generation,
                "refresh": {"id": "stale", "owner": {"pid": 1, "startedAtMs": 2}},
                "authorization": {"leaseId": "x", "leaseExpiresAtMs": 1}
            }),
        }
    }

    #[test]
    fn prepare_bumps_generation_and_clears_transactions() {
        let region = Region::Cn;
        let live_gen = 7;
        let (out, credential) =
            prepare(&payload(region, 3, "authenticated"), region, live_gen).unwrap();
        assert_eq!(credential.generation, 8);
        assert_eq!(out.state["generation"], 8);
        assert_eq!(out.state["status"], "authenticated");
        assert_eq!(out.state["expiresAtMs"], 1_800_000_000_000i64);
        assert!(out.state.get("refresh").is_none());
        assert!(out.state.get("authorization").is_none());
        assert_eq!(credential.epoch(), "epoch-1");
    }

    #[test]
    fn prepare_rejects_incomplete_or_foreign_credentials() {
        let region = Region::Cn;
        let mut missing_refresh = payload(region, 1, "authenticated");
        let key = record_key(&missing_refresh.auth, region).unwrap();
        missing_refresh.auth["records"][key.as_str()]
            .as_object_mut()
            .unwrap()
            .remove("refreshToken");
        assert!(prepare(&missing_refresh, region, 0).is_err());

        let anonymous = payload(region, 1, "anonymous");
        assert!(prepare(&anonymous, region, 0).is_err());

        let other_region = payload(Region::En, 1, "authenticated");
        assert!(prepare(&other_region, Region::Cn, 0).is_err());
    }

    #[test]
    fn prepare_requires_matching_state_versions_and_store_kind() {
        let region = Region::Cn;
        let mut keyring = payload(region, 1, "authenticated");
        keyring.state["storeKind"] = json!("os-keyring");
        assert!(prepare(&keyring, region, 0).is_err());

        let mut wrong_region = payload(region, 1, "authenticated");
        wrong_region.state["region"] = json!("en");
        assert!(prepare(&wrong_region, region, 0).is_err());
    }

    #[test]
    fn apply_grant_replaces_tokens_and_keeps_epoch() {
        let region = Region::Cn;
        let (out, credential) = apply_grant(
            &payload(region, 2, "authenticated"),
            region,
            "mmoat_new",
            "mmort_new",
            1_900_000_000_000,
            4,
        )
        .unwrap();
        assert_eq!(credential.access_token, "mmoat_new");
        assert_eq!(credential.refresh_token, "mmort_new");
        assert_eq!(credential.generation, 5);
        assert_eq!(credential.expires_at_ms, 1_900_000_000_000);
        assert_eq!(credential.epoch(), "epoch-1");
        assert_eq!(out.state["expiresAtMs"], 1_900_000_000_000i64);
    }

    #[test]
    fn fresh_login_is_consistent_and_uses_the_requested_key() {
        let region = Region::Cn;
        let key = format!("{}\u{0}account-digest", region.namespace_service());
        let payload = fresh(
            region,
            &key,
            "mmoat_fresh",
            "mmort_fresh",
            1_900_000_000_000,
            "epoch-fresh",
        );
        assert_eq!(record_key(&payload.auth, region).unwrap(), key);
        let credential = parse(&payload, region).unwrap();
        assert_eq!(credential.generation, 1);
        assert_eq!(payload.state["generation"], 1);
        assert_eq!(credential.epoch(), "epoch-fresh");
        assert_eq!(credential.access_token, "mmoat_fresh");
        // 换到另一个区域时必须不被当成同一份登录。
        assert!(parse(&payload, Region::En).is_err());
    }

    #[test]
    fn verify_written_detects_generation_mismatch() {
        let region = Region::Cn;
        let (out, _) = prepare(&payload(region, 1, "authenticated"), region, 0).unwrap();
        assert!(verify_written(&out.auth, &out.state, region).is_ok());
        let mut broken = out.state.clone();
        broken["generation"] = json!(1);
        assert!(verify_written(&out.auth, &broken, region).is_err());
    }
}
