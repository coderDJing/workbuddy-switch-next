use super::*;
use base64::{engine::general_purpose::STANDARD,Engine as _};
const FORMAT:&str="wb-switch-doubao-v1";
#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Portable { name:String, identity:Identity }
fn records(v:&Value)->Result<Vec<Portable>,String>{
    if v["format"]!=FORMAT{return Err("不是豆包加密账号备份".into());}
    let list=v["accounts"].as_array().filter(|a|a.len()<=1000).ok_or("豆包备份账号列表无效")?;
    let mut ids=std::collections::HashSet::new();let mut out=vec![];
    for item in list {
        let p:Portable=serde_json::from_value(item.clone()).map_err(|_|"豆包备份字段无效")?;
        if p.identity.uid.is_empty()||p.identity.uid.len()>64||!p.identity.uid.bytes().all(|b|b.is_ascii_digit())||p.name.trim().is_empty()||p.name.chars().count()>80||p.identity.name.chars().count()>80||!ids.insert(p.identity.uid.clone())||p.identity.session.len()<8||p.identity.session.len()>512||!p.identity.session.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')||!profile::profile_name(&p.identity.profile){return Err("豆包备份账号校验失败".into());}out.push(p);
    }Ok(out)
}
pub(crate) fn merge(local:Value,remote:Value)->Result<Value,String>{
    let mut all=records(&local)?;
    for p in records(&remote)? {if let Some(a)=all.iter().find(|a|a.identity.uid==p.identity.uid){if a.identity.session!=p.identity.session{return Err("同一豆包账号的两端授权不同，已停止同步，请确认需要保留的备份".into());}}else{all.push(p);}}
    let result=json!({"format":FORMAT,"accounts":all});records(&result)?;Ok(result)
}
impl Engine {
    pub(crate) fn backup_payload(&self)->Result<Value,String>{
        let _g=self.lock()?;let mut items=vec![];
        for a in self.load()?.accounts {items.push(Portable{name:a.name.clone(),identity:self.credential(&a)?});}
        let value=json!({"format":FORMAT,"accounts":items});records(&value)?;Ok(value)
    }
    pub(crate) fn preview_payload(&self,payload:&Value)->Result<Value,String>{
        let items=records(payload)?;let _g=self.lock()?;let s=self.load()?;
        Ok(json!({"accounts":items.iter().map(|p|json!({"name":p.name,"exists":s.accounts.iter().any(|a|a.uid==p.identity.uid)})).collect::<Vec<_>>(),"count":items.len(),"desktopSessionIncluded":false}))
    }
    pub(crate) fn import_payload(&self,payload:&Value)->Result<Value,String>{
        let items=records(payload)?;let _g=self.lock()?;let mut store=self.load()?;let(mut imported,mut skipped)=(0,0);
        for p in items {
            if store.accounts.iter().any(|a|a.uid==p.identity.uid){skipped+=1;continue;}
            let a=self.put(&mut store,&p.identity,None)?;
            let a=store.accounts.iter_mut().find(|v|v.id==a.id).ok_or("豆包账号导入失败")?;a.name=p.name;a.custom_name=true;imported+=1;
        }self.save(&store)?;Ok(json!({"ok":true,"imported":imported,"skipped":skipped,"desktopSessionIncluded":false}))
    }
    pub(super) fn export_backup(&self,passphrase:&str,path:Option<&str>)->Result<Value,String>{
        let payload=self.backup_payload()?;let count=payload["accounts"].as_array().map_or(0,Vec::len);
        if count==0{return Err("没有可导出的豆包账号".into());}
        let bytes=crate::modules::webdav_sync::build_envelope(&payload,passphrase,now()*1000)?;let data=STANDARD.encode(bytes);
        if let Some(path)=path {
            use std::io::Write;let mut options=std::fs::OpenOptions::new();options.write(true).create_new(true);
            #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
            let mut file=options.open(path).map_err(|_|"无法创建备份，请选择未存在的文件名")?;
            if file.write_all(data.as_bytes()).and_then(|_|file.sync_all()).is_err(){drop(file);let _=std::fs::remove_file(path);return Err("豆包备份写入失败".into());}
            Ok(json!({"count":count,"path":path}))
        }else{Ok(json!({"count":count,"data":data}))}
    }
    pub(super) fn preview_backup(&self,data:&str,passphrase:&str)->Result<Value,String>{
        let bytes=decode(data)?;let(_,payload)=crate::modules::webdav_sync::decrypt_envelope(&bytes,passphrase)?;let mut result=self.preview_payload(&payload)?;result["snapshotId"]=json!(hash(&bytes));Ok(result)
    }
    pub(super) fn import_backup(&self,data:&str,passphrase:&str,snapshot:&str)->Result<Value,String>{
        let bytes=decode(data)?;if hash(&bytes)!=snapshot{return Err("备份在预览后发生变化，请重新预览".into());}
        let(_,payload)=crate::modules::webdav_sync::decrypt_envelope(&bytes,passphrase)?;self.import_payload(&payload)
    }
}
fn decode(data:&str)->Result<Vec<u8>,String>{if data.len()>16*1024*1024{return Err("豆包备份超过大小上限".into());}STANDARD.decode(data.trim()).map_err(|_|"豆包备份编码无效".into())}
