use crate::{api::Api, model::Instance};
use anyhow::{ensure, Context, Result};
use serde_json::json;
use std::{collections::{BTreeMap, BTreeSet, HashMap}, fs::{File,OpenOptions}, io::Write, path::{Path,PathBuf}, thread, time::{Duration,SystemTime,UNIX_EPOCH}};

#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub(crate) enum Status { Planned, Unchanged, Conflict }
impl Status {pub(crate) fn as_str(self)->&'static str{match self{Self::Planned=>"planned",Self::Unchanged=>"unchanged",Self::Conflict=>"conflict"}}}
pub(crate) fn plan<'a>(wanted:&'a [Instance], old:&[Instance])->Vec<(&'a Instance,Status,Vec<String>)> {
    let mut index:HashMap<&str,Vec<&Instance>>=HashMap::new();
    for r in old{index.entry(&r.id).or_default().push(r);}
    wanted.iter().map(|r|{
        let matches=index.get(r.id.as_str()).map(Vec::as_slice).unwrap_or(&[]);
        let status=match matches {[]=>Status::Planned,[existing] if *existing==r=>Status::Unchanged,_=>Status::Conflict};
        let groups=matches.iter().map(|x|x.name.clone()).collect::<BTreeSet<_>>().into_iter().collect();
        (r,status,groups)
    }).collect()
}
pub fn select_groups(api:&Api,requested:&[String])->Result<Vec<String>> {
    let groups=api.groups()?;
    if requested.is_empty(){return Ok(groups);}
    for g in requested{ensure!(groups.contains(g),"unknown service group: {g}");}
    Ok(requested.iter().cloned().collect::<BTreeSet<_>>().into_iter().collect())
}
pub fn snapshot(api:&Api,groups:&[String])->Result<Vec<Instance>> {
    let mut rows=Vec::new();
    for g in groups{let batch=api.instances(g)?;eprintln!("[INFO] Service: {g} ({} instances)",batch.len());rows.extend(batch);}
    Ok(rows)
}
fn record(file:&mut File,phase:&str,status:&str,r:&Instance,groups:Option<&[String]>)->Result<()> {
    let mut event=json!({"phase":phase,"status":status,"ID":r.id,"service":r.name});
    if let Some(g)=groups{event["existing_groups"]=json!(g);}
    serde_json::to_writer(&mut *file,&event)?;writeln!(file)?;file.sync_data()?;Ok(())
}
fn create_report(path:&Path)->Result<File> {
    let mut o=OpenOptions::new();o.write(true).create_new(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;o.mode(0o600);}
    o.open(path).with_context(||format!("create report {} (will not overwrite)",path.display()))
}
pub fn import(api:&Api,wanted:&[Instance],apply:bool,skip_conflicts:bool,report:Option<PathBuf>)->Result<i32>{
    let groups=api.groups()?;
    let old=snapshot(api,&groups).context("catalog query failed; no writes")?;
    execute(api,wanted,&old,apply,skip_conflicts,report)
}
pub(crate) fn execute(api:&Api,wanted:&[Instance],old:&[Instance],apply:bool,skip_conflicts:bool,report:Option<PathBuf>)->Result<i32>{
    let items=plan(wanted,old);
    let report=report.unwrap_or_else(||PathBuf::from(format!("import-report-{}-{}.jsonl",SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),std::process::id())));
    let mut log=create_report(&report)?;
    let (mut planned,mut unchanged,mut conflicts)=(0,0,0);
    for (r,s,g) in &items {
        record(&mut log,"plan",s.as_str(),r,Some(g))?;
        match s {Status::Planned=>planned+=1,Status::Unchanged=>unchanged+=1,Status::Conflict=>conflicts+=1};
        eprintln!("[{}] {} / {}",s.as_str(),r.name,r.id);
    }
    eprintln!("[INFO] Planned={planned} Unchanged={unchanged} Conflicts={conflicts}\n[INFO] Report: {}",report.display());
    if !apply{return Ok(if conflicts>0&&!skip_conflicts{2}else{0});}
    if conflicts>0&&!skip_conflicts{eprintln!("[ERROR] conflicts; no writes");return Ok(1);}
    let mut accepted:BTreeMap<&str,Vec<&Instance>>=BTreeMap::new();
    let mut failed=false; let mut unattempted=0;let mut sent=0;
    for (r,s,_) in &items {
        if *s!=Status::Planned{continue;}
        if failed{record(&mut log,"create","not_attempted",r,None)?;unattempted+=1;continue;}
        // Persist intent before the non-idempotent HTTP request. Never retry a write.
        record(&mut log,"create","submitting",r,None)?;
        match api.create(r){
            Ok(())=>{record(&mut log,"create","accepted",r,None)?;accepted.entry(&r.name).or_default().push(r);sent+=1;eprintln!("[ACCEPTED] {} / {}",r.name,r.id);},
            Err(e)=>{eprintln!("[ERROR] {} / {}: {e:#}",r.name,r.id);record(&mut log,"create","failed_or_uncertain",r,None)?;failed=true;}
        }
    }
    let mut verified=0;let mut unverified=0;
    for (group,rows) in accepted {
        let mut good=BTreeSet::<String>::new();
        for attempt in 0..5 {
            match api.instances(group) {
                Ok(current)=>{
                    let mut index:HashMap<&str,Vec<&Instance>>=HashMap::new();
                    for r in &current{index.entry(&r.id).or_default().push(r);}
                    good.clear();
                    for r in &rows{if let Some(m)=index.get(r.id.as_str()){if m.len()==1&&m[0]==*r{good.insert(r.id.clone());}}}
                    if good.len()==rows.len(){break;}
                },
                Err(e)=>{good.clear();eprintln!("[WARN] readback {group}: {e:#}");}
            }
            if attempt<4{thread::sleep(Duration::from_secs(1));}
        }
        for r in rows{if good.contains(&r.id){record(&mut log,"verify","verified",r,None)?;verified+=1;}else{record(&mut log,"verify","unverified",r,None)?;unverified+=1;}}
    }
    eprintln!("[INFO] Accepted={sent} Verified={verified} Unverified={unverified} NotAttempted={unattempted}");
    Ok(if failed||unverified>0{1}else{0})
}
#[cfg(test)]
mod tests{
    use super::*;
    fn row(id:String)->Instance{Instance{id,name:"snmp".into(),address:"10.0.0.1".into(),port:161,tags:vec![],meta:Default::default()}}
    #[test]fn large_catalog(){let old=(0..6001).map(|i|row(format!("id-{i}"))).collect::<Vec<_>>();let mut changed=old[1].clone();changed.name="other".into();let wanted=vec![old[0].clone(),changed,row("new".into())];let p=plan(&wanted,&old);assert_eq!(p.iter().map(|x|x.1).collect::<Vec<_>>(),vec![Status::Unchanged,Status::Conflict,Status::Planned]);}
    #[test]fn duplicate_existing_id(){let r=row("id".into());let wanted=[r.clone()];let p=plan(&wanted,&[r.clone(),r]);assert_eq!(p[0].1,Status::Conflict);}
}
