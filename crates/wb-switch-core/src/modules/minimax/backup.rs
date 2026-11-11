//! Portable exports use the existing authenticated, password-protected WBS envelope.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
const FORMAT: &str = "wb-switch-minimax-v1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Portable {
    uid: String,
    name: String,
    region: Region,
    email: Option<String>,
    auth: Payload,
}
fn records(payload: &Value) -> Result<Vec<Portable>, String> {
    if payload["format"] != FORMAT {
        return Err("不是 MiniMax 加密账号备份".into());
    }
    let list = payload["accounts"]
        .as_array()
        .filter(|list| list.len() <= 1000)
        .ok_or("备份账号列表无效或过大")?;
    let mut found = std::collections::HashSet::new();
    let mut result = Vec::new();
    for entry in list {
        let item: Portable =
            serde_json::from_value(entry.clone()).map_err(|_| "MiniMax 备份记录格式无效")?;
        let auth = protocol::parse(&item.auth, item.region)?;
        protocol::verify_written(&item.auth.auth, &item.auth.state, item.region)?;
        if item.uid.trim().is_empty()
            || item.uid.len() > 256
            || auth.epoch().is_empty()
            || item.name.trim().is_empty()
            || item.name.chars().count() > 80
            || !found.insert(format!("{}:{}", item.region.text(), item.uid))
        {
            return Err("MiniMax 备份账号校验失败".into());
        }
        result.push(item);
    }
    Ok(result)
}
fn decode(data: &str) -> Result<Vec<u8>, String> {
    if data.len() > 24 * 1024 * 1024 {
        return Err("备份文件过大".into());
    }
    STANDARD
        .decode(data.trim())
        .map_err(|_| "备份文件编码无效".into())
}
pub(crate) fn merge(local: Value, remote: Value) -> Result<Value, String> {
    let mut all = records(&local)?;
    let remote = records(&remote)?;
    for item in remote {
        if let Some(existing) = all
            .iter()
            .find(|a| a.uid == item.uid && a.region == item.region)
        {
            let left = protocol::parse(&existing.auth, existing.region)?;
            let right = protocol::parse(&item.auth, item.region)?;
            if left.access_token != right.access_token || left.refresh_token != right.refresh_token
            {
                return Err(
                    "同一账号的云端与本地授权不同，已停止同步；请先确认要保留的备份".into(),
                );
            }
        } else {
            all.push(item);
        }
    }
    let value = json!({"format":FORMAT,"accounts":all});
    records(&value)?;
    Ok(value)
}
impl Engine {
    pub(crate) fn backup_payload(&self) -> Result<Value, String> {
        let _guard = self.lock()?;
        let store = self.load()?;
        let mut accounts = Vec::new();
        for a in &store.accounts {
            accounts.push(Portable {
                uid: a.uid.clone(),
                name: a.name.clone(),
                region: a.region,
                email: a.email.clone(),
                auth: self.unseal(a)?,
            });
        }
        let payload = json!({"format":FORMAT,"accounts":accounts});
        records(&payload)?;
        Ok(payload)
    }
    pub(crate) fn preview_payload(&self, payload: &Value) -> Result<Value, String> {
        let items = records(payload)?;
        let _guard = self.lock()?;
        let store = self.load()?;
        Ok(
            json!({"accounts":items.iter().map(|a|json!({"name":a.name,"region":a.region,"exists":store.accounts.iter().any(|b|a.uid==b.uid&&a.region==b.region)})).collect::<Vec<_>>(),"count":items.len()}),
        )
    }
    pub(crate) fn import_payload(&self, payload: &Value) -> Result<Value, String> {
        let items = records(payload)?;
        let _guard = self.lock()?;
        let mut store = self.load()?;
        let mut imported = 0;
        let mut skipped = 0;
        for a in items {
            if store
                .accounts
                .iter()
                .any(|b| b.uid == a.uid && b.region == a.region)
            {
                skipped += 1;
                continue;
            }
            // Re-key the credential namespace for the destination computer.
            let credential = protocol::parse(&a.auth, a.region)?;
            let payload = protocol::fresh(
                a.region,
                &self.paths.credential_key(a.region),
                &credential.access_token,
                &credential.refresh_token,
                credential.expires_at_ms,
                &credential.epoch(),
            );
            let identity = Identity {
                uid: a.uid,
                name: Some(a.name.clone()),
                email: a.email,
            };
            let live = Live {
                credential: protocol::parse(&payload, a.region)?,
                payload,
            };
            let id = self
                .upsert(&mut store, a.region, &live, Some(&identity))?
                .id;
            store
                .accounts
                .iter_mut()
                .find(|b| b.id == id)
                .ok_or("账号导入失败")?
                .name = a.name;
            imported += 1;
        }
        self.save(&store)?;
        Ok(json!({"imported":imported,"skipped":skipped,"ok":true}))
    }
    pub(super) fn export_backup(
        &self,
        passphrase: &str,
        path: Option<&str>,
    ) -> Result<Value, String> {
        let payload = self.backup_payload()?;
        let count = payload["accounts"].as_array().map_or(0, Vec::len);
        if count == 0 {
            return Err("没有可备份的 MiniMax 账号".into());
        }
        let bytes = crate::modules::webdav_sync::build_envelope(
            &payload,
            passphrase,
            crate::modules::config::now_ms(),
        )?;
        let data = STANDARD.encode(bytes);
        if let Some(path) = path {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(path)
                .map_err(|_| "无法创建导出文件，请选择未存在的文件名")?;
            if file
                .write_all(data.as_bytes())
                .and_then(|_| file.sync_all())
                .is_err()
            {
                drop(file);
                let _ = std::fs::remove_file(path);
                return Err("导出文件写入失败".into());
            }
            return Ok(json!({"path":path,"count":count}));
        }
        Ok(json!({"data":data,"count":count}))
    }
    pub(super) fn preview_backup(&self, data: &str, passphrase: &str) -> Result<Value, String> {
        let bytes = decode(data)?;
        let (_, payload) = crate::modules::webdav_sync::decrypt_envelope(&bytes, passphrase)?;
        let mut preview = self.preview_payload(&payload)?;
        preview["snapshotId"] = json!(format!("{:x}", Sha256::digest(&bytes)));
        Ok(preview)
    }
    pub(super) fn import_backup(
        &self,
        data: &str,
        passphrase: &str,
        snapshot: &str,
    ) -> Result<Value, String> {
        let bytes = decode(data)?;
        if format!("{:x}", Sha256::digest(&bytes)) != snapshot {
            return Err("备份文件已变化，请重新预览".into());
        }
        let (_, payload) = crate::modules::webdav_sync::decrypt_envelope(&bytes, passphrase)?;
        self.import_payload(&payload)
    }
}
