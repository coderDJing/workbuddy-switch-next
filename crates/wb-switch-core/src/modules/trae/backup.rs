//! Portable exports use the existing authenticated, password-protected WBS envelope.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
const FORMAT: &str = "wb-switch-trae-v1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Portable {
    uid: String,
    name: String,
    variant: Region,
    auth: Map<String, Value>,
}
fn records(payload: &Value) -> Result<Vec<Portable>, String> {
    if payload["format"] != FORMAT {
        return Err("不是 TRAE 加密账号备份".into());
    }
    let list = payload["accounts"]
        .as_array()
        .filter(|list| list.len() <= 1000)
        .ok_or("备份账号列表无效或过大")?;
    let mut found = std::collections::HashSet::new();
    let mut result = Vec::new();
    for entry in list {
        let item: Portable =
            serde_json::from_value(entry.clone()).map_err(|_| "TRAE 备份记录格式无效")?;
        let auth = protocol::identity(&item.auth)?;
        if item.uid != auth.uid
            || item.name.trim().is_empty()
            || item.name.chars().count() > 80
            || item.auth.keys().any(|key| !protocol::owned_key(key))
            || !found.insert(format!("{}:{}", item.variant.text(), item.uid))
        {
            return Err("TRAE 备份账号校验失败".into());
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
        if !all
            .iter()
            .any(|a| a.uid == item.uid && a.variant == item.variant)
        {
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
                variant: a.variant,
                auth: self.keys(a)?,
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
            json!({"accounts":items.iter().map(|a|json!({"name":a.name,"variant":a.variant,"exists":store.accounts.iter().any(|b|a.uid==b.uid&&a.variant==b.variant)})).collect::<Vec<_>>(),"count":items.len()}),
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
                .any(|b| b.uid == a.uid && b.variant == a.variant)
            {
                skipped += 1;
                continue;
            }
            let id = self.upsert(&mut store, a.variant, Client::Desktop, &a.auth)?;
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
            return Err("没有可备份的 TRAE 账号".into());
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
