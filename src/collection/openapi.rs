//! OpenAPI 3.x and Swagger 2.0 documents (JSON or YAML) as a collection: one request per
//! operation, grouped into folders by tag, with the server address kept in a `baseUrl`
//! variable.
use super::{Collection, Value};
use crate::{Auth, AuthKind, BodyMode, Header, RawKind, Request, environment::Variable};
use std::collections::BTreeMap;

const METHODS: [&str; 7] = ["get", "put", "post", "delete", "patch", "head", "options"];

/// Follows a local `$ref` (`#/components/schemas/Name`); anything else is returned as is.
fn resolve<'a>(doc: &'a Value, mut v: &'a Value) -> &'a Value {
    for _ in 0..8 {
        let Some(pointer) = v["$ref"].as_str().and_then(|r| r.strip_prefix('#')) else {
            break;
        };
        match doc.pointer(&pointer.replace("~1", "/").replace("~0", "~")) {
            Some(next) => v = next,
            None => break,
        }
    }
    v
}
/// A small example value for a schema, so request bodies start from something sensible.
fn example(doc: &Value, schema: &Value, depth: usize) -> Value {
    let schema = resolve(doc, schema);
    if let Some(e) = schema.get("example") {
        return e.clone();
    }
    if let Some(d) = schema.get("default") {
        return d.clone();
    }
    if let Some(first) = schema["enum"].as_array().and_then(|a| a.first()) {
        return first.clone();
    }
    if depth > 4 {
        return Value::Null;
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(first) = schema[key].as_array().and_then(|a| a.first()) {
            return example(doc, first, depth + 1);
        }
    }
    if let Some(parts) = schema["allOf"].as_array() {
        let mut merged = serde_json::Map::new();
        for part in parts {
            if let Value::Object(map) = example(doc, part, depth + 1) {
                merged.extend(map);
            }
        }
        return Value::Object(merged);
    }
    let kind = schema["type"]
        .as_str()
        .unwrap_or(if schema.get("properties").is_some() {
            "object"
        } else {
            ""
        });
    match kind {
        "object" => {
            let mut map = serde_json::Map::new();
            if let Some(props) = schema["properties"].as_object() {
                for (name, prop) in props.iter().take(25) {
                    map.insert(name.clone(), example(doc, prop, depth + 1));
                }
            }
            Value::Object(map)
        }
        "array" => Value::Array(vec![example(doc, &schema["items"], depth + 1)]),
        "integer" | "number" => Value::from(0),
        "boolean" => Value::Bool(false),
        "string" => Value::String(
            match schema["format"].as_str() {
                Some("date-time") => "2024-01-01T00:00:00Z",
                Some("date") => "2024-01-01",
                Some("email") => "user@example.com",
                Some("uuid") => "00000000-0000-0000-0000-000000000000",
                Some("uri") | Some("url") => "https://example.com",
                _ => "string",
            }
            .into(),
        ),
        _ => Value::Null,
    }
}
fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
/// Keeps a name usable as one segment of a folder path.
fn segment(name: &str) -> String {
    let cleaned = name.replace(" / ", " - ").replace('/', "-");
    cleaned.trim().to_owned()
}
fn base_url(doc: &Value) -> String {
    if let Some(server) = doc["servers"][0]["url"].as_str() {
        let mut url = server.to_owned();
        if let Some(vars) = doc["servers"][0]["variables"].as_object() {
            for (name, var) in vars {
                url = url.replace(
                    &format!("{{{name}}}"),
                    var["default"].as_str().unwrap_or(""),
                );
            }
        }
        return url.trim_end_matches('/').to_owned();
    }
    if let Some(host) = doc["host"].as_str() {
        let scheme = doc["schemes"][0].as_str().unwrap_or("https");
        let base = doc["basePath"].as_str().unwrap_or("");
        return format!("{scheme}://{host}{}", base.trim_end_matches('/'));
    }
    String::new()
}
/// The auth setting for one security scheme, plus the variable it expects to find.
fn scheme_auth(
    scheme: &Value,
    notes: &mut Vec<String>,
    uses: &mut BTreeMap<&'static str, ()>,
) -> Auth {
    let kind = scheme["type"].as_str().unwrap_or("");
    match (
        kind,
        scheme["scheme"].as_str().map(str::to_lowercase).as_deref(),
    ) {
        ("http", Some("bearer")) => {
            uses.insert("token", ());
            Auth {
                kind: AuthKind::Bearer,
                token: "{{token}}".into(),
                ..Auth::default()
            }
        }
        ("http", Some("basic")) | ("basic", _) => {
            uses.insert("username", ());
            uses.insert("password", ());
            Auth {
                kind: AuthKind::Basic,
                username: "{{username}}".into(),
                password: "{{password}}".into(),
                ..Auth::default()
            }
        }
        ("apiKey", _) => {
            uses.insert("apiKey", ());
            Auth {
                kind: AuthKind::ApiKey,
                key: scheme["name"].as_str().unwrap_or("X-API-Key").into(),
                value: "{{apiKey}}".into(),
                in_query: scheme["in"].as_str() == Some("query"),
                ..Auth::default()
            }
        }
        (other, _) => {
            let label = format!("{other} security");
            if !notes.contains(&label) {
                notes.push(label);
            }
            Auth::default()
        }
    }
}
pub fn convert(doc: &Value) -> Result<(Collection, Vec<String>), String> {
    let paths = doc["paths"]
        .as_object()
        .ok_or("This OpenAPI document has no paths")?;
    let mut notes = vec![];
    let mut uses: BTreeMap<&'static str, ()> = BTreeMap::new();
    let schemes = if doc["components"]["securitySchemes"].is_object() {
        &doc["components"]["securitySchemes"]
    } else {
        &doc["securityDefinitions"]
    };
    // The first requirement's first scheme stands for "how to authenticate".
    let auth_for = |security: &Value,
                    notes: &mut Vec<String>,
                    uses: &mut BTreeMap<&'static str, ()>|
     -> Auth {
        let name = security[0].as_object().and_then(|m| m.keys().next());
        match name.and_then(|n| schemes.get(n.as_str())) {
            Some(scheme) => scheme_auth(resolve(doc, scheme), notes, uses),
            None => Auth::default(),
        }
    };
    let mut requests = vec![];
    for (path, item) in paths {
        let item = resolve(doc, item);
        for method in METHODS {
            let Some(op) = item.get(method).filter(|o| o.is_object()) else {
                continue;
            };
            let verb = method.to_uppercase();
            let leaf = op["summary"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| op["operationId"].as_str())
                .map(segment)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| segment(&format!("{verb} {path}")));
            let name = match op["tags"][0]
                .as_str()
                .map(segment)
                .filter(|t| !t.is_empty())
            {
                Some(tag) => format!("{tag} / {leaf}"),
                None => leaf,
            };
            let mut r = Request {
                name,
                method: verb,
                ..Request::default()
            };
            r.description = op["description"]
                .as_str()
                .or(op["summary"].as_str())
                .unwrap_or("")
                .to_owned();
            // Parameters: the path item's, then the operation's (which win by name and place).
            let mut parameters: Vec<&Value> = vec![];
            for list in [&item["parameters"], &op["parameters"]] {
                for p in list.as_array().into_iter().flatten() {
                    let p = resolve(doc, p);
                    parameters.retain(|q| !(q["name"] == p["name"] && q["in"] == p["in"]));
                    parameters.push(p);
                }
            }
            // `{id}` becomes `:id`, the path variable form used in Postman collections.
            let mut url_path = String::new();
            let mut rest = path.as_str();
            while let Some(open) = rest.find('{') {
                url_path.push_str(&rest[..open]);
                match rest[open..].find('}') {
                    Some(close) => {
                        url_path.push(':');
                        url_path.push_str(&rest[open + 1..open + close]);
                        rest = &rest[open + close + 1..];
                    }
                    None => {
                        rest = &rest[open..];
                        break;
                    }
                }
            }
            url_path.push_str(rest);
            let mut query = vec![];
            let mut form_params = vec![];
            let mut body_schema: Option<&Value> = None;
            for p in &parameters {
                let name = p["name"].as_str().unwrap_or("");
                let sample = p.get("example").cloned().unwrap_or_else(|| {
                    example(
                        doc,
                        if p.get("schema").is_some() {
                            &p["schema"]
                        } else {
                            p
                        },
                        0,
                    )
                });
                match p["in"].as_str().unwrap_or("") {
                    "query" if p["required"] == true => query.push(format!(
                        "{}={}",
                        url::form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>(),
                        url::form_urlencoded::byte_serialize(scalar(&sample).as_bytes())
                            .collect::<String>()
                    )),
                    "header" if !name.is_empty() => {
                        let mut h = Header::new(name, &scalar(&sample));
                        h.enabled = p["required"] == true;
                        h.description = p["description"].as_str().unwrap_or("").to_owned();
                        r.headers.push(h);
                    }
                    "formData" => {
                        form_params.push((name.to_owned(), p["type"] == "file", scalar(&sample)))
                    }
                    "body" => body_schema = Some(&p["schema"]),
                    _ => {}
                }
            }
            r.url = format!(
                "{{{{baseUrl}}}}{url_path}{}",
                if query.is_empty() {
                    String::new()
                } else {
                    format!("?{}", query.join("&"))
                }
            );
            // Body: OpenAPI 3 requestBody, Swagger 2 body / formData parameters.
            let request_body = resolve(doc, &op["requestBody"]);
            if let Some(content) = request_body["content"].as_object() {
                let pick = content
                    .keys()
                    .find(|k| k.contains("json"))
                    .or_else(|| content.keys().find(|k| k.contains("x-www-form-urlencoded")))
                    .or_else(|| content.keys().find(|k| k.contains("multipart")))
                    .or_else(|| content.keys().next());
                if let Some(kind) = pick {
                    let media = &content[kind.as_str()];
                    let sample = media
                        .get("example")
                        .cloned()
                        .or_else(|| {
                            media["examples"]
                                .as_object()
                                .and_then(|e| e.values().next())
                                .map(|e| resolve(doc, e)["value"].clone())
                        })
                        .unwrap_or_else(|| example(doc, &media["schema"], 0));
                    set_body(doc, &mut r, kind, &media["schema"], sample);
                }
            } else if let Some(schema) = body_schema {
                let consumes = op["consumes"][0]
                    .as_str()
                    .or(doc["consumes"][0].as_str())
                    .unwrap_or("application/json");
                set_body(doc, &mut r, consumes, schema, example(doc, schema, 0));
            } else if !form_params.is_empty() {
                let multipart = form_params.iter().any(|f| f.1)
                    || op["consumes"][0]
                        .as_str()
                        .or(doc["consumes"][0].as_str())
                        .is_some_and(|c| c.contains("multipart"));
                let rows: Vec<Header> = form_params
                    .iter()
                    .map(|(name, file, value)| {
                        let mut h = Header::new(name, value);
                        h.file = *file;
                        h
                    })
                    .collect();
                if multipart {
                    r.body_mode = BodyMode::FormData;
                    r.form = rows;
                } else {
                    r.body_mode = BodyMode::UrlEncoded;
                    r.urlencoded = rows;
                }
            }
            // The operation's own `security` wins over the document's, and `[]` means none.
            let security = if op.get("security").is_some() {
                &op["security"]
            } else {
                &doc["security"]
            };
            r.auth = auth_for(security, &mut notes, &mut uses);
            requests.push(r);
        }
    }
    if requests.is_empty() {
        return Err("This OpenAPI document has no operations".into());
    }
    let mut variables = vec![Variable {
        enabled: true,
        name: "baseUrl".into(),
        value: base_url(doc),
    }];
    for name in uses.keys() {
        variables.push(Variable {
            enabled: true,
            name: (*name).into(),
            value: String::new(),
        });
    }
    let collection = Collection {
        name: doc["info"]["title"]
            .as_str()
            .unwrap_or("OpenAPI")
            .to_owned(),
        requests,
        folders: vec![],
        pinned: vec![],
        description: doc["info"]["description"].as_str().unwrap_or("").to_owned(),
        variables,
        ..Collection::default()
    };
    Ok((collection, notes))
}
/// Fills the body of a request from a media type and its example value.
fn set_body(doc: &Value, r: &mut Request, media_type: &str, schema: &Value, sample: Value) {
    let form_rows = |sample: &Value| -> Vec<Header> {
        let schema = resolve(doc, schema);
        sample
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| {
                let mut h = Header::new(k, &scalar(v));
                h.file = resolve(doc, &schema["properties"][k.as_str()])["format"] == "binary";
                h
            })
            .collect()
    };
    if media_type.contains("multipart") {
        r.body_mode = BodyMode::FormData;
        r.form = form_rows(&sample);
    } else if media_type.contains("x-www-form-urlencoded") {
        r.body_mode = BodyMode::UrlEncoded;
        r.urlencoded = form_rows(&sample);
    } else if media_type.contains("json") {
        r.body_mode = BodyMode::Raw;
        r.raw_kind = RawKind::Json;
        r.body = serde_json::to_string_pretty(&sample).unwrap_or_default();
    } else {
        r.body_mode = BodyMode::Raw;
        r.raw_kind = RawKind::from_content_type(media_type);
        r.body = scalar(&sample);
    }
}
#[cfg(test)]
mod tests {
    use super::super::import_with_notes;
    #[test]
    fn openapi_yaml_becomes_requests_by_tag_with_auth_and_bodies() {
        let doc = r##"
openapi: 3.0.3
info: {title: Pets, description: Pet store}
servers:
  - url: https://{env}.pets.test/v1/
    variables: {env: {default: api}}
security: [{bearerAuth: []}]
components:
  securitySchemes:
    bearerAuth: {type: http, scheme: bearer}
    oauth: {type: oauth2}
  schemas:
    Pet:
      type: object
      properties:
        name: {type: string}
        age: {type: integer}
        tags: {type: array, items: {type: string}}
paths:
  /pets:
    get:
      tags: [Pets]
      summary: List pets
      parameters:
        - {name: limit, in: query, required: true, schema: {type: integer, default: 10}}
        - {name: offset, in: query, schema: {type: integer}}
    post:
      tags: [Pets]
      summary: Add pet
      requestBody:
        content:
          application/json:
            schema: {$ref: '#/components/schemas/Pet'}
  /pets/{petId}:
    parameters:
      - {name: petId, in: path, required: true, schema: {type: string}}
    delete:
      operationId: deletePet
      security: []
"##;
        let (c, notes) = import_with_notes(doc).unwrap();
        assert_eq!(c.name, "Pets");
        let names: Vec<_> = c.requests.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Pets / List pets", "Pets / Add pet", "deletePet"]);
        assert_eq!(c.variables[0].value, "https://api.pets.test/v1");
        assert_eq!(c.requests[0].url, "{{baseUrl}}/pets?limit=10");
        assert_eq!(c.requests[2].url, "{{baseUrl}}/pets/:petId");
        assert_eq!(c.requests[1].auth.kind, crate::AuthKind::Bearer);
        assert_eq!(c.requests[2].auth.kind, crate::AuthKind::None);
        assert_eq!(c.requests[1].body_mode, crate::BodyMode::Raw);
        let body: serde_json::Value = serde_json::from_str(&c.requests[1].body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"name": "string", "age": 0, "tags": ["string"]})
        );
        assert!(c.variables.iter().any(|v| v.name == "token"));
        assert!(notes.is_empty());
        // Swagger 2 with a body parameter and basic auth.
        let swagger = r#"{"swagger":"2.0","info":{"title":"Old"},"host":"old.test","basePath":"/api","schemes":["http"],
          "securityDefinitions":{"b":{"type":"basic"}},"security":[{"b":[]}],
          "paths":{"/x":{"put":{"parameters":[{"in":"body","name":"body","schema":{"type":"object","properties":{"a":{"type":"boolean"}}}}]}}}}"#;
        let (c, _) = import_with_notes(swagger).unwrap();
        assert_eq!(c.variables[0].value, "http://old.test/api");
        assert_eq!(c.requests[0].method, "PUT");
        assert_eq!(c.requests[0].auth.kind, crate::AuthKind::Basic);
        assert!(c.requests[0].body.contains("false"));
        // An unsupported scheme is reported.
        let oauth = r#"{"openapi":"3.0.0","components":{"securitySchemes":{"o":{"type":"oauth2"}}},"security":[{"o":[]}],"paths":{"/a":{"get":{}}}}"#;
        let (_, notes) = import_with_notes(oauth).unwrap();
        assert_eq!(notes, ["oauth2 security"]);
    }
}
