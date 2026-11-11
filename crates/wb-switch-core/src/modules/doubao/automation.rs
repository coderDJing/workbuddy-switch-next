use super::*;
use std::collections::{BTreeMap,HashSet};
#[derive(Clone,Serialize,Deserialize)]
#[serde(default,rename_all="camelCase",deny_unknown_fields)]
pub struct Settings {
    pub refresh_enabled:bool,pub interval_minutes:u32,pub daily_enabled:bool,pub daily_time:String,
    pub alerts_enabled:bool,pub expiry_days:u32,pub low_balance:f64,
    pub rotate_enabled:bool,pub rotate_below:f64,pub cooldown_minutes:u32,
}
impl Default for Settings {fn default()->Self{Self{refresh_enabled:false,interval_minutes:30,daily_enabled:false,daily_time:"09:00".into(),alerts_enabled:false,expiry_days:7,low_balance:20.0,rotate_enabled:false,rotate_below:10.0,cooldown_minutes:30}}}
fn minute(v:&str)->Option<u32>{if v.len()!=5{return None;}let(h,m)=v.split_once(':')?;let(h,m):(u32,u32)=(h.parse().ok()?,m.parse().ok()?);(h<24&&m<60).then_some(h*60+m)}
impl Settings {
    fn validate(&self)->Result<(),String>{if !(5..=1440).contains(&self.interval_minutes)||!(5..=1440).contains(&self.cooldown_minutes)||self.expiry_days>90||!self.low_balance.is_finite()||!self.rotate_below.is_finite()||!(0.0..=100.0).contains(&self.low_balance)||!(0.0..=100.0).contains(&self.rotate_below)||minute(&self.daily_time).is_none(){return Err("豆包设置无效：额度阈值为 0–100%，间隔为 5–1440 分钟，时间为 HH:mm".into());}Ok(())}
    fn enabled(&self)->bool{self.refresh_enabled||self.daily_enabled||self.alerts_enabled||self.rotate_enabled}
}
#[derive(Default,Serialize,Deserialize)]
#[serde(default,deny_unknown_fields)]
struct State{checked:i64,rotated:i64,claims:BTreeMap<String,Attempt>,alerted:BTreeMap<String,i64>,logs:Vec<Value>}
#[derive(Default,Serialize,Deserialize)]
#[serde(default,deny_unknown_fields)]
struct Attempt{day:String,at:i64,count:u32,done:bool}
fn load<T:serde::de::DeserializeOwned+Default>(e:&Engine,file:&str)->Result<T,String>{let p=e.paths.store.join(file);if p.exists(){read(&p,2*1024*1024)}else{Ok(T::default())}}
fn settings(e:&Engine)->Result<Settings,String>{let s:Settings=load(e,"automation-config.json")?;s.validate()?;Ok(s)}
pub(super) fn status(e:&Engine)->Result<Value,String>{let state:State=load(e,"automation-state.json")?;Ok(json!({"settings":settings(e)?,"logs":state.logs.into_iter().rev().collect::<Vec<_>>()}))}
pub(super) fn save(e:&Engine,value:Value)->Result<Value,String>{let _g=StateLock::try_at(&e.paths.store.join("automation.lock")).map_err(|_|"豆包自动任务正在执行")?;let s:Settings=serde_json::from_value(value).map_err(|_|"豆包设置包含未知字段或错误类型")?;s.validate()?;write(&e.paths.store.join("automation-config.json"),&s)?;status(e)}
pub fn patch(value:Value)->Result<Value,String>{let e=Engine::real();let _g=StateLock::try_at(&e.paths.store.join("automation.lock")).map_err(|_|"豆包自动任务正在执行")?;let mut s=serde_json::to_value(settings(&e)?).map_err(|_|"无法读取豆包设置")?;for(k,v)in value.as_object().ok_or("设置须为 JSON 对象")?{s[k]=v.clone();}let s:Settings=serde_json::from_value(s).map_err(|_|"未知豆包设置字段")?;s.validate()?;write(&e.paths.store.join("automation-config.json"),&s)?;status(&e)}
fn log(s:&mut State,action:&str,outcome:&str){s.logs.push(json!({"at":now(),"action":action,"outcome":outcome}));if s.logs.len()>200{s.logs.drain(..s.logs.len()-200);}}
fn balance(a:&Account,now:i64,max_age:i64)->Option<f64>{if a.needs_login{return None;}let c=a.credits.as_ref()?;let at=c["checkedAt"].as_i64()?;if c["available"]!=true||at>now||now-at>max_age{return None;}c["remaining"].as_f64().filter(|v|v.is_finite()&&(0.0..=100.0).contains(v))}
pub(super) async fn run(e:&Engine)->Result<Value,String>{
    let _g=StateLock::try_at(&e.paths.store.join("automation.lock")).map_err(|_|"豆包自动任务正在执行")?;
    let c=settings(e)?;if !c.enabled(){return Ok(json!({"ran":false,"notices":[]}));}
    let mut s:State=load(e,"automation-state.json")?;let at=now();let day=chrono::Local::now().format("%Y-%m-%d").to_string();let mut notices=vec![];let mut errors=vec![];
    let accounts={let _g=e.lock()?;e.load()?.accounts};
    if c.daily_enabled&&minute(&chrono::Local::now().format("%H:%M").to_string()).unwrap_or(0)>=minute(&c.daily_time).unwrap_or(540){
        for a in &accounts {
            let due=s.claims.get(&a.id).is_none_or(|v|v.day!=day||(!v.done&&v.count<3&&at-v.at>=1800));if !due{continue;}
            let count=s.claims.get(&a.id).filter(|v|v.day==day).map_or(1,|v|v.count+1);
            s.claims.insert(a.id.clone(),Attempt{day:day.clone(),at,count,done:false});write(&e.paths.store.join("automation-state.json"),&s)?;
            let result=e.checkin(&a.id,true,None).await;
            let outcome=match &result{Ok(v)=>v["state"].as_str().unwrap_or("failed"),Err(_)=>"failed"};
            // An unknown claim receipt must not trigger another submission today.
            if matches!(outcome,"claimed"|"already"|"closed"|"unconfirmed"){if let Some(v)=s.claims.get_mut(&a.id){v.done=true;}}
            if outcome=="failed"{errors.push(json!({"accountId":a.id,"action":"checkin"}));}
            log(&mut s,"checkin",outcome);write(&e.paths.store.join("automation-state.json"),&s)?;
        }
    }
    if (c.refresh_enabled||c.alerts_enabled||c.rotate_enabled)&&at-s.checked>=c.interval_minutes as i64*60 {
        s.checked=at;write(&e.paths.store.join("automation-state.json"),&s)?;let mut failed=HashSet::new();
        for a in &accounts{if e.credit(&a.id,None).await.is_err(){failed.insert(a.id.clone());errors.push(json!({"accountId":a.id,"action":"credits"}));}}
        log(&mut s,"refresh",if failed.is_empty(){"updated"}else{"partial"});
        let accounts={let _g=e.lock()?;e.load()?.accounts};
        if c.alerts_enabled {for a in &accounts {
            let low=balance(a,at,c.interval_minutes as i64*120).is_some_and(|b|b<c.low_balance);
            let expiry=a.credits.as_ref().and_then(|v|v["planExpiresAt"].as_i64()).is_some_and(|t|t>at&&t-at<=c.expiry_days as i64*86400);
            for(kind,enabled,title)in[("low",low,"可用额度不足"),("expiry",expiry,"订阅即将到期"),("login",a.needs_login,"登录已过期"),("query",failed.contains(&a.id),"额度查询失败")] {
                let key=format!("{}:{kind}",a.id);if enabled&&s.alerted.get(&key).is_none_or(|v|at-v>=86400){let title=format!("豆包 {} · {title}",a.name);if crate::modules::notifications::record("warning",&title,Some("请前往豆包账号管理查看详情")).is_ok(){notices.push(title);s.alerted.insert(key,at);}}
            }
        }}
        if c.rotate_enabled&&at-s.rotated>=c.cooldown_minutes as i64*60 {
            let origin=e.origin()?;
            if let Some(current)=profile::live(&e.paths.data)?.and_then(|id|accounts.iter().find(|a|a.uid==id.uid)) {
                let current_balance=balance(current,at,c.interval_minutes as i64*120).filter(|_|!failed.contains(&current.id));
                if let Some(b)=current_balance.filter(|b|*b<c.rotate_below){
                    let next=accounts.iter().filter(|a|a.id!=current.id&&a.snapshot.is_some()&&a.origin.as_deref()==Some(origin.as_str())&&!failed.contains(&a.id)).filter_map(|a|balance(a,at,c.interval_minutes as i64*120).filter(|v|*v>b&&*v>c.rotate_below).map(|v|(a,v))).max_by(|a,b|a.1.total_cmp(&b.1));
                    if let Some((next,_))=next{let outcome=match e.switch(&next.id,false,false,false,Some(&current.uid)){Ok(_)=>{s.rotated=at;"switched"},Err(_)=>"deferred"};log(&mut s,"rotate",outcome);}
                }
            }
        }
    }
    write(&e.paths.store.join("automation-state.json"),&s)?;Ok(json!({"ran":true,"notices":notices,"errors":errors}))
}
pub async fn cycle()->Result<Value,String>{let runtime=tokio::runtime::Handle::current();tokio::task::spawn_blocking(move||runtime.block_on(run(&Engine::real()))).await.map_err(|_|"豆包自动任务异常结束".to_string())?}
