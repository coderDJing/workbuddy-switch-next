//! Decode the client's existing authentication envelope. Never synthesize a
//! session or copy project/session databases. Protocol cross-check: cockpit-tools
//! 02498ace, trae_account_core_platform_storage/import (see research note).
use aes::Aes128;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use serde_json::{Map, Value};
use sha2::{Digest, Sha512};
include!("crypto_constants.rs");

pub fn decode(value: &Value) -> Result<Value, String> {
    if value.is_object() {
        return Ok(value.clone());
    }
    let text = value.as_str().ok_or("无法读取 TRAE 登录数据")?;
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return Ok(value);
    }
    let raw = STANDARD
        .decode(text)
        .map_err(|_| "TRAE 登录数据编码不支持")?;
    if raw.len() < 54 || (raw.len() - 38) % 16 != 0 {
        return Err("TRAE 登录数据长度不合法".into());
    }
    let salt = match &raw[..6] {
        [116, 99, 5, 16, 0, 0] => &SALT_AES,
        [18, 57, 32, 32, 2, 3] => &SALT_PRIVATE,
        _ => return Err("TRAE 登录数据格式已变化，请更新本工具".into()),
    };
    let mut material = Sha512::digest(&raw[6..38]).to_vec();
    material.extend_from_slice(salt);
    let derived = Sha512::digest(&material);
    let mut cipher = raw[38..].to_vec();
    let plain = cbc::Decryptor::<Aes128>::new_from_slices(&derived[..16], &derived[16..32])
        .map_err(|_| "无法准备 TRAE 解码")?
        .decrypt_padded_mut::<Pkcs7>(&mut cipher)
        .map_err(|_| "无法解码 TRAE 登录数据")?;
    if plain.len() < 64 || Sha512::digest(&plain[64..]).as_slice() != &plain[..64] {
        return Err("TRAE 登录数据校验失败".into());
    }
    serde_json::from_slice(&plain[64..]).map_err(|_| "TRAE 登录数据不是有效 JSON".into())
}

pub fn owned_key(key: &str) -> bool {
    key.starts_with("iCubeAuthInfo://")
        || key.starts_with("iCubeServerData://")
        || key.starts_with("iCubeEntitlementInfo://")
}
pub fn auth_keys(root: &Value) -> Result<Map<String, Value>, String> {
    let root = root.as_object().ok_or("TRAE storage.json 必须是对象")?;
    Ok(root
        .iter()
        .filter(|(k, _)| owned_key(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect())
}
pub fn pick(root: &Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        let v = root.pointer(path)?;
        v.as_str()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| v.as_u64().map(|v| v.to_string()))
    })
}
pub struct Auth {
    pub uid: String,
    pub name: String,
    pub email: Option<String>,
    pub token: String,
    pub expires: Option<i64>,
    pub region: Option<String>,
}
pub fn identity(keys: &Map<String, Value>) -> Result<Auth, String> {
    let candidates: Vec<_> = keys
        .iter()
        .filter(|(k, _)| {
            k.starts_with("iCubeAuthInfo://") && !k.contains("icube-dc:") && !k.ends_with("usertag")
        })
        .collect();
    let mut matches = Vec::new();
    for (_, value) in candidates {
        let decoded = decode(value)?;
        let Some(token) = pick(
            &decoded,
            &[
                "/accessToken",
                "/access_token",
                "/token",
                "/data/accessToken",
                "/auth/accessToken",
            ],
        ) else {
            continue;
        };
        let claims = token
            .split('.')
            .nth(1)
            .and_then(|v| URL_SAFE_NO_PAD.decode(v).ok())
            .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
            .unwrap_or(Value::Null);
        let jwt_uid = pick(&claims, &["/data/user_id", "/data/id", "/user_id"]);
        let local_uid = pick(
            &decoded,
            &[
                "/userId",
                "/user_id",
                "/uid",
                "/data/userId",
                "/user/id",
                "/account/uid",
            ],
        );
        if jwt_uid
            .as_ref()
            .zip(local_uid.as_ref())
            .is_some_and(|(a, b)| a != b)
        {
            return Err("TRAE 登录身份不一致，请先在客户端重新登录".into());
        }
        let uid = jwt_uid
            .or(local_uid)
            .ok_or("TRAE 登录数据缺少可靠账号 ID")?;
        let name = pick(
            &decoded,
            &[
                "/nickname",
                "/name",
                "/displayName",
                "/account/username",
                "/account/name",
                "/user/name",
            ],
        )
        .unwrap_or_else(|| {
            format!(
                "TRAE …{}",
                uid.chars()
                    .rev()
                    .take(4)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            )
        });
        matches.push(Auth {
            uid,
            name,
            email: pick(&decoded, &["/email", "/account/email", "/user/email"]),
            token,
            expires: claims["exp"].as_i64(),
            region: pick(
                &decoded,
                &[
                    "/userRegion/_aiRegion",
                    "/userRegion/region",
                    "/storeRegion",
                    "/aiRegion",
                    "/region",
                    "/account/storeRegion",
                ],
            ),
        });
    }
    if matches.len() != 1 {
        return Err("TRAE 登录提供方缺失或有多个身份，未猜测当前账号".into());
    }
    Ok(matches.remove(0))
}

pub fn patch(root: &Value, keys: &Map<String, Value>) -> Result<Value, String> {
    identity(keys)?;
    let mut root = root
        .as_object()
        .ok_or("TRAE storage.json 已损坏，未写入")?
        .clone();
    root.retain(|k, _| !owned_key(k));
    for (k, v) in keys {
        if !owned_key(k) {
            return Err("快照包含非登录字段".into());
        }
        root.insert(k.clone(), v.clone());
    }
    Ok(Value::Object(root))
}
