use crate::{BodyMode, Header, RawKind, Request, environment::Variable};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A folder created on its own (it may be empty). `before` says where it sits among the
/// requests: it is shown just ahead of the request at that index, or last when `before` is
/// the number of requests.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FolderMark {
    pub path: String,
    pub before: usize,
}
impl FolderMark {
    pub fn new(path: impl Into<String>, before: usize) -> Self {
        Self {
            path: path.into(),
            before,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Collection {
    pub name: String,
    pub requests: Vec<Request>,
    /// Folders created explicitly, so they exist even while empty (full " / " paths).
    pub folders: Vec<FolderMark>,
    /// Requests (full name) and folders (full path) shown first among their siblings.
    pub pinned: Vec<String>,
    pub description: String,
    pub variables: Vec<Variable>,
    pub headers: Vec<Header>,
    pub default_timeout: u32,
    pub follow_redirects: bool,
    pub insecure: bool,
}
impl Default for Collection {
    fn default() -> Self {
        Self {
            name: "My collection".into(),
            requests: vec![Request::default()],
            folders: vec![],
            pinned: vec![],
            description: String::new(),
            variables: vec![],
            headers: vec![],
            default_timeout: 30,
            follow_redirects: false,
            insecure: false,
        }
    }
}
pub fn export(c: &Collection) -> Value {
    let mut items = Vec::new();
    // Requests and empty folders are written in one pass so folders keep their place.
    let mut marks: Vec<&FolderMark> = c.folders.iter().collect();
    marks.sort_by_key(|m| m.before);
    let mut next = 0;
    let mut place_marks = |items: &mut Vec<Value>, upto: usize| {
        while next < marks.len() && marks[next].before <= upto {
            let parts: Vec<&str> = marks[next].path.split(" / ").collect();
            ensure_folder(items, &parts);
            next += 1;
        }
    };
    for (i, r) in c.requests.iter().enumerate() {
        place_marks(&mut items, i);
        let parts: Vec<&str> = r.name.split(" / ").collect();
        let item = json!({"name":parts.last().unwrap_or(&"Request"),"request":{"method":r.method,"url":r.url,
            "description":r.description,"header":r.headers.iter().map(|h|json!({"key":h.name,"value":h.value,"description":h.description,"disabled":!h.enabled})).collect::<Vec<_>>(),
            "auth":auth_json(&r.auth),"body":body_json(r)},"x-post-ronda":{"timeout":r.timeout,"follow_redirects":r.follow_redirects,"insecure":r.insecure,"query_notes":r.query_notes}});
        insert(&mut items, &parts[..parts.len() - 1], item);
    }
    place_marks(&mut items, usize::MAX);
    json!({"info":{"name":c.name,"description":c.description,"schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"variable":c.variables.iter().map(|v|json!({"key":v.name,"value":v.value,"disabled":!v.enabled})).collect::<Vec<_>>(),"x-post-ronda":{"headers":c.headers,"timeout":c.default_timeout,"follow_redirects":c.follow_redirects,"insecure":c.insecure,"pinned":c.pinned},"item":items})
}
/// Plain Postman Collection v2.1: the same document without this app's `x-post-ronda`
/// extension fields, for importing into Postman or other tools.
pub fn export_postman(c: &Collection) -> Value {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(map) => {
                map.remove("x-post-ronda");
                map.values_mut().for_each(strip);
            }
            Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut v = export(c);
    strip(&mut v);
    v
}
fn auth_json(a: &crate::Auth) -> Value {
    let pairs = |items: &[(&str, &str)]| -> Vec<Value> {
        items
            .iter()
            .map(|(k, v)| json!({"key": k, "value": v, "type": "string"}))
            .collect()
    };
    match a.kind {
        crate::AuthKind::None => json!({"type": "noauth"}),
        crate::AuthKind::Bearer => {
            json!({"type": "bearer", "bearer": pairs(&[("token", &a.token)])})
        }
        crate::AuthKind::Basic => {
            json!({"type": "basic", "basic": pairs(&[("username", &a.username), ("password", &a.password)])})
        }
        crate::AuthKind::ApiKey => json!({"type": "apikey", "apikey": pairs(&[
            ("key", &a.key),
            ("value", &a.value),
            ("in", if a.in_query { "query" } else { "header" }),
        ])}),
    }
}
fn rows_json(rows: &[Header], files: bool) -> Vec<Value> {
    rows.iter()
        .map(|h| {
            if files && h.file {
                json!({"key":h.name,"type":"file","src":h.value,"disabled":!h.enabled,"description":h.description})
            } else {
                json!({"key":h.name,"value":h.value,"type":"text","disabled":!h.enabled,"description":h.description})
            }
        })
        .collect()
}
fn body_json(r: &Request) -> Value {
    match r.body_mode {
        BodyMode::None => Value::Null,
        BodyMode::Raw => {
            let language = match r.raw_kind {
                RawKind::Json => "json",
                RawKind::Text => "text",
                RawKind::Xml => "xml",
                RawKind::Html => "html",
                RawKind::JavaScript => "javascript",
            };
            json!({"mode":"raw","raw":r.body,"options":{"raw":{"language":language}}})
        }
        BodyMode::UrlEncoded => {
            json!({"mode":"urlencoded","urlencoded":rows_json(&r.urlencoded, false)})
        }
        BodyMode::FormData => json!({"mode":"formdata","formdata":rows_json(&r.form, true)}),
        BodyMode::Binary => json!({"mode":"file","file":{"src":r.binary_path}}),
    }
}
fn rows_from_json(rows: &Value, files: bool) -> Vec<Header> {
    rows.as_array()
        .map(|a| {
            a.iter()
                .map(|f| {
                    let file = files && f["type"] == "file";
                    let value = if file {
                        f["src"]
                            .as_str()
                            .or_else(|| f["src"][0].as_str())
                            .unwrap_or("")
                    } else {
                        f["value"].as_str().unwrap_or("")
                    };
                    Header {
                        enabled: f["disabled"] != true,
                        name: f["key"].as_str().unwrap_or("").into(),
                        value: value.into(),
                        description: f["description"].as_str().unwrap_or("").into(),
                        file,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}
fn import_body(q: &Value, r: &mut Request) -> Result<(), String> {
    let body = &q["body"];
    match body["mode"].as_str() {
        None | Some("none") => r.body_mode = BodyMode::None,
        Some("raw") => {
            r.body_mode = BodyMode::Raw;
            r.body = body["raw"].as_str().unwrap_or("").into();
            r.raw_kind = match body["options"]["raw"]["language"].as_str() {
                Some("json") => RawKind::Json,
                Some("xml") => RawKind::Xml,
                Some("html") => RawKind::Html,
                Some("javascript") => RawKind::JavaScript,
                Some(_) => RawKind::Text,
                None => {
                    let ct = r
                        .headers
                        .iter()
                        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
                        .map(|h| h.value.as_str());
                    match ct {
                        Some(ct) => RawKind::from_content_type(ct),
                        None if r.body.trim_start().starts_with(['{', '[']) => RawKind::Json,
                        None => RawKind::Text,
                    }
                }
            };
        }
        Some("urlencoded") => {
            r.body_mode = BodyMode::UrlEncoded;
            r.urlencoded = rows_from_json(&body["urlencoded"], false);
        }
        Some("formdata") => {
            r.body_mode = BodyMode::FormData;
            r.form = rows_from_json(&body["formdata"], true);
        }
        Some("file") => {
            r.body_mode = BodyMode::Binary;
            r.binary_path = body["file"]["src"].as_str().unwrap_or("").into();
        }
        Some(mode) => {
            return Err(format!("Body mode '{mode}' is unsupported in {}", r.name));
        }
    }
    Ok(())
}
fn ensure_folder(items: &mut Vec<Value>, folders: &[&str]) {
    let Some(first) = folders.first() else {
        return;
    };
    let i = items
        .iter()
        .position(|v| v["name"] == *first && v["item"].is_array())
        .unwrap_or_else(|| {
            items.push(json!({"name":first,"item":[]}));
            items.len() - 1
        });
    ensure_folder(items[i]["item"].as_array_mut().unwrap(), &folders[1..]);
}
fn insert(items: &mut Vec<Value>, folders: &[&str], item: Value) {
    if folders.is_empty() {
        items.push(item);
        return;
    }
    let i = items
        .iter()
        .position(|v| v["name"] == folders[0] && v["item"].is_array())
        .unwrap_or_else(|| {
            items.push(json!({"name":folders[0],"item":[]}));
            items.len() - 1
        });
    insert(
        items[i]["item"].as_array_mut().unwrap(),
        &folders[1..],
        item,
    );
}
pub fn import(s: &str) -> Result<Collection, String> {
    import_with_notes(s).map(|(c, _)| c)
}
/// Like [`import`], plus what could not be carried over (scripts, auth that is not a plain
/// header), so the caller can say so instead of dropping it silently.
pub fn import_with_notes(s: &str) -> Result<(Collection, Vec<String>), String> {
    let mut v: Value = match serde_json::from_str(s) {
        Ok(v) => v,
        // OpenAPI documents are often YAML.
        Err(json_error) => match serde_yaml::from_str::<Value>(s) {
            Ok(v) if v.get("openapi").is_some() || v.get("swagger").is_some() => v,
            _ => return Err(json_error.to_string()),
        },
    };
    if v.get("openapi").is_some() || v.get("swagger").is_some() {
        return openapi::convert(&v);
    }
    // Postman's API (and backups made with it) wrap the collection as {"collection": {…}}.
    if v.get("item").is_none()
        && v.get("requests").is_none()
        && let Some(inner) = v
            .get_mut("collection")
            .map(Value::take)
            .filter(Value::is_object)
    {
        v = inner;
    }
    if v.get("requests").is_some() {
        let mut c: Collection = serde_json::from_value(v).map_err(|e| e.to_string())?;
        if c.requests.is_empty() {
            c.requests.push(Request::default());
        }
        return Ok((c, vec![]));
    }
    if !v["item"].is_array() {
        return Err(
            "Expected a Postman v2/v2.1 collection, an OpenAPI file or a Post Ronda collection"
                .into(),
        );
    }
    let mut notes = Notes::default();
    notes.scripts += script_count(&v);
    let mut c = Collection {
        name: v["info"]["name"]
            .as_str()
            .unwrap_or("Imported collection")
            .into(),
        requests: vec![],
        folders: vec![],
        pinned: serde_json::from_value(v["x-post-ronda"]["pinned"].clone()).unwrap_or_default(),
        description: v["info"]["description"].as_str().unwrap_or("").into(),
        variables: v["variable"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|v| Variable {
                        enabled: v["disabled"] != true,
                        name: v["key"].as_str().unwrap_or("").into(),
                        value: v["value"]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| v["value"].to_string()),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        headers: serde_json::from_value(v["x-post-ronda"]["headers"].clone()).unwrap_or_default(),
        default_timeout: v["x-post-ronda"]["timeout"]
            .as_u64()
            .unwrap_or(30)
            .clamp(1, 300) as u32,
        follow_redirects: v["x-post-ronda"]["follow_redirects"]
            .as_bool()
            .unwrap_or(false),
        insecure: v["x-post-ronda"]["insecure"].as_bool().unwrap_or(false),
    };
    walk(
        v["item"].as_array().unwrap(),
        "",
        &mut c.requests,
        &mut c.folders,
        &mut notes,
        &v["auth"],
    )?;
    if c.requests.is_empty() {
        c.requests.push(Request::default());
    }
    Ok((c, notes.messages()))
}
mod openapi;
/// Things an import had to leave out.
#[derive(Default)]
struct Notes {
    scripts: usize,
    auth: std::collections::BTreeSet<String>,
}
impl Notes {
    fn messages(&self) -> Vec<String> {
        let mut out = vec![];
        if self.scripts > 0 {
            out.push(format!(
                "{} script(s) skipped, scripts do not run here",
                self.scripts
            ));
        }
        if !self.auth.is_empty() {
            out.push(format!(
                "{} auth not converted, set it as a header",
                self.auth.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        out
    }
}
fn script_count(v: &Value) -> usize {
    v["event"].as_array().map_or(0, Vec::len)
}
/// A value of a Postman auth block, whether written as a list of {key, value} or as an object.
fn auth_value(auth: &Value, key: &str) -> String {
    let kind = auth["type"].as_str().unwrap_or("");
    let block = &auth[kind];
    let found = match block {
        Value::Array(a) => a.iter().find(|e| e["key"] == key).map(|e| &e["value"]),
        Value::Object(_) => Some(&block[key]),
        _ => None,
    };
    match found {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}
/// Sets a request's auth from the Postman auth block that applies to it. Types this app has no
/// setting for are left out and reported.
fn apply_auth(auth: &Value, r: &mut Request, notes: &mut Notes) {
    use crate::{Auth, AuthKind};
    let kind = auth["type"].as_str().unwrap_or("");
    r.auth = match kind {
        "" | "noauth" => Auth::default(),
        "bearer" => Auth {
            kind: AuthKind::Bearer,
            token: auth_value(auth, "token"),
            ..Auth::default()
        },
        "basic" => Auth {
            kind: AuthKind::Basic,
            username: auth_value(auth, "username"),
            password: auth_value(auth, "password"),
            ..Auth::default()
        },
        "apikey" => Auth {
            kind: AuthKind::ApiKey,
            key: auth_value(auth, "key"),
            value: auth_value(auth, "value"),
            in_query: auth_value(auth, "in") == "query",
            ..Auth::default()
        },
        other => {
            notes.auth.insert(other.to_owned());
            Auth::default()
        }
    };
}
fn walk(
    items: &[Value],
    prefix: &str,
    out: &mut Vec<Request>,
    folders: &mut Vec<FolderMark>,
    notes: &mut Notes,
    inherited: &Value,
) -> Result<(), String> {
    for item in items {
        notes.scripts += script_count(item);
        // A folder's auth covers what is inside it; "inherit" and a missing block defer upwards.
        let own = &item["auth"];
        let effective = if own.is_object() && own["type"] != "inherit" {
            own
        } else {
            inherited
        };
        let name = item["name"].as_str().unwrap_or("Request");
        let name = if prefix.is_empty() {
            name.into()
        } else {
            format!("{prefix} / {name}")
        };
        if let Some(children) = item["item"].as_array() {
            if children.is_empty() {
                folders.push(FolderMark::new(name.clone(), out.len()));
            }
            walk(children, &name, out, folders, notes, effective)?;
            continue;
        }
        let q = &item["request"];
        let request_auth = &q["auth"];
        let effective = if request_auth.is_object() && request_auth["type"] != "inherit" {
            request_auth
        } else {
            effective
        };
        let mut r = Request {
            name,
            ..Request::default()
        };
        if let Some(url) = q.as_str() {
            r.url = url.into();
        } else {
            r.method = q["method"].as_str().unwrap_or("GET").into();
            r.url = q["url"]
                .as_str()
                .or_else(|| q["url"]["raw"].as_str())
                .ok_or("Request URL is missing")?
                .into();
            r.description = q["description"]
                .as_str()
                .or_else(|| q["description"]["content"].as_str())
                .unwrap_or("")
                .into();
            if let Some(headers) = q["header"].as_array() {
                for h in headers {
                    r.headers.push(Header {
                        enabled: !h["disabled"].as_bool().unwrap_or(false),
                        name: h["key"].as_str().unwrap_or("").into(),
                        value: h["value"].as_str().unwrap_or("").into(),
                        description: h["description"].as_str().unwrap_or("").into(),
                        file: false,
                    });
                }
            }
            import_body(q, &mut r)?;
            apply_auth(effective, &mut r, notes);
        }
        let opts = &item["x-post-ronda"];
        r.timeout = opts["timeout"].as_u64().unwrap_or(30).clamp(1, 300) as u32;
        r.follow_redirects = opts["follow_redirects"].as_bool().unwrap_or(false);
        r.insecure = opts["insecure"].as_bool().unwrap_or(false);
        r.query_notes = serde_json::from_value(opts["query_notes"].clone()).unwrap_or_default();
        out.push(r);
    }
    Ok(())
}
pub fn markdown(c: &Collection) -> String {
    let mut s = format!("# {}\n\n{}\n\n", c.name, c.description);
    for r in &c.requests {
        let curl = crate::curl::export(r);
        let fence = "`".repeat(
            curl.split(|c| c != '`')
                .map(str::len)
                .max()
                .unwrap_or(0)
                .max(2)
                + 1,
        );
        s.push_str(&format!(
            "## {}\n\n**{}** `{}`\n\n{}\n\n{}bash\n{}\n{}\n\n",
            r.name, r.method, r.url, r.description, fence, curl, fence
        ));
    }
    s
}
#[cfg(test)]
mod tests {
    #![allow(clippy::field_reassign_with_default)]
    use super::*;
    #[test]
    fn every_body_mode_survives_export_and_import() {
        let mut file = Header::new("doc", "/tmp/a.pdf");
        file.file = true;
        let mut off = Header::new("off", "1");
        off.enabled = false;
        let mut c = Collection::default();
        c.requests = [
            (BodyMode::None, RawKind::Json),
            (BodyMode::Raw, RawKind::Xml),
            (BodyMode::UrlEncoded, RawKind::Json),
            (BodyMode::FormData, RawKind::Json),
            (BodyMode::Binary, RawKind::Json),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (mode, kind))| Request {
            name: format!("r{i}"),
            url: "https://a.test".into(),
            body_mode: mode,
            raw_kind: kind,
            body: if mode == BodyMode::Raw {
                "<a/>".into()
            } else {
                String::new()
            },
            urlencoded: if mode == BodyMode::UrlEncoded {
                vec![Header::new("k", "v w"), off.clone()]
            } else {
                vec![]
            },
            form: if mode == BodyMode::FormData {
                vec![Header::new("t", "x"), file.clone()]
            } else {
                vec![]
            },
            binary_path: if mode == BodyMode::Binary {
                "/tmp/b.bin".into()
            } else {
                String::new()
            },
            ..Request::default()
        })
        .collect();
        let again = import(&export(&c).to_string()).unwrap();
        assert_eq!(again.requests, c.requests);
        // Files saved before body modes existed keep their text as a raw body.
        let legacy = r#"{"name":"old","requests":[{"name":"a","method":"POST","url":"https://a.test","body":"{\"x\":1}"}]}"#;
        let old = import(legacy).unwrap();
        assert_eq!(old.requests[0].body_mode, BodyMode::Raw);
        assert_eq!(old.requests[0].body, "{\"x\":1}");
    }
    #[test]
    fn postman_export_is_plain_v2_1_and_still_imports() {
        let mut c = Collection::default();
        c.name = "Billing".into();
        c.requests[0].name = "Invoices / List".into();
        c.requests[0].url = "https://a.test/invoices".into();
        c.requests[0].timeout = 99;
        c.pinned = vec!["Invoices".into()];
        let v = export_postman(&c);
        assert!(!v.to_string().contains("x-post-ronda"));
        assert_eq!(v["info"]["name"], "Billing");
        assert!(v["info"]["schema"].as_str().unwrap().contains("v2.1.0"));
        assert_eq!(v["item"][0]["name"], "Invoices");
        let again = import(&v.to_string()).unwrap();
        assert_eq!(again.requests[0].name, "Invoices / List");
        assert_eq!(again.requests[0].url, "https://a.test/invoices");
    }
    #[test]
    fn empty_folders_survive_export_and_import() {
        let mut c = Collection::default();
        c.requests[0].name = "Billing / Invoices / List".into();
        c.folders = vec![
            FolderMark::new("Billing / Archive", 0),
            FolderMark::new("Drafts", 1),
        ];
        c.pinned = vec![
            "Billing / Invoices".into(),
            "Billing / Invoices / List".into(),
        ];
        let again = import(&export(&c).to_string()).unwrap();
        assert_eq!(again.pinned, c.pinned);
        assert_eq!(again.requests[0].name, "Billing / Invoices / List");
        assert_eq!(again.folders, c.folders);
    }
    #[test]
    fn roundtrip() {
        let mut c = Collection::default();
        c.requests[0].insecure = true;
        c.requests[0].url = "{{base_url}}/users".into();
        c.variables.push(Variable {
            enabled: true,
            name: "base_url".into(),
            value: "http://localhost".into(),
        });
        c.description = "API documentation".into();
        c.default_timeout = 45;
        c.headers.push(Header::new("X-Collection", "yes"));
        c.requests[0]
            .query_notes
            .insert("page".into(), "Page number".into());
        c.requests[0].headers.push(Header::new("X-Test", "hello"));
        let r = import(&export(&c).to_string()).unwrap();
        assert_eq!(r.requests, c.requests);
        assert_eq!(r.variables, c.variables);
        assert_eq!(r.headers, c.headers);
        assert_eq!(r.description, c.description);
        assert_eq!(r.default_timeout, 45);
    }
    #[test]
    fn folders() {
        let c=import(r#"{"info":{"name":"Demo"},"item":[{"name":"Users","item":[{"name":"List","request":{"method":"GET","url":{"raw":"http://localhost/users"}}}]}]}"#).unwrap();
        assert_eq!(c.requests[0].name, "Users / List");
        let v = export(&c);
        assert!(v["item"][0]["item"].is_array());
        assert_eq!(import(&v.to_string()).unwrap().requests, c.requests);
    }
    #[test]
    fn collections_wrapped_by_the_postman_api_import_like_plain_ones() {
        let plain = r#"{"info":{"name":"W"},"item":[{"name":"A","request":{"method":"GET","url":"http://a.test"}}]}"#;
        let wrapped = format!(r#"{{"collection":{plain}}}"#);
        assert_eq!(import(&wrapped).unwrap(), import(plain).unwrap());
        let env = crate::environment::Environment::from_json(
            r#"{"environment":{"name":"E","values":[{"key":"k","value":"v","enabled":true}]}}"#,
        )
        .unwrap();
        assert_eq!((env.name.as_str(), env.variables.len()), ("E", 1));
    }
    #[test]
    fn no_silent_script_loss() {
        let (_, notes) = import_with_notes(r#"{"item":[],"event":[{}]}"#).unwrap();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("script"));
        let (_, notes) = import_with_notes(
            r#"{"item":[{"name":"a","event":[{}],"request":{"url":"http://a.test"}}]}"#,
        )
        .unwrap();
        assert!(notes[0].contains("1 script"));
    }
    #[test]
    fn auth_is_kept_per_request_and_inherits_through_folders() {
        let text = r#"{"info":{"name":"A"},
          "auth":{"type":"bearer","bearer":[{"key":"token","value":"{{tok}}"}]},
          "item":[
            {"name":"Top","request":{"method":"GET","url":"http://a.test/x"}},
            {"name":"Own","request":{"method":"GET","url":"http://a.test/y","auth":{"type":"noauth"}}},
            {"name":"F","auth":{"type":"basic","basic":[{"key":"username","value":"u"},{"key":"password","value":"p"}]},
              "item":[
                {"name":"Inner","request":{"method":"GET","url":"http://a.test/z"}},
                {"name":"Key","request":{"method":"GET","url":"http://a.test/k?a=1","auth":{"type":"apikey","apikey":[{"key":"key","value":"k"},{"key":"value","value":"v"},{"key":"in","value":"query"}]}}},
                {"name":"Dig","request":{"method":"GET","url":"http://a.test/d","auth":{"type":"digest"}}}
              ]}
          ]}"#;
        let (c, notes) = import_with_notes(text).unwrap();
        let auth = |i: usize| c.requests[i].auth.clone();
        assert_eq!(auth(0).kind, crate::AuthKind::Bearer);
        assert_eq!(auth(0).token, "{{tok}}");
        assert_eq!(auth(1).kind, crate::AuthKind::None);
        assert_eq!(
            (
                auth(2).kind,
                auth(2).username.as_str(),
                auth(2).password.as_str()
            ),
            (crate::AuthKind::Basic, "u", "p")
        );
        assert_eq!(
            (
                auth(3).kind,
                auth(3).key.as_str(),
                auth(3).value.as_str(),
                auth(3).in_query
            ),
            (crate::AuthKind::ApiKey, "k", "v", true)
        );
        assert_eq!(auth(4).kind, crate::AuthKind::None);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("digest"));
        // Auth survives export and import.
        let again = import(&export_postman(&c).to_string()).unwrap();
        assert_eq!(
            again
                .requests
                .iter()
                .map(|r| r.auth.clone())
                .collect::<Vec<_>>(),
            c.requests
                .iter()
                .map(|r| r.auth.clone())
                .collect::<Vec<_>>()
        );
    }
}
