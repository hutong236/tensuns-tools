use anyhow::{ensure,Context,Result};
use serde::{Deserialize,Serialize,de::DeserializeOwned};
use std::{path::{Path,PathBuf},io::Write,collections::BTreeMap};
use uuid::Uuid;
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {pub id:String,pub name:String,pub url:String,pub username:String}
pub fn read_json<T:DeserializeOwned>(path:&Path)->Result<T>{Ok(serde_json::from_reader(std::fs::File::open(path)?)?)}
pub fn write_json<T:Serialize>(path:&Path,value:&T)->Result<()>{
    let mut tmp=tempfile::NamedTempFile::new_in(path.parent().context("missing parent")?)?;
    serde_json::to_writer_pretty(&mut tmp,value)?;writeln!(tmp)?;tmp.as_file().sync_all()?;tmp.persist(path)?;Ok(())
}
pub fn validate_url(url:&str)->Result<String>{
    let parsed=reqwest::Url::parse(url)?;
    ensure!(matches!(parsed.scheme(),"http"|"https")&&parsed.host_str().is_some()&&parsed.username().is_empty()&&parsed.password().is_none()&&parsed.query().is_none()&&parsed.fragment().is_none(),"invalid TenSunS Web URL");
    let url=parsed.as_str().trim_end_matches('/').to_owned();ensure!(!url.ends_with("/api")&&!url.ends_with("/v1"),"use Web base URL");Ok(url)
}
pub fn valid_id(id:&str)->bool{Uuid::parse_str(id).is_ok_and(|x|x.to_string()==id)}
pub struct Profiles{file:PathBuf,pub items:Vec<Profile>,pub default_id:String}
impl Profiles{
    pub fn open(root:&Path,url:&str,username:&str)->Result<Self>{
        let file=root.join("profiles.json");let url=validate_url(url)?;
        let mut items:Vec<Profile>=if file.exists(){read_json(&file)?}else{Vec::new()};
        let mut ids=std::collections::HashSet::new();
        for p in &items{ensure!(valid_id(&p.id)&&ids.insert(&p.id),"invalid profile ID");validate_url(&p.url)?;}
        let default_id=if let Some(p)=items.iter().find(|p|p.url==url&&p.username==username){p.id.clone()}else{
            let id=Uuid::new_v4().to_string();items.push(Profile{id:id.clone(),name:format!("TenSunS {}",items.len()+1),url,username:username.into()});write_json(&file,&items)?;id
        };
        Ok(Self{file,items,default_id})
    }
    pub fn get(&self,id:Option<&str>)->Result<Profile>{let id=id.unwrap_or(&self.default_id);self.items.iter().find(|p|p.id==id).cloned().context("unknown connection profile")}
    pub fn add(&mut self,name:String,url:String,username:String)->Result<Profile>{
        ensure!(!name.trim().is_empty()&&name.len()<=128&&!username.trim().is_empty()&&username.len()<=256,"invalid profile name or username");
        let url=validate_url(&url)?;
        ensure!(self.items.len()<100,"maximum 100 profiles");
        ensure!(!self.items.iter().any(|p|p.url==url&&p.username==username.trim()),"connection already exists");
        let p=Profile{id:Uuid::new_v4().to_string(),name:name.trim().into(),url,username:username.trim().into()};
        let mut next=self.items.clone();next.push(p.clone());write_json(&self.file,&next)?;self.items=next;Ok(p)
    }
}
pub fn profile_dir(root:&Path,id:&str)->Result<PathBuf>{ensure!(valid_id(id),"invalid profile ID");let path=root.join(id);std::fs::create_dir_all(&path)?;ensure!(!std::fs::symlink_metadata(&path)?.file_type().is_symlink(),"profile directory cannot be a symlink");Ok(path)}
pub fn report_counts(report:&str)->BTreeMap<String,usize>{
    let mut latest=BTreeMap::<String,String>::new();
    for line in report.lines(){if let Ok(v)=serde_json::from_str::<serde_json::Value>(line){if let(Some(id),Some(status))=(v["ID"].as_str(),v["status"].as_str()){latest.insert(id.into(),status.into());}}}
    let mut counts=BTreeMap::new();for status in latest.values(){*counts.entry(status.clone()).or_insert(0)+=1;}counts
}
#[derive(Clone,Serialize,Deserialize)]
pub struct Task {pub id:String,pub created:u64,pub state:String,pub exit_code:Option<i32>,#[serde(default)]pub source:Option<crate::snapshots::Ref>}
impl Task{
    pub fn new(id:String)->Self{Self{id,created:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),state:"running".into(),exit_code:None,source:None}}
    pub fn save(&self,dir:&Path)->Result<()>{write_json(&dir.join(format!("task-{}.json",self.id)),self)}
}
pub fn tasks(dir:&Path)->Result<Vec<Task>>{
    let mut result=Vec::new();
    for e in std::fs::read_dir(dir)?{let e=e?;if !e.file_type()?.is_file(){continue;}let file=e.file_name();let file=file.to_string_lossy();if let Some(id)=file.strip_prefix("task-").and_then(|x|x.strip_suffix(".json")){if valid_id(id){let mut t:Task=read_json(&e.path())?;ensure!(t.id==id,"task ID mismatch");if t.state=="running"{t.state="interrupted".into();}result.push(t);}}}
    result.sort_by(|a,b|b.created.cmp(&a.created).then_with(||b.id.cmp(&a.id)));Ok(result)
}
