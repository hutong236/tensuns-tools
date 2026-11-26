mod api;
mod formats;
mod model;
mod operations;
mod web;
mod excel;
mod workspace;
mod snapshots;
use anyhow::Result;
use clap::{Parser,Subcommand};
use formats::Format;
use std::{ffi::OsString,path::PathBuf};

#[derive(Parser)]
#[command(version,about="TenSunS custom instance import/export via Web API")]
struct Cli{
    #[arg(long,env="TENSUNS_URL",default_value="http://127.0.0.1:1026",global=true)] url:String,
    #[arg(long,env="TENSUNS_USER",default_value="admin",global=true)] user:String,
    #[arg(long,env="TENSUNS_TIMEOUT",default_value_t=120,global=true)] timeout:u64,
    #[command(subcommand)] command:Command,
}
#[derive(Subcommand)]
enum Command{
    Serve{#[arg(long,default_value="127.0.0.1")]host:std::net::IpAddr,#[arg(long,default_value_t=8080)]port:u16,#[arg(long,default_value="tensuns-web-reports")]report_dir:PathBuf},
    Diff{file:PathBuf,#[arg(long,value_enum)]format:Option<Format>,#[arg(long)]changes_only:bool},
    Validate{file:PathBuf,#[arg(long,value_enum)]format:Option<Format>},
    Import{file:PathBuf,#[arg(long,value_enum)]format:Option<Format>,#[arg(long,conflicts_with="dry_run")]apply:bool,#[arg(long)]dry_run:bool,#[arg(long)]skip_conflicts:bool,#[arg(long)]report:Option<PathBuf>},
    Export{file:PathBuf,#[arg(long,value_enum)]format:Option<Format>,#[arg(long)]service:Vec<String>,#[arg(long)]force:bool},
    List{#[arg(long)]service:Vec<String>},
}
fn run()->Result<i32>{
    let mut args=std::env::args_os().collect::<Vec<OsString>>();
    // Legacy CLI: tensuns FILE --apply
    if let Some(s)=args.get(1).and_then(|s|s.to_str()) {if !s.starts_with('-')&&!matches!(s,"import"|"export"|"list"|"validate"|"diff"|"serve"|"help"){args.insert(1,"import".into());}}
    let cli=Cli::parse_from(args);
    match cli.command{
        Command::Serve{host,port,report_dir}=>{web::serve(host,port,cli.url,cli.user,cli.timeout,report_dir)?;Ok(0)},
        Command::Diff{file,format,changes_only}=>{let rows=formats::read(&file,Format::infer(&file,format)?)?;let api=api::Api::login(&cli.url,&cli.user,cli.timeout)?;let old=operations::snapshot(&api,&api.groups()?)?;let scope=rows.iter().map(|r|r.name.clone()).collect();let mut result=snapshots::compare(&rows,&old,Some(&scope));if changes_only{result.entries.retain(|e|e.status!="unchanged");}serde_json::to_writer_pretty(std::io::stdout().lock(),&result)?;println!();Ok(0)},
        Command::Validate{file,format}=>{let rows=formats::read(&file,Format::infer(&file,format)?)?;eprintln!("[SUCCESS] Validated {} instances (offline)",rows.len());Ok(0)},
        Command::Import{file,format,apply,dry_run:_,skip_conflicts,report}=>{let rows=formats::read(&file,Format::infer(&file,format)?)?;eprintln!("[INFO] Validated {} instances",rows.len());let api=api::Api::login(&cli.url,&cli.user,cli.timeout)?;operations::import(&api,&rows,apply,skip_conflicts,report)},
        Command::Export{file,format,service,force}=>{let f=Format::infer(&file,format)?;let api=api::Api::login(&cli.url,&cli.user,cli.timeout)?;let groups=operations::select_groups(&api,&service)?;let rows=operations::snapshot(&api,&groups)?;formats::write(&file,f,&rows,force)?;eprintln!("[SUCCESS] Exported {} instances to {}",rows.len(),file.display());Ok(0)},
        Command::List{service}=>{let api=api::Api::login(&cli.url,&cli.user,cli.timeout)?;let groups=operations::select_groups(&api,&service)?;let mut counts=Vec::new();for g in groups{let n=api.instances(&g)?.len();counts.push((g,n));}println!("ServiceName\tCount");for(g,n)in counts{println!("{g}\t{n}");}Ok(0)},
    }
}
fn main(){let code=match run(){Ok(c)=>c,Err(e)=>{eprintln!("[ERROR] {e:#}");1}};std::process::exit(code);}
