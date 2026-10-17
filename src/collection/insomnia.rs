//! Insomnia exports as a collection: the v4 JSON file (a flat list of `resources`) and the v5
//! YAML file (requests nested under `collection`). Folders become folders, the base
//! environment becomes the collection's variables.
use super::{Collection, FolderMark, Notes, Value};
use crate::{Auth, AuthKind, BodyMode, Header, RawKind, Request, environment::Variable};
use std::collections::{BTreeMap, BTreeSet};

/// Whether a parsed document is an Insomnia export (either generation).
pub fn is_export(v: &Value) -> bool {
    (v["_type"] == "export" && v["resources"].is_array())
        || v["type"]
            .as_str()
            .is_some_and(|t| t.starts_with("collection.insomnia.rest"))
}
/// `{{ _.token }}` is how Insomnia refers to an environment variable; here it is `{{token}}`.
fn variables(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = after[..end].trim();
        out.push_str("{{");
        out.push_str(inner.strip_prefix("_.").unwrap_or(inner));
        out.push_str("}}");
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => variables(s),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
fn query_escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'{' | b'}' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
fn rows(v: &Value, files: bool) -> Vec<Header> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|p| {
                    let file = files && p["type"] == "file";
                    Header {
                        enabled: p["disabled"] != true,
                        name: text(&p["name"]),
                        value: if file {
                            p["fileName"].as_str().unwrap_or("").into()
                        } else {
                            text(&p["value"])
                        },
                        description: p["description"].as_str().unwrap_or("").into(),
                        file,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}
/// One request, in the shape both generations share.
fn request(v: &Value, name: String, notes: &mut Notes) -> Request {
    let mut r = Request {
        name,
        method: v["method"].as_str().unwrap_or("GET").to_ascii_uppercase(),
        url: text(&v["url"]),
        description: v["description"]
            .as_str()
            .or_else(|| v["meta"]["description"].as_str())
            .unwrap_or("")
            .into(),
        headers: rows(&v["headers"], false),
        ..Request::default()
    };
    // Insomnia keeps query parameters apart from the URL; here they are part of it.
    let query: Vec<String> = rows(&v["parameters"], false)
        .into_iter()
        .filter(|p| p.enabled && !p.name.is_empty())
        .map(|p| format!("{}={}", query_escape(&p.name), query_escape(&p.value)))
        .collect();
    if !query.is_empty() {
        r.url.push(if r.url.contains('?') { '&' } else { '?' });
        r.url.push_str(&query.join("&"));
    }
    let body = &v["body"];
    match body["mimeType"].as_str().unwrap_or("") {
        "" => {}
        "application/x-www-form-urlencoded" => {
            r.body_mode = BodyMode::UrlEncoded;
            r.urlencoded = rows(&body["params"], false);
        }
        "multipart/form-data" => {
            r.body_mode = BodyMode::FormData;
            r.form = rows(&body["params"], true);
        }
        "application/octet-stream" => {
            r.body_mode = BodyMode::Binary;
            r.binary_path = body["fileName"].as_str().unwrap_or("").into();
        }
        mime => {
            r.body_mode = BodyMode::Raw;
            r.body = text(&body["text"]);
            r.raw_kind = if mime.contains("graphql") {
                RawKind::Json
            } else {
                RawKind::from_content_type(mime)
            };
        }
    }
    let auth = &v["authentication"];
    if auth["disabled"] != true {
        r.auth = match auth["type"].as_str().unwrap_or("") {
            "" | "none" => Auth::default(),
            "bearer" => Auth {
                kind: AuthKind::Bearer,
                token: text(&auth["token"]),
                ..Auth::default()
            },
            "basic" => Auth {
                kind: AuthKind::Basic,
                username: text(&auth["username"]),
                password: text(&auth["password"]),
                ..Auth::default()
            },
            "apikey" => Auth {
                kind: AuthKind::ApiKey,
                key: text(&auth["key"]),
                value: text(&auth["value"]),
                in_query: auth["addTo"] == "queryParams",
                ..Auth::default()
            },
            other => {
                notes.auth.insert(other.to_owned());
                Auth::default()
            }
        };
    }
    for field in ["preRequestScript", "afterResponseScript"] {
        if v[field].as_str().is_some_and(|s| !s.trim().is_empty()) {
            notes.scripts += 1;
        }
    }
    r
}
/// Plain string, number and boolean entries of an environment's `data`.
fn variables_of(data: &Value) -> Vec<Variable> {
    data.as_object()
        .map(|m| {
            m.iter()
                .filter(|(_, v)| !v.is_object() && !v.is_array() && !v.is_null())
                .map(|(k, v)| Variable {
                    enabled: true,
                    name: k.clone(),
                    value: text(v),
                })
                .collect()
        })
        .unwrap_or_default()
}
fn path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.into()
    } else {
        format!("{prefix} / {name}")
    }
}
#[derive(Default)]
struct Extras {
    /// Requests of kinds this app has no equivalent for (gRPC, WebSocket, …).
    unsupported: usize,
    environments: usize,
}
pub fn convert(v: &Value) -> Result<(Collection, Vec<String>), String> {
    let mut c = Collection {
        name: "Imported from Insomnia".into(),
        requests: vec![],
        ..Collection::default()
    };
    let mut notes = Notes::default();
    let mut extras = Extras::default();
    if v["_type"] == "export" {
        convert_v4(v, &mut c, &mut notes, &mut extras);
    } else {
        convert_v5(v, &mut c, &mut notes, &mut extras);
    }
    if c.requests.is_empty() {
        c.requests.push(Request::default());
    }
    let mut messages = notes.messages();
    if extras.unsupported > 0 {
        messages.push(format!(
            "{} gRPC or WebSocket request(s) skipped",
            extras.unsupported
        ));
    }
    if extras.environments > 0 {
        messages.push(format!(
            "{} sub-environment(s) skipped, only the base environment became variables",
            extras.environments
        ));
    }
    Ok((c, messages))
}
fn convert_v4(v: &Value, c: &mut Collection, notes: &mut Notes, extras: &mut Extras) {
    let resources: Vec<&Value> = v["resources"].as_array().into_iter().flatten().collect();
    let workspace = resources.iter().find(|r| r["_type"] == "workspace");
    if let Some(w) = workspace {
        if let Some(name) = w["name"].as_str().filter(|n| !n.is_empty()) {
            c.name = name.into();
        }
        c.description = w["description"].as_str().unwrap_or("").into();
    }
    let groups: BTreeSet<&str> = resources
        .iter()
        .filter(|r| r["_type"] == "request_group")
        .filter_map(|r| r["_id"].as_str())
        .collect();
    // Children by parent; whatever hangs off something that is not a folder sits at the top.
    let mut children: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for r in &resources {
        let kind = r["_type"].as_str().unwrap_or("");
        if kind.ends_with("request") || kind == "request_group" {
            let parent = r["parentId"].as_str().unwrap_or("");
            let key = if groups.contains(parent) { parent } else { "" };
            children.entry(key).or_default().push(r);
        }
    }
    for list in children.values_mut() {
        list.sort_by(|a, b| {
            let key = |r: &Value| r["metaSortKey"].as_f64().unwrap_or(0.);
            key(a).total_cmp(&key(b))
        });
    }
    fn walk(
        parent: &str,
        prefix: &str,
        children: &BTreeMap<&str, Vec<&Value>>,
        c: &mut Collection,
        notes: &mut Notes,
        extras: &mut Extras,
    ) {
        for item in children.get(parent).into_iter().flatten() {
            let name = path(prefix, item["name"].as_str().unwrap_or("Request"));
            match item["_type"].as_str().unwrap_or("") {
                "request_group" => {
                    let id = item["_id"].as_str().unwrap_or("");
                    if children.get(id).is_none_or(Vec::is_empty) {
                        c.folders
                            .push(FolderMark::new(name.clone(), c.requests.len()));
                    }
                    walk(id, &name, children, c, notes, extras);
                }
                "request" => c.requests.push(request(item, name, notes)),
                _ => extras.unsupported += 1,
            }
        }
    }
    walk("", "", &children, c, notes, extras);
    let base = workspace.and_then(|w| w["_id"].as_str());
    for env in resources.iter().filter(|r| r["_type"] == "environment") {
        if base.is_some_and(|id| env["parentId"] == id) {
            c.variables.extend(variables_of(&env["data"]));
        } else {
            extras.environments += 1;
        }
    }
}
fn convert_v5(v: &Value, c: &mut Collection, notes: &mut Notes, extras: &mut Extras) {
    if let Some(name) = v["name"].as_str().filter(|n| !n.is_empty()) {
        c.name = name.into();
    }
    c.description = v["meta"]["description"].as_str().unwrap_or("").into();
    fn walk(
        items: &Value,
        prefix: &str,
        c: &mut Collection,
        notes: &mut Notes,
        extras: &mut Extras,
    ) {
        for item in items.as_array().into_iter().flatten() {
            let name = path(prefix, item["name"].as_str().unwrap_or("Request"));
            if let Some(children) = item["children"].as_array() {
                if children.is_empty() {
                    c.folders
                        .push(FolderMark::new(name.clone(), c.requests.len()));
                }
                walk(&item["children"], &name, c, notes, extras);
            } else if item.get("url").is_some() || item.get("method").is_some() {
                c.requests.push(request(item, name, notes));
            } else {
                extras.unsupported += 1;
            }
        }
    }
    walk(&v["collection"], "", c, notes, extras);
    let env = &v["environments"];
    c.variables.extend(variables_of(&env["data"]));
    extras.environments += env["subEnvironments"].as_array().map_or(0, Vec::len);
}
