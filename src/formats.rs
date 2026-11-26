use crate::model::Instance;
use anyhow::{bail, ensure, Context, Result};
use clap::ValueEnum;
use std::{collections::{BTreeMap, HashSet}, fs::File, io::{BufRead, BufReader, Write}, path::Path};
use tempfile::NamedTempFile;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Format { Txt, Csv, Json, Jsonl }
impl Format {
    pub fn infer(path: &Path, explicit: Option<Self>) -> Result<Self> {
        if let Some(f) = explicit { return Ok(f); }
        match path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
            "txt" => Ok(Self::Txt), "csv" => Ok(Self::Csv), "json" => Ok(Self::Json), "jsonl" => Ok(Self::Jsonl), _ => bail!("use --format txt|csv|json|jsonl"),
        }
    }
}
const HEADER: [&str; 6] = ["ServiceName","InstanceID","IP","Port","Tags","Meta"];
fn from_fields(f: &[&str], csv: bool) -> Result<Instance> {
    ensure!(f.len() == 6, "expected six fields");
    let (tags, meta) = if csv {
        (serde_json::from_str(if f[4].is_empty() { "[]" } else { f[4] })?, serde_json::from_str(if f[5].is_empty() { "{}" } else { f[5] })?)
    } else {
        let tags = f[4].split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect();
        let mut meta = BTreeMap::new();
        for s in f[5].split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (k,v) = s.split_once('=').context("Meta must be key=value")?;
            ensure!(!k.trim().is_empty() && meta.insert(k.trim().to_owned(),v.trim().to_owned()).is_none(), "empty or duplicate Meta key");
        }
        (tags,meta)
    };
    Ok(Instance { name:f[0].into(), id:f[1].into(), address:f[2].into(), port:if f[3].is_empty(){0}else{f[3].parse()?},tags,meta })
}
pub fn read(path: &Path, format: Format) -> Result<Vec<Instance>> {
    let file = File::open(path).with_context(|| format!("open {}",path.display()))?;
    read_from(BufReader::new(file),format)
}
pub(crate) fn read_from<R: BufRead>(file:R, format:Format)->Result<Vec<Instance>> {
    let mut rows = Vec::new();
    match format {
        Format::Json => { rows = serde_json::from_reader(file)?; }
        Format::Jsonl | Format::Txt => for (i,line) in file.lines().enumerate() {
            let line = line?;
            let line = if i==0 {line.trim_start_matches('\u{feff}')} else {&line};
            if line.trim().is_empty() || (matches!(format,Format::Txt) && line.trim_start().starts_with('#')) {continue;}
            let row = match format { Format::Jsonl => serde_json::from_str(line).map_err(anyhow::Error::from), _ => from_fields(&line.split('|').map(str::trim).collect::<Vec<_>>(),false) }.with_context(||format!("line {}",i+1))?;
            rows.push(row);
        },
        Format::Csv => {
            let mut rdr = csv::Reader::from_reader(file);
            ensure!(rdr.headers()?.iter().eq(HEADER.iter().copied()), "invalid CSV header");
            for record in rdr.records() { let r=record?; rows.push(from_fields(&r.iter().collect::<Vec<_>>(),true)?); }
        }
    }
    ensure!(!rows.is_empty(),"no instances found");
    let mut ids=HashSet::new();
    for r in &mut rows {r.validate()?; r.canonicalize(); ensure!(ids.insert(r.id.clone()),"duplicate input ID: {}",r.id);}
    Ok(rows)
}
fn txt_line(r: &Instance) -> Result<String> {
    let plain = |s: &str| s.trim()==s && !s.contains(['|','\r','\n']);
    let tag = |s: &str| plain(s) && !s.is_empty() && !s.contains(',');
    ensure!(plain(&r.id) && plain(&r.name) && !r.name.starts_with('#') && plain(&r.address) && r.tags.iter().all(|s|tag(s)) && r.meta.iter().all(|(k,v)| tag(k) && !k.contains('=') && plain(v) && !v.contains(',')), "TXT cannot represent {}; use JSON or CSV",r.id);
    Ok(format!("{}|{}|{}|{}|{}|{}",r.name,r.id,r.address,r.port,r.tags.join(","),r.meta.iter().map(|(k,v)|format!("{k}={v}")).collect::<Vec<_>>().join(",")))
}
pub fn write(path: &Path, format: Format, rows: &[Instance], force: bool) -> Result<()> {
    if let Ok(m)=std::fs::symlink_metadata(path) { ensure!(!m.file_type().is_symlink() && m.is_file() && force,"refusing existing output (use --force for regular files): {}",path.display()); }
    let parent=path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut tmp=NamedTempFile::new_in(parent)?;
    write_to(&mut tmp,format,rows)?;
    tmp.as_file().sync_all()?;
    if force {tmp.persist(path)?;} else {tmp.persist_noclobber(path)?;}
    Ok(())
}
pub(crate) fn write_to<W: Write>(mut tmp: W, format:Format, rows:&[Instance])->Result<()> {
    match format {
        Format::Json => {serde_json::to_writer_pretty(&mut tmp,rows)?; writeln!(tmp)?;},
        Format::Jsonl => for r in rows {serde_json::to_writer(&mut tmp,r)?;writeln!(tmp)?;},
        Format::Txt => {writeln!(tmp,"# ServiceName|InstanceID|IP|Port|Tags|Meta")?;for r in rows {writeln!(tmp,"{}",txt_line(r)?)?;}},
        Format::Csv => {let mut w=csv::Writer::from_writer(&mut tmp); w.write_record(HEADER)?; for r in rows {w.write_record([r.name.clone(),r.id.clone(),r.address.clone(),r.port.to_string(),serde_json::to_string(&r.tags)?,serde_json::to_string(&r.meta)?])?;}w.flush()?;},
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn roundtrip() {
        let t=tempfile::tempdir().unwrap();
        let row:Instance=serde_json::from_str(r#"{"ID":"a/b@c","name":"snmp","address":"10.0.0.1","port":161,"tags":[],"meta":{"v":"hello"}}"#).unwrap();
        for f in [Format::Txt,Format::Csv,Format::Json,Format::Jsonl] {let p=t.path().join(format!("{f:?}"));write(&p,f,&[row.clone()],false).unwrap();assert_eq!(read(&p,f).unwrap(),vec![row.clone()]);assert!(write(&p,f,&[],false).is_err());}
    }
}
