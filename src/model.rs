use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    #[serde(rename = "ID")]
    pub id: String,
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub address: String,
    #[serde(default, deserialize_with = "port_value")]
    pub port: u16,
    #[serde(default, deserialize_with = "null_default")]
    pub tags: Vec<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub meta: BTreeMap<String, String>,
}
fn null_default<'de, D, T>(d: D) -> Result<T, D::Error>
where D: serde::Deserializer<'de>, T: Deserialize<'de> + Default {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}
fn port_value<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
    use serde::de::Error;
    let v = Value::deserialize(d)?;
    match v {
        Value::Null => Ok(0),
        Value::String(s) if s.is_empty() => Ok(0),
        Value::String(s) => s.parse().map_err(D::Error::custom),
        Value::Number(n) => n.as_u64().and_then(|x| u16::try_from(x).ok()).ok_or_else(|| D::Error::custom("port must be an integer 0..65535")),
        _ => Err(D::Error::custom("invalid port")),
    }
}
impl Instance {
    pub fn canonicalize(&mut self) { self.tags.sort(); self.tags.dedup(); }
    pub fn validate(&self) -> Result<()> {
        let simple = |c: char| c.is_ascii_alphanumeric() || "_.-".contains(c);
        ensure!(!self.name.is_empty() && self.name != "consul" && self.name.chars().all(simple), "invalid service name: {}", self.name);
        ensure!(!self.id.is_empty() && self.id.chars().all(|c| simple(c) || "@/".contains(c)) && !self.id.starts_with('/') && !self.id.ends_with('/') && !self.id.contains("//"), "invalid ID: {}", self.id);
        ensure!(!self.address.chars().any(|c| c.is_whitespace() || c.is_control()), "invalid address: {}", self.id);
        ensure!(self.tags.iter().all(|s| !s.is_empty()), "empty tag: {}", self.id);
        ensure!(self.meta.keys().all(|s| !s.is_empty()), "empty Meta key: {}", self.id);
        Ok(())
    }
    pub fn from_api(v: Value) -> Result<Self> {
        let obj = v.as_object().context("instance response not an object")?;
        let tags = match obj.get("tags") {
            None | Some(Value::Null) => json!([]),
            Some(Value::String(s)) if s == "无" => json!([]),
            Some(Value::Array(a)) => json!(a),
            _ => bail!("invalid API tags"),
        };
        let meta = match obj.get("meta") {
            None | Some(Value::Null) => json!({}),
            Some(Value::String(s)) if s == "无" => json!({}),
            Some(Value::Array(a)) if a.len() == 1 && a[0].is_object() => a[0].clone(),
            _ => bail!("invalid API meta"),
        };
        let mut row: Self = serde_json::from_value(json!({"ID":obj.get("ID"),"name":obj.get("name"),"address":obj.get("address"),"port":obj.get("port"),"tags":tags,"meta":meta}))?;
        ensure!(!row.id.is_empty() && !row.name.is_empty(), "empty API ID/name");
        row.canonicalize();
        Ok(row)
    }
    pub fn payload(&self) -> Result<Value> {
        Ok(json!({"instance_dict": {"ID":self.id,"name":self.name,"address":self.address,"port":self.port,"tags":self.tags,
            "metaInfo":{"isMeta":!self.meta.is_empty(),"metaJson":serde_json::to_string(&self.meta)?},
            "checkInfo":{"isCheck":false,"ctype":"TCP","isAddress":"true","caddress":"","interval":"15s","timeout":"5s"}}}))
    }
}
