use crate::{model::Instance,workspace::{self,Profile}};
use anyhow::{ensure,Context,Result};
use serde::{Serialize,Deserialize};
use std::{collections::{BTreeMap,BTreeSet},path::Path,time::{SystemTime,UNIX_EPOCH}};
use uuid::Uuid;

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
pub struct SnapshotMeta{
    pub id:String,pub profile_id:String,pub profile_name:String,pub label:String,
    pub created:u64,pub services:Vec<String>,pub all_groups:bool,pub count:usize,
}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct Snapshot{pub meta:SnapshotMeta,pub rows:Vec<Instance>}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ref{pub profile_id:String,pub snapshot_id:String}
fn data_name(id:&str)->String{format!("snapshot-{id}.json")}
fn meta_name(id:&str)->String{format!("snapshot-{id}.meta.json")}
pub fn save(dir:&Path,profile:&Profile,label:&str,services:Vec<String>,all_groups:bool,rows:Vec<Instance>)->Result<SnapshotMeta>{
    ensure!(!label.trim().is_empty()&&label.len()<=128,"snapshot name must be 1..128 bytes");
    ensure!(rows.iter().all(|r|services.contains(&r.name)),"rows outside snapshot scope");
    let id=Uuid::new_v4().to_string();
    let meta=SnapshotMeta{id:id.clone(),profile_id:profile.id.clone(),profile_name:profile.name.clone(),label:label.trim().into(),created:SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),services,all_groups,count:rows.len()};
    let data=dir.join(data_name(&id));
    workspace::write_json(&data,&Snapshot{meta:meta.clone(),rows})?;
    if let Err(e)=workspace::write_json(&dir.join(meta_name(&id)),&meta){let _=std::fs::remove_file(&data);return Err(e);}
    Ok(meta)
}
pub fn list(root:&Path,profiles:&[Profile])->Result<Vec<SnapshotMeta>>{
    let mut result=Vec::new();
    for p in profiles{
        let dir=root.join(&p.id);
        if !dir.exists(){continue;}
        let dir=workspace::profile_dir(root,&p.id)?;
        for file in std::fs::read_dir(dir)?{
            let file=file?;
            if !file.file_type()?.is_file(){continue;}
            let name=file.file_name();let name=name.to_string_lossy();
            if let Some(id)=name.strip_prefix("snapshot-").and_then(|x|x.strip_suffix(".meta.json")){
                if workspace::valid_id(id){
                    let m:SnapshotMeta=workspace::read_json(&file.path())?;
                    ensure!(m.id==id&&m.profile_id==p.id,"snapshot metadata mismatch");
                    result.push(m);
                }
            }
        }
    }
    result.sort_by(|a,b|b.created.cmp(&a.created).then_with(||b.id.cmp(&a.id)));Ok(result)
}
pub fn load(root:&Path,profiles:&[Profile],reference:&Ref)->Result<Snapshot>{
    ensure!(workspace::valid_id(&reference.snapshot_id)&&profiles.iter().any(|p|p.id==reference.profile_id),"unknown snapshot reference");
    let dir=workspace::profile_dir(root,&reference.profile_id)?;
    let m:SnapshotMeta=workspace::read_json(&dir.join(meta_name(&reference.snapshot_id))).context("snapshot not found")?;
    let s:Snapshot=workspace::read_json(&dir.join(data_name(&reference.snapshot_id)))?;
    ensure!(s.meta.id==reference.snapshot_id&&s.meta.profile_id==reference.profile_id&&m==s.meta&&m.count==s.rows.len(),"snapshot identity or count mismatch");
    ensure!(s.rows.iter().all(|r|s.meta.services.contains(&r.name)),"snapshot rows outside recorded service scope");
    Ok(s)
}
// None = full catalog; Some limits target-only rows but never hides global ID conflicts.
pub fn select(source:&Snapshot,services:&[String])->Result<(Vec<Instance>,Option<BTreeSet<String>>)>{
    for g in services{ensure!(source.meta.services.contains(g),"source snapshot does not cover service {g}");}
    let scope=if !services.is_empty(){Some(services.iter().cloned().collect::<BTreeSet<_>>())}else if source.meta.all_groups{None}else{Some(source.meta.services.iter().cloned().collect())};
    let rows=source.rows.iter().filter(|r|scope.as_ref().is_none_or(|s|s.contains(&r.name))).cloned().collect();Ok((rows,scope))
}
#[derive(Debug,Serialize)]
pub struct Entry{#[serde(rename="ID")]pub id:String,pub status:String,pub changed_fields:Vec<String>,pub source:Vec<Instance>,pub target:Vec<Instance>}
#[derive(Debug,Serialize)]
pub struct Diff{pub counts:BTreeMap<String,usize>,pub entries:Vec<Entry>}
pub fn compare(source:&[Instance],target:&[Instance],scope:Option<&BTreeSet<String>>)->Diff{
    let mut src:BTreeMap<&str,Vec<&Instance>>=BTreeMap::new();let mut dst:BTreeMap<&str,Vec<&Instance>>=BTreeMap::new();
    for r in source{src.entry(&r.id).or_default().push(r);}
    for r in target{dst.entry(&r.id).or_default().push(r);}
    let ids=src.keys().chain(dst.keys()).copied().collect::<BTreeSet<_>>();
    let mut counts=BTreeMap::new();let mut entries=Vec::new();
    for id in ids{
        let a=src.get(id).map(Vec::as_slice).unwrap_or(&[]);let b=dst.get(id).map(Vec::as_slice).unwrap_or(&[]);
        if a.is_empty()&&scope.is_some_and(|s|!b.iter().any(|r|s.contains(&r.name))){continue;}
        let mut fields=Vec::new();
        let status=match (a,b){
            (a,b) if a.len()>1||b.len()>1=>"ambiguous",
            ([_],[])=>"missing_in_target",
            ([],[_])=>"target_only",
            ([x],[y])=>{
                if x.name!=y.name{fields.push("name".to_owned());}if x.address!=y.address{fields.push("address".to_owned());}if x.port!=y.port{fields.push("port".to_owned());}
                if x.tags!=y.tags{fields.push("tags".to_owned());}if x.meta!=y.meta{fields.push("meta".to_owned());}
                if fields.is_empty(){"unchanged"}else{"changed"}
            },_=>unreachable!(),
        };
        *counts.entry(status.to_owned()).or_insert(0)+=1;
        entries.push(Entry{id:id.into(),status:status.into(),changed_fields:fields,source:a.iter().map(|r|(**r).clone()).collect(),target:b.iter().map(|r|(**r).clone()).collect()});
    }
    Diff{counts,entries}
}
#[cfg(test)]
mod tests{
    use super::*;
    fn row(id:&str,group:&str)->Instance{Instance{id:id.into(),name:group.into(),address:"192.0.2.1".into(),port:161,tags:vec![],meta:Default::default()}}
    #[test]fn diff_cross_group(){let a=vec![row("same","a"),row("missing","a"),row("changed","a")];let b=vec![row("same","a"),row("changed","b"),row("extra","a"),row("unrelated","b")];let scope=BTreeSet::from(["a".into()]);let d=compare(&a,&b,Some(&scope));assert_eq!(d.entries.len(),4);assert_eq!(d.counts["changed"],1);assert_eq!(d.counts["missing_in_target"],1);assert_eq!(d.counts["target_only"],1);assert_eq!(d.counts["unchanged"],1);assert_eq!(d.entries.iter().find(|e|e.id=="changed").unwrap().changed_fields,vec!["name"]);}
    #[test]fn ambiguous_ids(){let d=compare(&[row("same","a"),row("same","b")],&[row("same","a")],None);assert_eq!(d.counts["ambiguous"],1);}
}
