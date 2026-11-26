//! Local-only Web console. Blocking TenSunS client stays outside async runtime.
use base64::{Engine as _,engine::general_purpose::STANDARD as B64};
use crate::workspace::{self,Profiles,Task};
use crate::snapshots::{self,Ref as SnapshotRef};
use crate::{api::Api, formats::{self,Format}, model::Instance, operations::{self,Status}};
use anyhow::{ensure,Context,Result};
use axum::{Router,Json,body::Body,extract::{State,DefaultBodyLimit},http::{HeaderMap,HeaderValue,Request,StatusCode,header},middleware::{self,Next},response::{Html,IntoResponse,Response},routing::{get,post}};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{collections::BTreeMap,io::Cursor,net::{IpAddr,SocketAddr},path::{Path,PathBuf},sync::{Arc,Mutex},time::{Duration,Instant}};
use tokio::sync::Semaphore;
use uuid::Uuid;

const MAX_ROWS:usize=10_000;
const MAX_BODY:usize=16*1024*1024;
type WebResult=std::result::Result<Response,WebError>;
struct WebError(StatusCode,String);
impl IntoResponse for WebError {
    fn into_response(self)->Response{(self.0,Json(json!({"error":self.1}))).into_response()}
}
fn bad(message:impl Into<String>)->WebError{WebError(StatusCode::BAD_REQUEST,message.into())}
fn unauthorized()->WebError{WebError(StatusCode::UNAUTHORIZED,"请重新登录".into())}
fn upstream(e:anyhow::Error)->WebError{WebError(StatusCode::BAD_GATEWAY,format!("{e:#}"))}
fn internal(e:impl std::fmt::Display)->WebError{eprintln!("[WEB] local error: {e}");WebError(StatusCode::INTERNAL_SERVER_ERROR,"本地处理失败，请检查服务端及报告目录".into())}
fn conflict(message:impl Into<String>)->WebError{WebError(StatusCode::CONFLICT,message.into())}
fn ok(v:Value)->Response{Json(v).into_response()}
struct Preview{ticket:String,rows:Vec<Instance>,signature:Vec<(Status,Vec<String>)>,created:Instant,source:Option<SnapshotRef>}
struct Session{
    key:String,api:Api,started:Instant,preview:Option<Preview>,
    profile_id:String,
}
struct Shared{
    authority:String,origin:String,base:String,user:String,timeout:u64,
    report_dir:PathBuf,session:Mutex<Option<Session>>,gate:Arc<Semaphore>,profiles:Mutex<Profiles>,progress:Mutex<Option<(String,PathBuf,String)>>,
}
#[derive(Deserialize)]
#[serde(tag="action",rename_all="snake_case",deny_unknown_fields)]
enum Action{
    Login{username:String,password:String,#[serde(default)]profile_id:Option<String>,#[serde(default)]url:Option<String>},
    AddProfile{name:String,url:String,username:String},
    Logout,
    Groups,
    Instances{#[serde(default)]service:Option<String>,#[serde(default)]query:String,page:usize,page_size:usize},
    Preview{format:String,content:String},
    Apply{ticket:String,confirm:bool,#[serde(default)]skip_conflicts:bool},
    Export{format:String,#[serde(default)]services:Vec<String>},
    Template,
    SnapshotCreate{label:String,#[serde(default)]services:Vec<String>},
    Snapshots,
    SnapshotDownload{source:SnapshotRef},
    Compare{source:SnapshotRef,#[serde(default)]target:Option<SnapshotRef>,#[serde(default)]services:Vec<String>},
    SyncPreview{source:SnapshotRef,#[serde(default)]services:Vec<String>},
    Reports,
    Report{id:String},
}
fn fmt(s:&str)->std::result::Result<Format,WebError>{match s{"txt"=>Ok(Format::Txt),"csv"=>Ok(Format::Csv),"json"=>Ok(Format::Json),"jsonl"=>Ok(Format::Jsonl),"xlsx"=>Ok(Format::Xlsx),_=>Err(bad("不支持的文件格式"))}}
fn signature(rows:&[Instance],old:&[Instance])->Vec<(Status,Vec<String>)>{
    operations::plan(rows,old).into_iter().map(|(_,s,g)|(s,g)).collect()
}
fn preview_rows(s:&mut Session,rows:Vec<Instance>,old:&[Instance],source:Option<SnapshotRef>)->WebResult{
    if rows.is_empty()||rows.len()>MAX_ROWS{return Err(bad("单批必须为 1 至 10000 条实例"));}
    let sig=signature(&rows,old);let mut counts=BTreeMap::<&str,usize>::new();
    let entries=rows.iter().zip(&sig).map(|(row,(status,groups))|{*counts.entry(status.as_str()).or_default()+=1;json!({"instance":row,"status":status.as_str(),"existing_groups":groups})}).collect::<Vec<_>>();
    let ticket=Uuid::new_v4().to_string();
    s.preview=Some(Preview{ticket:ticket.clone(),rows,signature:sig,created:Instant::now(),source:source.clone()});
    Ok(ok(json!({"ticket":ticket,"counts":counts,"entries":entries,"expires_in":600,"source":source})))
}
fn catalog(api:&Api)->Result<Vec<Instance>>{operations::snapshot(api,&api.groups()?)}
fn report_path(dir:&Path,id:&str)->PathBuf{dir.join(format!("import-{id}.jsonl"))}

async fn guard(State(state):State<Arc<Shared>>,request:Request<Body>,next:Next)->Response{
    let host=request.headers().get(header::HOST).and_then(|v|v.to_str().ok());
    if host!=Some(state.authority.as_str()){
        return (StatusCode::FORBIDDEN,"Invalid Host; open the exact URL printed by tensuns").into_response();
    }
    if request.method()!=axum::http::Method::GET {
        let origin=request.headers().get(header::ORIGIN).and_then(|v|v.to_str().ok());
        if origin!=Some(state.origin.as_str()){
            return (StatusCode::FORBIDDEN,"Invalid Origin").into_response();
        }
    }
    let mut r=next.run(request).await;
    let h=r.headers_mut();
    h.insert(header::CACHE_CONTROL,HeaderValue::from_static("no-store"));
    h.insert("x-content-type-options",HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options",HeaderValue::from_static("DENY"));
    h.insert("referrer-policy",HeaderValue::from_static("no-referrer"));
    h.insert("content-security-policy",HeaderValue::from_static("default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"));
    r
}
async fn page()->Html<&'static str>{Html(include_str!("../web/index.html"))}
async fn js()->impl IntoResponse{([(header::CONTENT_TYPE,"application/javascript; charset=utf-8")],include_str!("../web/app.js"))}
async fn css()->impl IntoResponse{([(header::CONTENT_TYPE,"text/css; charset=utf-8")],include_str!("../web/style.css"))}
async fn info(State(s):State<Arc<Shared>>)->WebResult{
    let p=s.profiles.lock().map_err(internal)?;
    Ok(ok(json!({"url":s.base,"username":s.user,"version":env!("CARGO_PKG_VERSION"),"profiles":p.items,"default_id":p.default_id})))
}
async fn progress(State(s):State<Arc<Shared>>,headers:HeaderMap)->WebResult{
    let key=headers.get("x-tensuns-session").and_then(|v|v.to_str().ok()).unwrap_or("");
    let current=s.progress.lock().map_err(internal)?.clone();
    if let Some((owner,path,id))=current{if key==owner && !key.is_empty(){let text=std::fs::read_to_string(path).unwrap_or_default();return Ok(ok(json!({"running":true,"id":id,"counts":workspace::report_counts(&text)})));}}
    Ok(ok(json!({"running":false})))
}
async fn action(State(state):State<Arc<Shared>>,headers:HeaderMap,Json(action):Json<Action>)->WebResult{
    let key=headers.get("x-tensuns-session").and_then(|x|x.to_str().ok()).unwrap_or("").to_owned();
    let permit=state.gate.clone().try_acquire_owned().map_err(|_|conflict("已有操作正在执行，请勿重复提交"))?;
    tokio::task::spawn_blocking(move||{let _permit=permit;dispatch(&state,&key,action)}).await.map_err(internal)?
}
fn dispatch(state:&Shared,key:&str,action:Action)->WebResult{
    let mut slot=state.session.lock().map_err(internal)?;
    if slot.as_ref().is_some_and(|s|s.started.elapsed()>=Duration::from_secs(3600)){*slot=None;}
    let action=match action{
        Action::Login{username,password,profile_id,url}=>{
            let mut password=zeroize::Zeroizing::new(password);
            if username.trim().is_empty()||password.is_empty()||username.len()>256||password.len()>4096{return Err(bad("用户名或密码无效"));}
            // An explicit URL selects a connection by URL + username, never rebinds a
            // saved profile ID (its existing snapshots/reports must keep their origin).
            let (url,existing)={
                let profiles=state.profiles.lock().map_err(internal)?;
                if let Some(url)=url{
                    let url=workspace::validate_url(url.trim()).map_err(|e|bad(e.to_string()))?;
                    let existing=profiles.items.iter().find(|p|p.url==url&&p.username==username.trim()).cloned();
                    if existing.is_none()&&profiles.items.len()>=100{return Err(bad("最多保存 100 个连接"));}
                    (url,existing)
                }else{
                    let p=profiles.get(profile_id.as_deref()).map_err(|e|bad(e.to_string()))?;
                    if username.trim()!=p.username{return Err(bad("用户名需与连接配置一致，或显式提供 URL"));}
                    (p.url.clone(),Some(p))
                }
            };
            let api=Api::login_web(&url,username.trim(),state.timeout,std::mem::take(&mut *password)).map_err(|_|unauthorized())?;
            api.groups().map_err(upstream)?;
            // Persist a new profile only after successful authentication and catalog access.
            let profile=match existing{
                Some(p)=>p,
                None=>{
                    let mut profiles=state.profiles.lock().map_err(internal)?;
                    let name=format!("TenSunS {}",profiles.items.len()+1);
                    profiles.add(name,url,username.trim().to_owned()).map_err(internal)?
                }
            };
            let token=Uuid::new_v4().to_string();
            *slot=Some(Session{key:token.clone(),api,started:Instant::now(),preview:None,profile_id:profile.id.clone()});
            return Ok(ok(json!({"session":token,"expires_in":3600,"profile":profile})));
        },
        other=>other,
    };
    if slot.as_ref().is_none_or(|s|key.is_empty()||key!=s.key){return Err(unauthorized());}
    if matches!(&action,Action::Logout){*slot=None;return Ok(ok(json!({"ok":true}))); }
    let s=slot.as_mut().ok_or_else(unauthorized)?;
    let dir=workspace::profile_dir(&state.report_dir,&s.profile_id).map_err(internal)?;
    match action {
        Action::AddProfile{name,url,username}=>{let p=state.profiles.lock().map_err(internal)?.add(name,url,username).map_err(|e|bad(e.to_string()))?;Ok(ok(json!({"profile":p})))},
        Action::Groups=>{
            let mut result=Vec::new();
            for name in s.api.groups().map_err(upstream)?{
                let count=s.api.instances(&name).map_err(upstream)?.len();
                result.push(json!({"name":name,"count":count}));
            }
            Ok(ok(json!({"groups":result})))
        },
        Action::Instances{service,query,page,page_size}=>{
            if page==0||![10,20,50,100].contains(&page_size)||query.len()>4096{return Err(bad("无效分页或搜索条件"));}
            let requested=service.into_iter().collect::<Vec<_>>();
            let groups=operations::select_groups(&s.api,&requested).map_err(upstream)?;
            let mut rows=operations::snapshot(&s.api,&groups).map_err(upstream)?;
            let q=query.trim().to_lowercase();
            if !q.is_empty(){rows.retain(|r|r.id.to_lowercase().contains(&q)||r.name.to_lowercase().contains(&q)||r.address.to_lowercase().contains(&q)||r.tags.iter().any(|x|x.to_lowercase().contains(&q))||r.meta.iter().any(|(k,v)|k.to_lowercase().contains(&q)||v.to_lowercase().contains(&q)));}
            rows.sort_by(|a,b|(&a.name,&a.id).cmp(&(&b.name,&b.id)));
            let total=rows.len();
            let start=page.saturating_sub(1).saturating_mul(page_size).min(total);
            let end=start.saturating_add(page_size).min(total);
            Ok(ok(json!({"total":total,"rows":&rows[start..end],"page":page,"page_size":page_size})))
        },
        Action::Preview{format,content}=>{
            s.preview=None;
            let rows=if format=="xlsx"{let bytes=B64.decode(&content).map_err(|_|bad("无效 XLSX 数据"))?;crate::excel::read(&bytes)}else{formats::read_from(Cursor::new(content.as_bytes()),fmt(&format)?)}.map_err(|e|bad(format!("{e:#}")))?;
            if rows.len()>MAX_ROWS{return Err(bad("单批最多 10000 条实例"));}
            let old=catalog(&s.api).map_err(upstream)?;
            preview_rows(s,rows,&old,None)
        },
        Action::Apply{ticket,confirm,skip_conflicts}=>{
            if !confirm{return Err(bad("必须明确确认执行"));}
            let p=s.preview.take().ok_or_else(||conflict("请先生成导入预览"))?;
            if p.ticket!=ticket||p.created.elapsed()>=Duration::from_secs(600){return Err(conflict("预览已失效，请重新预览"));}
            let old=catalog(&s.api).map_err(upstream)?;
            if signature(&p.rows,&old)!=p.signature{return Err(conflict("远端配置已变化，请重新预览"));}
            if !skip_conflicts&&p.signature.iter().any(|(status,_)|*status==Status::Conflict){return Err(conflict("存在冲突，未写入；请重新预览或显式跳过冲突"));}
            let id=Uuid::new_v4().to_string();
            let path=report_path(&dir,&id);
            let mut task=Task::new(id.clone());task.source=p.source.clone();task.save(&dir).map_err(internal)?;
            *state.progress.lock().map_err(internal)?=Some((key.into(),path.clone(),id.clone()));
            // Shared CLI implementation persists intent and stops on uncertain writes.
            let result=operations::execute(&s.api,&p.rows,&old,true,skip_conflicts,Some(path.clone()));
            let (code,message)=match result{Ok(c)=>(c,None),Err(e)=>(1,Some(format!("{e:#}")))};
            *state.progress.lock().map_err(internal)?=None;
            task.state=if code==0{"completed"}else{"failed"}.into();task.exit_code=Some(code);task.save(&dir).map_err(internal)?;
            let report=std::fs::read_to_string(&path).unwrap_or_default();
            let mut latest=BTreeMap::<String,String>::new();
            for line in report.lines(){
                if let Ok(v)=serde_json::from_str::<Value>(line){
                    if let (Some(id),Some(status))=(v["ID"].as_str(),v["status"].as_str()){latest.insert(id.into(),status.into());}
                }
            }
            let mut counts=BTreeMap::<String,usize>::new();
            for status in latest.values(){*counts.entry(status.clone()).or_default()+=1;}
            Ok(ok(json!({"exit_code":code,"report_id":if report.is_empty(){None}else{Some(id)},"counts":counts,"message":message,"report":report})))
        },
        Action::Export{format,services}=>{
            let f=fmt(&format)?;
            let groups=operations::select_groups(&s.api,&services).map_err(upstream)?;
            let rows=operations::snapshot(&s.api,&groups).map_err(upstream)?;
            let mut bytes=Vec::new();
            formats::write_to(&mut bytes,f,&rows).map_err(|e|bad(format!("{e:#}")))?;
            let (text,encoding)=if format=="xlsx"{(B64.encode(bytes),"base64")}else{(String::from_utf8(bytes).map_err(internal)?,"utf8")};
            Ok(ok(json!({"filename":format!("tensuns-instances.{format}"),"content":text,"encoding":encoding,"count":rows.len()})))
        },
        Action::SnapshotCreate{label,services}=>{
            let profile=state.profiles.lock().map_err(internal)?.get(Some(&s.profile_id)).map_err(internal)?;
            let all_groups=services.is_empty();let selected=operations::select_groups(&s.api,&services).map_err(upstream)?;
            let rows=operations::snapshot(&s.api,&selected).map_err(upstream)?;
            let meta=snapshots::save(&dir,&profile,&label,selected,all_groups,rows).map_err(|e|bad(format!("{e:#}")))?;
            Ok(ok(json!({"snapshot":meta})))
        },
        Action::Snapshots=>{
            let profiles=state.profiles.lock().map_err(internal)?.items.clone();
            Ok(ok(json!({"snapshots":snapshots::list(&state.report_dir,&profiles).map_err(internal)?})))
        },
        Action::SnapshotDownload{source}=>{
            let profiles=state.profiles.lock().map_err(internal)?.items.clone();
            let snap=snapshots::load(&state.report_dir,&profiles,&source).map_err(|e|bad(format!("{e:#}")))?;
            let content=serde_json::to_string_pretty(&snap.rows).map_err(internal)?;
            Ok(ok(json!({"filename":format!("snapshot-{}.json",snap.meta.id),"content":content,"count":snap.rows.len()})))
        },
        Action::Compare{source,target,services}=>{
            let profiles=state.profiles.lock().map_err(internal)?.items.clone();
            let source=snapshots::load(&state.report_dir,&profiles,&source).map_err(|e|bad(format!("{e:#}")))?;
            let (wanted,scope)=snapshots::select(&source,&services).map_err(|e|bad(format!("{e:#}")))?;
            let old=if let Some(reference)=target{
                let target=snapshots::load(&state.report_dir,&profiles,&reference).map_err(|e|bad(format!("{e:#}")))?;
                if !target.meta.all_groups{return Err(bad("历史比较的目标必须是全量快照，以检测跨服务组 ID 冲突"));}
                target.rows
            }else{catalog(&s.api).map_err(upstream)?};
            let result=snapshots::compare(&wanted,&old,scope.as_ref());
            Ok(ok(serde_json::to_value(result).map_err(internal)?))
        },
        Action::SyncPreview{source,services}=>{
            s.preview=None;
            let profiles=state.profiles.lock().map_err(internal)?.items.clone();
            let snap=snapshots::load(&state.report_dir,&profiles,&source).map_err(|e|bad(format!("{e:#}")))?;
            let (wanted,_)=snapshots::select(&snap,&services).map_err(|e|bad(format!("{e:#}")))?;
            let rows=formats::validate_rows(wanted).map_err(|e|bad(format!("{e:#}")))?;
            if rows.len()>MAX_ROWS{return Err(bad("同步单批最多 10000 条，请按服务组拆分"));}
            let old=catalog(&s.api).map_err(upstream)?;
            preview_rows(s,rows,&old,Some(source))
        },
        Action::Template=>{let row=Instance{id:"switch-01".into(),name:"snmp_exporter".into(),address:"192.0.2.11".into(),port:161,tags:vec![],meta:BTreeMap::new()};let content=B64.encode(crate::excel::write(&[row]).map_err(internal)?);Ok(ok(json!({"filename":"instances.xlsx","content":content,"encoding":"base64"})))},
        Action::Reports=>{
            let reports=workspace::tasks(&dir).map_err(internal)?;
            Ok(ok(json!({"reports":reports})))
        },
        Action::Report{id}=>{
            if !workspace::valid_id(&id)||!workspace::tasks(&dir).map_err(internal)?.iter().any(|t|t.id==id){return Err(WebError(StatusCode::NOT_FOUND,"报告不属于当前连接".into()));}
            let content=std::fs::read_to_string(report_path(&dir,&id)).map_err(internal)?;
            Ok(ok(json!({"filename":format!("import-{id}.jsonl"),"content":content})))
        },
        Action::Login{..}|Action::Logout=>unreachable!(),
    }
}
pub fn serve(host:IpAddr,port:u16,base:String,user:String,timeout:u64,report_dir:PathBuf)->Result<()>{
    ensure!(host.is_loopback(),"Web 模式只允许 loopback 地址；禁止对外监听");
    ensure!(port>0 && timeout>0,"port and timeout must be positive");
    let url=reqwest::Url::parse(&base)?;
    ensure!(matches!(url.scheme(),"http"|"https")&&url.username().is_empty()&&url.password().is_none()&&url.query().is_none()&&url.fragment().is_none()&&url.host_str().is_some(),"invalid TenSunS Web URL");
    std::fs::create_dir_all(&report_dir).context("create report directory")?;
    let report_dir=std::fs::canonicalize(report_dir)?;
    let addr=SocketAddr::new(host,port);
    let authority=addr.to_string();
    let origin=format!("http://{authority}");
    let profiles=Profiles::open(&report_dir,&base,&user)?;
    let state=Arc::new(Shared{authority,origin:origin.clone(),base,user,timeout,report_dir,session:Mutex::new(None),gate:Arc::new(Semaphore::new(1)),profiles:Mutex::new(profiles),progress:Mutex::new(None)});
    // Keep a reference outside Tokio so the blocking reqwest runtime is dropped synchronously.
    let runtime=tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let app=Router::new().route("/",get(page)).route("/app.js",get(js)).route("/style.css",get(css)).route("/api/info",get(info)).route("/api/progress",get(progress)).route("/api/action",post(action))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn_with_state(state.clone(),guard)).with_state(state.clone());
    runtime.block_on(async move{
        let listener=tokio::net::TcpListener::bind(addr).await?;
        eprintln!("[WEB] Open {origin}");
        axum::serve(listener,app).with_graceful_shutdown(async{let _=tokio::signal::ctrl_c().await;}).await?;
        Ok::<(),std::io::Error>(())
    })?;
    drop(runtime);
    drop(state);
    Ok(())
}
