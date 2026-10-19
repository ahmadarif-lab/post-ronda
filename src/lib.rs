pub mod codegen;
pub mod collection;
pub mod crashlog;
pub mod curl;
pub mod environment;
pub mod html;
pub mod http;
pub mod process;
pub mod update;

/// One line that says what Post Ronda is: the welcome screen, About, README and package
/// metadata all use these words (a test keeps the files that cannot import it in step).
pub const TAGLINE: &str = "A fast, native, local-first API client. No account needed.";

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Header {
    pub enabled: bool,
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub description: String,
    /// Form-data rows only: the value is a file path instead of text.
    #[serde(default)]
    pub file: bool,
}
impl Header {
    pub fn new(name: &str, value: &str) -> Self {
        Self {
            enabled: true,
            name: name.into(),
            value: value.into(),
            description: String::new(),
            file: false,
        }
    }
}
/// Which body editor a request uses, as in Postman.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BodyMode {
    #[default]
    None,
    FormData,
    UrlEncoded,
    Raw,
    Binary,
}
/// Language of a raw body; decides the default Content-Type and syntax colouring.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RawKind {
    #[default]
    Json,
    Text,
    Xml,
    Html,
    JavaScript,
}
impl RawKind {
    pub const ALL: [RawKind; 5] = [
        RawKind::Json,
        RawKind::Text,
        RawKind::JavaScript,
        RawKind::Html,
        RawKind::Xml,
    ];
    pub fn label(self) -> &'static str {
        match self {
            RawKind::Json => "JSON",
            RawKind::Text => "Text",
            RawKind::Xml => "XML",
            RawKind::Html => "HTML",
            RawKind::JavaScript => "JavaScript",
        }
    }
    pub fn content_type(self) -> &'static str {
        match self {
            RawKind::Json => "application/json",
            RawKind::Text => "text/plain",
            RawKind::Xml => "application/xml",
            RawKind::Html => "text/html",
            RawKind::JavaScript => "application/javascript",
        }
    }
    /// Guess from a Content-Type header value.
    pub fn from_content_type(content_type: &str) -> Self {
        let ct = content_type.to_ascii_lowercase();
        if ct.contains("json") {
            RawKind::Json
        } else if ct.contains("xml") {
            RawKind::Xml
        } else if ct.contains("html") {
            RawKind::Html
        } else if ct.contains("javascript") {
            RawKind::JavaScript
        } else {
            RawKind::Text
        }
    }
}
/// Requests saved before body modes existed carry only `body`; they are raw bodies.
fn legacy_body_mode() -> BodyMode {
    BodyMode::Raw
}
/// How a request proves who is calling. Kept as its own setting instead of a hand-written
/// header, so it can be edited, exported and inherited on import as what it is.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthKind {
    #[default]
    None,
    ApiKey,
    Bearer,
    Basic,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Auth {
    pub kind: AuthKind,
    /// API key: name and value, sent as a header or as a query parameter.
    pub key: String,
    pub value: String,
    pub in_query: bool,
    /// Bearer token.
    pub token: String,
    /// Basic credentials.
    pub username: String,
    pub password: String,
}
impl Auth {
    pub fn is_set(&self) -> bool {
        self.kind != AuthKind::None
    }
}
/// Standard base64, as needed for the Basic scheme.
pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Request {
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<Header>,
    pub auth: Auth,
    /// Raw body text (used when `body_mode` is `Raw`).
    pub body: String,
    #[serde(default = "legacy_body_mode")]
    pub body_mode: BodyMode,
    pub raw_kind: RawKind,
    /// Rows of the form-data body.
    pub form: Vec<Header>,
    /// Rows of the x-www-form-urlencoded body.
    pub urlencoded: Vec<Header>,
    /// File sent as-is for the binary body.
    pub binary_path: String,
    pub description: String,
    pub follow_redirects: bool,
    pub insecure: bool,
    pub timeout: u32,
    pub query_notes: std::collections::BTreeMap<String, String>,
}
impl Default for Request {
    fn default() -> Self {
        Self {
            name: "Untitled request".into(),
            method: "GET".into(),
            url: String::new(),
            headers: vec![],
            auth: Auth::default(),
            body: String::new(),
            body_mode: BodyMode::None,
            raw_kind: RawKind::Json,
            form: vec![],
            urlencoded: vec![],
            binary_path: String::new(),
            description: String::new(),
            follow_redirects: false,
            insecure: false,
            timeout: 30,
            query_notes: Default::default(),
        }
    }
}

/// The URL as it will be sent: one typed without a scheme (`localhost:3030/api`, `example.com`)
/// is read as plain HTTP, like curl and browsers do. A leading `//` takes only the scheme.
pub fn with_scheme(url: &str) -> String {
    let url = url.trim();
    let has_scheme = url.split_once("://").is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    });
    if url.is_empty() || has_scheme {
        url.to_owned()
    } else if let Some(rest) = url.strip_prefix("//") {
        format!("http://{rest}")
    } else {
        format!("http://{url}")
    }
}
impl Request {
    /// The request with its auth written out as the header or query parameter it stands for.
    /// A header the user set by hand wins over the auth setting.
    pub fn with_auth(&self) -> Request {
        let mut r = self.clone();
        let a = &self.auth;
        let header = match a.kind {
            AuthKind::None => None,
            AuthKind::Bearer if !a.token.trim().is_empty() => Some((
                "Authorization".to_owned(),
                format!("Bearer {}", a.token.trim()),
            )),
            AuthKind::Basic if !a.username.is_empty() || !a.password.is_empty() => Some((
                "Authorization".to_owned(),
                format!(
                    "Basic {}",
                    base64(format!("{}:{}", a.username, a.password).as_bytes())
                ),
            )),
            AuthKind::ApiKey if !a.key.trim().is_empty() && !a.in_query => {
                Some((a.key.trim().to_owned(), a.value.clone()))
            }
            AuthKind::ApiKey if !a.key.trim().is_empty() => {
                let enc = |s: &str| {
                    url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>()
                };
                let pair = format!("{}={}", enc(a.key.trim()), enc(&a.value));
                let (base, fragment) = match r.url.split_once('#') {
                    Some((b, f)) => (b.to_owned(), format!("#{f}")),
                    None => (r.url.clone(), String::new()),
                };
                let join = if base.contains('?') {
                    if base.ends_with('?') || base.ends_with('&') {
                        ""
                    } else {
                        "&"
                    }
                } else {
                    "?"
                };
                r.url = format!("{base}{join}{pair}{fragment}");
                None
            }
            _ => None,
        };
        if let Some((name, value)) = header
            && !r.has_header(&name)
        {
            r.headers.push(Header::new(&name, &value));
        }
        r
    }
    /// Rows that are switched on and have a name.
    fn active_rows(rows: &[Header]) -> impl Iterator<Item = &Header> {
        rows.iter()
            .filter(|h| h.enabled && !h.name.trim().is_empty())
    }
    /// The x-www-form-urlencoded body as sent on the wire.
    pub fn urlencoded_body(&self) -> String {
        let mut out = url::form_urlencoded::Serializer::new(String::new());
        for h in Self::active_rows(&self.urlencoded) {
            out.append_pair(&h.name, &h.value);
        }
        out.finish()
    }
    pub fn has_header(&self, name: &str) -> bool {
        self.headers
            .iter()
            .any(|h| h.enabled && h.name.trim().eq_ignore_ascii_case(name))
    }
    /// Content-Type the body implies when the user has not set one. Multipart is left to curl,
    /// which adds the boundary itself.
    pub fn default_content_type(&self) -> Option<&'static str> {
        match self.body_mode {
            BodyMode::Raw if !self.body.is_empty() => Some(self.raw_kind.content_type()),
            BodyMode::UrlEncoded => Some("application/x-www-form-urlencoded"),
            BodyMode::Binary if !self.binary_path.is_empty() => Some("application/octet-stream"),
            _ => None,
        }
    }
    pub fn has_body(&self) -> bool {
        match self.body_mode {
            BodyMode::None => false,
            BodyMode::Raw => !self.body.is_empty(),
            BodyMode::UrlEncoded => Self::active_rows(&self.urlencoded).next().is_some(),
            BodyMode::FormData => Self::active_rows(&self.form).next().is_some(),
            BodyMode::Binary => !self.binary_path.is_empty(),
        }
    }
    pub fn form_fields(&self) -> impl Iterator<Item = &Header> {
        Self::active_rows(&self.form)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_tagline_reads_the_same_in_every_file_that_carries_it() {
        for (file, text) in [
            ("README.md", include_str!("../README.md")),
            ("Cargo.toml", include_str!("../Cargo.toml")),
            ("build.rs", include_str!("../build.rs")),
            (
                "scripts/bundle-linux.sh",
                include_str!("../scripts/bundle-linux.sh"),
            ),
        ] {
            assert!(text.contains(TAGLINE), "{file} has drifted from TAGLINE");
        }
    }
    #[test]
    fn auth_becomes_the_header_or_query_it_stands_for() {
        let with = |kind, f: &dyn Fn(&mut Auth)| {
            let mut r = Request {
                url: "http://a.test/x".into(),
                ..Default::default()
            };
            r.auth.kind = kind;
            f(&mut r.auth);
            r.with_auth()
        };
        let bearer = with(AuthKind::Bearer, &|a| a.token = " abc ".into());
        assert_eq!(bearer.headers[0].value, "Bearer abc");
        let basic = with(AuthKind::Basic, &|a| {
            a.username = "u".into();
            a.password = "p".into();
        });
        assert_eq!(basic.headers[0].value, "Basic dTpw");
        let header_key = with(AuthKind::ApiKey, &|a| {
            a.key = "X-Key".into();
            a.value = "v".into();
        });
        assert_eq!(
            (
                header_key.headers[0].name.as_str(),
                header_key.headers[0].value.as_str()
            ),
            ("X-Key", "v")
        );
        let query_key = with(AuthKind::ApiKey, &|a| {
            a.key = "api key".into();
            a.value = "a&b".into();
            a.in_query = true;
        });
        assert_eq!(query_key.url, "http://a.test/x?api+key=a%26b");
        assert!(query_key.headers.is_empty());
        // Nothing is added for empty values, and a hand-written header wins.
        assert!(with(AuthKind::Bearer, &|_| {}).headers.is_empty());
        assert!(with(AuthKind::None, &|_| {}).headers.is_empty());
        let mut r = Request::default();
        r.headers.push(Header::new("authorization", "Custom"));
        r.auth.kind = AuthKind::Bearer;
        r.auth.token = "t".into();
        assert_eq!(r.with_auth().headers.len(), 1);
        let mut q = Request {
            url: "http://a.test/x?a=1#top".into(),
            ..Default::default()
        };
        q.auth = Auth {
            kind: AuthKind::ApiKey,
            key: "k".into(),
            value: "v".into(),
            in_query: true,
            ..Default::default()
        };
        assert_eq!(q.with_auth().url, "http://a.test/x?a=1&k=v#top");
    }
    use super::with_scheme;
    #[test]
    fn urls_typed_without_a_scheme_are_plain_http() {
        assert_eq!(
            with_scheme("localhost:3030/a?b=1"),
            "http://localhost:3030/a?b=1"
        );
        assert_eq!(with_scheme(" example.com "), "http://example.com");
        assert_eq!(with_scheme("//example.com/x"), "http://example.com/x");
        assert_eq!(with_scheme("https://example.com"), "https://example.com");
        assert_eq!(with_scheme("HTTP://EXAMPLE.com"), "HTTP://EXAMPLE.com");
        assert_eq!(with_scheme("ftp://host/f"), "ftp://host/f");
        assert_eq!(with_scheme(""), "");
    }
}
