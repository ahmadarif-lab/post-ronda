//! Bruno collections: a folder of `.bru` files (what Bruno keeps on disk) or the JSON file
//! of Bruno's "Export collection". Folders stay folders, the order Bruno shows is kept.
use super::{Collection, FolderMark, Notes, Value};
use crate::{Auth, AuthKind, BodyMode, Header, RawKind, Request, environment::Variable};
use std::path::{Path, PathBuf};

const METHODS: [&str; 9] = [
    "get", "post", "put", "delete", "patch", "options", "head", "connect", "trace",
];
/// How deep folders are followed, so a link back to a parent cannot loop forever.
const MAX_DEPTH: usize = 24;

/// Whether `dir` is a Bruno collection on disk.
pub fn is_folder(dir: &Path) -> bool {
    dir.join("bruno.json").is_file()
}
/// Whether a parsed document is the JSON export of a Bruno collection.
pub fn is_export(v: &Value) -> bool {
    v["items"].is_array() && v["name"].is_string() && v.get("version").is_some()
}
/// What Bruno shows for a path parameter: `/users/:id` with `id` set to `5` is `/users/5`.
fn fill_path_params(url: &str, params: &[(String, String)]) -> String {
    let mut url = url.to_owned();
    for (name, value) in params
        .iter()
        .filter(|(n, v)| !n.is_empty() && !v.is_empty())
    {
        let marker = format!("/:{name}");
        let mut out = String::new();
        let mut rest = url.as_str();
        while let Some(i) = rest.find(&marker) {
            let after = &rest[i + marker.len()..];
            // Only a whole segment: `:id` must not hit `:identity`.
            if after.is_empty() || after.starts_with(['/', '?', '#']) {
                out.push_str(&rest[..i]);
                out.push('/');
                out.push_str(value);
            } else {
                out.push_str(&rest[..i + marker.len()]);
            }
            rest = after;
        }
        out.push_str(rest);
        url = out;
    }
    url
}
fn raw_kind(mode: &str) -> RawKind {
    match mode {
        "json" | "graphql" => RawKind::Json,
        "xml" => RawKind::Xml,
        "html" => RawKind::Html,
        "javascript" => RawKind::JavaScript,
        _ => RawKind::Text,
    }
}
fn apply_auth(mode: &str, get: &dyn Fn(&str) -> String, r: &mut Request, notes: &mut Notes) {
    r.auth = match mode {
        "" | "none" | "inherit" => Auth::default(),
        "bearer" => Auth {
            kind: AuthKind::Bearer,
            token: get("token"),
            ..Auth::default()
        },
        "basic" => Auth {
            kind: AuthKind::Basic,
            username: get("username"),
            password: get("password"),
            ..Auth::default()
        },
        "apikey" => Auth {
            kind: AuthKind::ApiKey,
            key: get("key"),
            value: get("value"),
            in_query: get("placement") == "queryparams",
            ..Auth::default()
        },
        other => {
            notes.auth.insert(other.to_owned());
            Auth::default()
        }
    };
}

// ---- The JSON export ------------------------------------------------------------------

fn rows(v: &Value, files: bool) -> Vec<Header> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|p| {
                    let file = files && p["type"] == "file";
                    let value = match &p["value"] {
                        Value::Array(paths) => paths.first().and_then(Value::as_str).unwrap_or(""),
                        other => other.as_str().unwrap_or(""),
                    };
                    Header {
                        enabled: p["enabled"] != false,
                        name: p["name"].as_str().unwrap_or("").into(),
                        value: value.into(),
                        description: p["description"].as_str().unwrap_or("").into(),
                        file,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}
fn json_request(item: &Value, name: String, notes: &mut Notes) -> Request {
    let q = &item["request"];
    let path_params: Vec<(String, String)> = q["params"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["type"] == "path")
        .map(|p| {
            (
                p["name"].as_str().unwrap_or("").to_owned(),
                p["value"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect();
    let mut r = Request {
        name,
        method: q["method"].as_str().unwrap_or("GET").to_ascii_uppercase(),
        url: fill_path_params(q["url"].as_str().unwrap_or(""), &path_params),
        description: q["docs"].as_str().unwrap_or("").into(),
        headers: rows(&q["headers"], false),
        ..Request::default()
    };
    let body = &q["body"];
    match body["mode"].as_str().unwrap_or("none") {
        "none" => {}
        "formUrlEncoded" => {
            r.body_mode = BodyMode::UrlEncoded;
            r.urlencoded = rows(&body["formUrlEncoded"], false);
        }
        "multipartForm" => {
            r.body_mode = BodyMode::FormData;
            r.form = rows(&body["multipartForm"], true);
        }
        mode => {
            r.body_mode = BodyMode::Raw;
            r.raw_kind = raw_kind(mode);
            let key = if mode == "graphql" { "text" } else { mode };
            r.body = body[key].as_str().unwrap_or("").into();
        }
    }
    let auth = &q["auth"];
    let mode = auth["mode"].as_str().unwrap_or("");
    apply_auth(
        mode,
        &|key| auth[mode][key].as_str().unwrap_or("").to_owned(),
        &mut r,
        notes,
    );
    for script in [&q["script"]["req"], &q["script"]["res"], &q["tests"]] {
        if script.as_str().is_some_and(|s| !s.trim().is_empty()) {
            notes.scripts += 1;
        }
    }
    r
}
fn sequence(item: &Value) -> i64 {
    item["seq"].as_i64().unwrap_or(i64::MAX)
}
fn walk_export(
    items: &Value,
    prefix: &str,
    c: &mut Collection,
    notes: &mut Notes,
    skipped: &mut usize,
) {
    let mut sorted: Vec<&Value> = items.as_array().into_iter().flatten().collect();
    sorted.sort_by_key(|i| sequence(i));
    for item in sorted {
        let name = item["name"].as_str().unwrap_or("Request");
        let name = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix} / {name}")
        };
        match item["type"].as_str().unwrap_or("") {
            "folder" => {
                if item["items"].as_array().is_none_or(Vec::is_empty) {
                    c.folders
                        .push(FolderMark::new(name.clone(), c.requests.len()));
                }
                walk_export(&item["items"], &name, c, notes, skipped);
            }
            "http-request" | "graphql-request" => {
                c.requests.push(json_request(item, name, notes));
            }
            _ => *skipped += 1,
        }
    }
}
pub fn convert_export(v: &Value) -> Result<(Collection, Vec<String>), String> {
    let mut c = Collection {
        name: v["name"].as_str().unwrap_or("Imported from Bruno").into(),
        requests: vec![],
        ..Collection::default()
    };
    let mut notes = Notes::default();
    let mut skipped = 0;
    walk_export(&v["items"], "", &mut c, &mut notes, &mut skipped);
    if c.requests.is_empty() {
        c.requests.push(Request::default());
    }
    let mut messages = notes.messages();
    if skipped > 0 {
        messages.push(format!("{skipped} gRPC or WebSocket request(s) skipped"));
    }
    let environments = v["environments"].as_array().map_or(0, Vec::len);
    if environments > 0 {
        messages.push(format!("{environments} environment(s) not imported"));
    }
    Ok((c, messages))
}

// ---- The .bru files -------------------------------------------------------------------

/// A `.bru` file as its blocks: `name { … }`, each with its lines (indent removed).
fn blocks(text: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = vec![];
    let mut open: Option<(String, Vec<String>)> = None;
    let mut list = false;
    for line in text.lines() {
        match &mut open {
            Some((_, lines)) => {
                if line.trim_end() == "}" {
                    out.extend(open.take());
                } else {
                    lines.push(line.strip_prefix("  ").unwrap_or(line).to_owned());
                }
            }
            None if list => list = line.trim_end() != "]",
            None => {
                let line = line.trim_end();
                if let Some(name) = line.strip_suffix('{').filter(|_| !line.starts_with(' ')) {
                    open = Some((name.trim().to_owned(), vec![]));
                } else if line.ends_with('[') && !line.starts_with(' ') {
                    // `tags [ … ]` and the like: nothing here is a request setting.
                    list = true;
                }
            }
        }
    }
    out
}
/// The `key: value` lines of a block; a leading `~` marks a disabled entry.
fn pairs(lines: &[String]) -> Vec<(bool, String, String)> {
    lines
        .iter()
        .filter_map(|l| {
            let l = l.trim();
            let (enabled, l) = match l.strip_prefix('~') {
                Some(rest) => (false, rest.trim_start()),
                None => (true, l),
            };
            let (key, value) = l.split_once(':')?;
            Some((enabled, key.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}
fn pair_rows(lines: &[String], files: bool) -> Vec<Header> {
    pairs(lines)
        .into_iter()
        .map(|(enabled, name, value)| {
            let path = value
                .strip_prefix("@file(")
                .and_then(|v| v.strip_suffix(')'));
            let file = files && path.is_some();
            Header {
                enabled,
                name,
                value: path.map_or(value.clone(), str::to_owned),
                description: String::new(),
                file,
            }
        })
        .collect()
}
fn lookup(lines: &[String], key: &str) -> String {
    pairs(lines)
        .into_iter()
        .find(|(_, k, _)| k == key)
        .map(|(_, _, v)| v)
        .unwrap_or_default()
}
struct Parsed {
    request: Option<Request>,
    seq: i64,
    name: String,
    /// Settings that belong to the collection or folder rather than one request.
    headers: Vec<Header>,
    variables: Vec<Variable>,
    docs: String,
}
fn parse_bru(text: &str, notes: &mut Notes) -> Parsed {
    let blocks = blocks(text);
    let block = |name: &str| {
        blocks
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, l)| l.as_slice())
    };
    let meta = block("meta").unwrap_or(&[]);
    let mut parsed = Parsed {
        request: None,
        seq: lookup(meta, "seq").parse().unwrap_or(i64::MAX),
        name: lookup(meta, "name"),
        headers: block("headers")
            .map(|l| pair_rows(l, false))
            .unwrap_or_default(),
        variables: block("vars:pre-request")
            .map(|l| {
                pairs(l)
                    .into_iter()
                    .map(|(enabled, name, value)| Variable {
                        enabled,
                        name,
                        value,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        docs: block("docs")
            .map(|l| l.join("\n").trim().to_owned())
            .unwrap_or_default(),
    };
    let Some((method, verb)) = METHODS
        .iter()
        .find_map(|m| block(m).map(|lines| (m.to_ascii_uppercase(), lines)))
    else {
        return parsed;
    };
    let path_params: Vec<(String, String)> = block("params:path")
        .map(|l| pairs(l).into_iter().map(|(_, k, v)| (k, v)).collect())
        .unwrap_or_default();
    let mut r = Request {
        name: parsed.name.clone(),
        method,
        url: fill_path_params(&lookup(verb, "url"), &path_params),
        description: parsed.docs.clone(),
        headers: parsed.headers.clone(),
        ..Request::default()
    };
    match lookup(verb, "body").as_str() {
        "" | "none" => {}
        "formUrlEncoded" => {
            r.body_mode = BodyMode::UrlEncoded;
            r.urlencoded = block("body:form-urlencoded")
                .map(|l| pair_rows(l, false))
                .unwrap_or_default();
        }
        "multipartForm" => {
            r.body_mode = BodyMode::FormData;
            r.form = block("body:multipart-form")
                .map(|l| pair_rows(l, true))
                .unwrap_or_default();
        }
        "graphql" => {
            r.body_mode = BodyMode::Raw;
            r.raw_kind = RawKind::Json;
            let query = block("body:graphql")
                .map(|l| l.join("\n"))
                .unwrap_or_default();
            let vars = block("body:graphql:vars")
                .map(|l| l.join("\n"))
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            r.body = serde_json::to_string_pretty(
                &serde_json::json!({"query": query.trim(), "variables": vars}),
            )
            .unwrap_or_default();
        }
        mode => {
            r.body_mode = BodyMode::Raw;
            r.raw_kind = raw_kind(mode);
            r.body = block(&format!("body:{mode}"))
                .map(|l| l.join("\n").trim_matches('\n').to_owned())
                .unwrap_or_default();
        }
    }
    let mode = lookup(verb, "auth");
    let auth = block(&format!("auth:{mode}")).unwrap_or(&[]);
    apply_auth(&mode, &|key| lookup(auth, key), &mut r, notes);
    for script in ["script:pre-request", "script:post-response", "tests"] {
        if block(script).is_some_and(|l| l.iter().any(|s| !s.trim().is_empty())) {
            notes.scripts += 1;
        }
    }
    parsed.request = Some(r);
    parsed
}
fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}
/// A folder (no `request`) or a request found in a directory, before ordering.
struct Entry {
    seq: i64,
    name: String,
    path: PathBuf,
    request: Option<Request>,
}
fn walk_dir(
    dir: &Path,
    prefix: &str,
    depth: usize,
    c: &mut Collection,
    notes: &mut Notes,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let mut entries = vec![];
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned());
        let Some(file) = file else { continue };
        if path.is_dir() {
            // Folders hold requests; these hold something else.
            if matches!(file.as_str(), "environments" | "node_modules") || file.starts_with('.') {
                continue;
            }
            let settings = path.join("folder.bru");
            let parsed = std::fs::read_to_string(&settings)
                .map(|t| parse_bru(&t, notes))
                .ok();
            entries.push(Entry {
                seq: parsed.as_ref().map_or(i64::MAX, |p| p.seq),
                name: parsed
                    .map(|p| p.name)
                    .filter(|n| !n.is_empty())
                    .unwrap_or(file),
                path,
                request: None,
            });
        } else if file.ends_with(".bru")
            && !matches!(file.as_str(), "collection.bru" | "folder.bru")
        {
            let parsed = parse_bru(&read(&path)?, notes);
            let Some(request) = parsed.request else {
                continue;
            };
            entries.push(Entry {
                seq: parsed.seq,
                name: if request.name.is_empty() {
                    file.trim_end_matches(".bru").to_owned()
                } else {
                    request.name.clone()
                },
                path,
                request: Some(request),
            });
        }
    }
    entries.sort_by(|a, b| (a.seq, &a.name).cmp(&(b.seq, &b.name)));
    for entry in entries {
        let name = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{prefix} / {}", entry.name)
        };
        match entry.request {
            None => {
                let before = c.requests.len();
                walk_dir(&entry.path, &name, depth + 1, c, notes)?;
                if c.requests.len() == before {
                    c.folders.push(FolderMark::new(name, before));
                }
            }
            Some(mut request) => {
                request.name = name;
                c.requests.push(request);
            }
        }
    }
    Ok(())
}
/// Reads a Bruno collection folder (the one with `bruno.json`).
pub fn import_folder(dir: &Path) -> Result<(Collection, Vec<String>), String> {
    if !is_folder(dir) {
        let hint = if dir.join("opencollection.yml").is_file() {
            "This is Bruno's YAML (OpenCollection) format, which is not supported yet. In Bruno, export the collection as JSON and import that file."
        } else {
            "That folder is not a Bruno collection: it has no bruno.json."
        };
        return Err(hint.into());
    }
    let config: Value = serde_json::from_str(&read(&dir.join("bruno.json"))?)
        .map_err(|e| format!("bruno.json: {e}"))?;
    let mut c = Collection {
        name: config["name"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Imported from Bruno".into()),
        requests: vec![],
        ..Collection::default()
    };
    let mut notes = Notes::default();
    if let Ok(text) = std::fs::read_to_string(dir.join("collection.bru")) {
        let settings = parse_bru(&text, &mut notes);
        c.headers = settings.headers;
        c.variables = settings.variables;
        c.description = settings.docs;
    }
    walk_dir(dir, "", 0, &mut c, &mut notes)?;
    if c.requests.is_empty() {
        c.requests.push(Request::default());
    }
    let mut messages = notes.messages();
    let environments = std::fs::read_dir(dir.join("environments"))
        .map(|d| {
            d.filter_map(Result::ok)
                .filter(|e| e.path().extension().is_some_and(|x| x == "bru"))
                .count()
        })
        .unwrap_or(0);
    if environments > 0 {
        messages.push(format!("{environments} environment(s) not imported"));
    }
    Ok((c, messages))
}
