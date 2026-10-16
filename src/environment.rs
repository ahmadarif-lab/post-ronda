use crate::{Header, Request};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Variable {
    pub enabled: bool,
    pub name: String,
    pub value: String,
}
impl Default for Variable {
    fn default() -> Self {
        Self {
            enabled: true,
            name: String::new(),
            value: String::new(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Environment {
    pub name: String,
    pub variables: Vec<Variable>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Environments {
    pub active: Option<usize>,
    pub items: Vec<Environment>,
}
impl Environments {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(b) => {
                let mut e: Self = serde_json::from_slice(&b)
                    .map_err(|e| format!("Cannot load environments: {e}"))?;
                if e.active.is_some_and(|i| i >= e.items.len()) {
                    e.active = None;
                }
                Ok(e)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        use std::io::Write;
        if let Ok(original) = std::fs::read(path)
            && serde_json::from_slice::<Self>(&original).is_err()
        {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            std::fs::write(
                path.with_file_name(format!("environments.recovery-{stamp}.json")),
                original,
            )
            .map_err(|e| e.to_string())?;
        }
        let mut f = tempfile::NamedTempFile::new_in(path.parent().ok_or("Invalid path")?)
            .map_err(|e| e.to_string())?;
        f.write_all(&serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        f.persist(path).map_err(|e| e.to_string())?;
        Ok(())
    }
}
impl Environment {
    /// Reads an environment file: Postman's export (`values`) or this app's own format.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        // Postman's API wraps it as {"environment": {…}}.
        if v.get("values").is_none()
            && let Some(inner) = v
                .get_mut("environment")
                .map(serde_json::Value::take)
                .filter(serde_json::Value::is_object)
        {
            v = inner;
        }
        if v.get("values").is_some() {
            Ok(Self {
                name: v["name"].as_str().unwrap_or("Imported").into(),
                variables: v["values"]
                    .as_array()
                    .ok_or("Invalid values array")?
                    .iter()
                    .map(|v| Variable {
                        enabled: v["enabled"].as_bool().unwrap_or(true),
                        name: v["key"].as_str().unwrap_or("").into(),
                        value: v["value"].as_str().unwrap_or("").into(),
                    })
                    .collect(),
            })
        } else {
            serde_json::from_value::<Self>(v).map_err(|e| e.to_string())
        }
    }
}
pub fn resolve(
    request: &Request,
    collection: &[Variable],
    environment: &[Variable],
    headers: &[Header],
) -> Result<Request, String> {
    let mut vars = std::collections::HashMap::new();
    for v in collection
        .iter()
        .chain(environment)
        .filter(|v| v.enabled && !v.name.is_empty())
    {
        vars.insert(v.name.as_str(), v.value.as_str());
    }
    let expand = |s: &str| -> Result<String, String> {
        let mut current = s.to_owned();
        for _ in 0..16 {
            if !current.contains("{{") {
                return Ok(current);
            }
            let mut result = String::new();
            let mut rest = current.as_str();
            while let Some(start) = rest.find("{{") {
                result.push_str(&rest[..start]);
                let tail = &rest[start + 2..];
                let end = tail.find("}}").ok_or("Unclosed {{variable}}")?;
                let name = tail[..end].trim();
                result.push_str(vars.get(name).ok_or_else(|| {
                    format!(
                        "Variable '{name}' is undefined in the selected environment or collection"
                    )
                })?);
                rest = &tail[end + 2..];
            }
            result.push_str(rest);
            current = result;
        }
        Err("Variable cycle or nesting deeper than 16 levels".into())
    };
    let mut r = request.clone();
    r.url = expand(&r.url)?;
    r.body = expand(&r.body)?;
    r.binary_path = expand(&r.binary_path)?;
    for field in [
        &mut r.auth.key,
        &mut r.auth.value,
        &mut r.auth.token,
        &mut r.auth.username,
        &mut r.auth.password,
    ] {
        *field = expand(field)?;
    }
    // The request's own auth beats a collection header of the same name.
    let auth_header = match r.auth.kind {
        crate::AuthKind::Bearer | crate::AuthKind::Basic => Some("Authorization".to_owned()),
        crate::AuthKind::ApiKey if !r.auth.in_query => Some(r.auth.key.trim().to_owned()),
        _ => None,
    };
    for row in r
        .form
        .iter_mut()
        .chain(r.urlencoded.iter_mut())
        .filter(|h| h.enabled)
    {
        row.name = expand(&row.name)?;
        row.value = expand(&row.value)?;
    }
    let mut merged: Vec<Header> = headers
        .iter()
        .filter(|h| {
            h.enabled
                && !request
                    .headers
                    .iter()
                    .any(|r| r.name.eq_ignore_ascii_case(&h.name))
                && !auth_header
                    .as_ref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(&h.name))
        })
        .cloned()
        .collect();
    merged.extend(request.headers.clone());
    r.headers = merged;
    for h in r.headers.iter_mut().filter(|h| h.enabled) {
        h.name = expand(&h.name)?;
        h.value = expand(&h.value)?;
    }
    Ok(r)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn environment_persistence_and_recovery() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("environments.json");
        let e = Environments {
            active: Some(0),
            items: vec![Environment {
                name: "Local".into(),
                variables: vec![Variable {
                    enabled: true,
                    name: "port".into(),
                    value: "9417".into(),
                }],
            }],
        };
        e.save(&path).unwrap();
        let loaded = Environments::load(&path).unwrap();
        assert_eq!(loaded.active, Some(0));
        assert_eq!(loaded.items[0].variables[0].value, "9417");
        std::fs::write(&path, "broken JSON").unwrap();
        assert!(Environments::load(&path).is_err());
        e.save(&path).unwrap();
        assert!(
            std::fs::read_dir(root.path())
                .unwrap()
                .filter_map(Result::ok)
                .any(|f| f
                    .file_name()
                    .to_string_lossy()
                    .starts_with("environments.recovery-"))
        );
    }
    #[test]
    fn overrides_and_nested() {
        let collection = vec![
            Variable {
                enabled: true,
                name: "base".into(),
                value: "http://localhost".into(),
            },
            Variable {
                enabled: true,
                name: "token".into(),
                value: "old".into(),
            },
        ];
        let env = vec![
            Variable {
                enabled: true,
                name: "token".into(),
                value: "new".into(),
            },
            Variable {
                enabled: true,
                name: "endpoint".into(),
                value: "{{base}}/users".into(),
            },
        ];
        let r = Request {
            url: "{{endpoint}}".into(),
            headers: vec![Header::new("Authorization", "Bearer {{token}}")],
            ..Default::default()
        };
        let out = resolve(&r, &collection, &env, &[]).unwrap();
        assert_eq!(out.url, "http://localhost/users");
        assert_eq!(out.headers[0].value, "Bearer new");
        assert_eq!(r.url, "{{endpoint}}");
    }
    #[test]
    fn errors_and_disabled() {
        let vars = vec![Variable {
            enabled: false,
            name: "x".into(),
            value: "ok".into(),
        }];
        let r = Request {
            url: "{{x}}".into(),
            ..Default::default()
        };
        assert!(resolve(&r, &vars, &[], &[]).is_err());
        let vars = vec![Variable {
            enabled: true,
            name: "x".into(),
            value: "{{x}}".into(),
        }];
        assert!(resolve(&r, &vars, &[], &[]).unwrap_err().contains("cycle"));
    }
    #[test]
    fn inherits_headers() {
        let r = Request {
            url: "http://localhost".into(),
            headers: vec![Header::new("X-Test", "request")],
            ..Default::default()
        };
        let out = resolve(
            &r,
            &[],
            &[],
            &[
                Header::new("x-test", "collection"),
                Header::new("Accept", "application/json"),
            ],
        )
        .unwrap();
        assert_eq!(out.headers.len(), 2);
        assert_eq!(out.headers[1].value, "request");
    }
}
