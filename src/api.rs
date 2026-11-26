use crate::model::Instance;
use anyhow::{bail, ensure, Context, Result};
use reqwest::{blocking::{Client,RequestBuilder}, header::{HeaderValue, ACCEPT, AUTHORIZATION}, redirect::Policy, Url};
use serde_json::{json, Value};
use std::{collections::BTreeSet, io::{self, IsTerminal}, time::Duration};
use zeroize::Zeroize;

pub struct Api { client: Client, base: String, auth: HeaderValue }
impl Api {
    pub fn login(base: &str, user: &str, timeout: u64) -> Result<Self> {
        let url = Url::parse(base)?;
        ensure!(matches!(url.scheme(),"http"|"https") && url.host_str().is_some() && url.username().is_empty() && url.password().is_none() && url.query().is_none(),"invalid Web base URL");
        let mut url=url; url.set_fragment(None);
        let base=url.as_str().trim_end_matches('/').to_string();
        ensure!(!base.ends_with("/api") && !base.ends_with("/v1"),"use Web base URL without /api or /v1");
        if url.scheme()=="http" {eprintln!("[WARN] HTTP transmits credentials in cleartext; HTTPS recommended");}
        ensure!(timeout>0,"timeout must be positive");
        let client=Client::builder().timeout(Duration::from_secs(timeout)).connect_timeout(Duration::from_secs(5)).redirect(Policy::none()).build()?;
        let mut token=match std::env::var("TENSUNS_TOKEN") {Ok(s) if !s.is_empty()=>s,_=>{
            eprintln!("[INFO] Login: {base} / {user}");
            let mut password=if io::stdin().is_terminal(){rpassword::prompt_password("TenSunS Password: ")?}else{let mut s=String::new();io::stdin().read_line(&mut s)?;s.trim_end_matches(['\r','\n']).to_owned()};
            let request=client.post(format!("{base}/api/user/login")).header(ACCEPT,"application/json").json(&json!({"username":user,"password":password}));
            password.zeroize();
            let mut response=send(request,"login",false)?;
            let tok=response.pointer_mut("/data/token").and_then(|v|v.as_str().map(str::to_owned)).context("missing login token")?;
            if let Some(Value::String(s))=response.pointer_mut("/data/token"){s.zeroize();}
            tok
        }};
        ensure!(!token.trim().is_empty(),"empty token");
        if !token.starts_with("Bearer "){token=format!("Bearer {token}");}
        let mut auth=HeaderValue::from_str(&token).context("invalid token header")?;
        auth.set_sensitive(true);token.zeroize();
        eprintln!("[SUCCESS] TenSunS authentication ready");
        Ok(Self{client,base,auth})
    }
    fn get(&self,path:&str)->RequestBuilder {self.client.get(format!("{}{}",self.base,path)).header(ACCEPT,"application/json").header(AUTHORIZATION,self.auth.clone())}
    pub fn groups(&self)->Result<Vec<String>> {
        let mut v=send(self.get("/api/consul/services_name"),"services_name",true)?;
        let names:Vec<String>=serde_json::from_value(v["services_name"].take()).context("invalid services_name")?;
        ensure!(names.iter().all(|s|!s.is_empty()),"empty service name");
        Ok(names.into_iter().collect::<BTreeSet<_>>().into_iter().collect())
    }
    pub fn instances(&self,group:&str)->Result<Vec<Instance>> {
        let mut v=send(self.get("/api/consul/instances").query(&[("service_name",group)]),"instances",true).with_context(||format!("query service {group}"))?;
        let rows:Vec<Value>=serde_json::from_value(v["instances"].take()).context("invalid instances response")?;
        rows.into_iter().map(Instance::from_api).collect()
    }
    pub fn create(&self,row:&Instance)->Result<()> {
        send(self.client.post(format!("{}/api/consul/sid",self.base)).header(ACCEPT,"application/json").header(AUTHORIZATION,self.auth.clone()).json(&row.payload()?),"create",true)?;
        Ok(())
    }
}
fn send(request:RequestBuilder, operation:&str, show_body:bool)->Result<Value> {
    let response=request.send().with_context(||format!("transport error: {operation}"))?;
    let status=response.status();
    let body=response.bytes().context("read API response")?;
    let excerpt=||if show_body {String::from_utf8_lossy(&body[..body.len().min(1000)]).escape_debug().to_string()}else{"[login response redacted]".to_string()};
    if status.as_u16()!=200 {bail!("HTTP {status}: {operation}: {}",excerpt());}
    let v:Value=serde_json::from_slice(&body).with_context(||format!("non-JSON API response: {operation}: {}",excerpt()))?;
    if v.get("code").and_then(Value::as_i64)!=Some(20000){bail!("TenSunS business error: {operation}: {}",excerpt());}
    Ok(v)
}
