use crate::{BodyMode, Request};
use std::{
    io::{Read, Write},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Largest request body (and header block) accepted.
const LIMIT: u64 = 10 * 1024 * 1024;
/// Largest response kept in memory; downloads such as spreadsheets can be big.
const RESPONSE_LIMIT: u64 = 200 * 1024 * 1024;
/// How much of a text response is turned into text for the viewer.
const TEXT_LIMIT: usize = 10 * 1024 * 1024;
#[derive(Clone, Debug)]
pub struct Response {
    pub status: String,
    pub headers: String,
    pub body: String,
    pub raw: Vec<u8>,
    pub elapsed: Duration,
    pub bytes: usize,
    /// The viewer shows only the first part of a very large text response.
    pub truncated: bool,
    /// Not text (spreadsheet, PDF, image, stream…): offered as a file instead of shown.
    pub binary: bool,
    pub content_type: String,
    /// Name suggested by the server's Content-Disposition, if any.
    pub file_name: String,
}
fn header_value(headers: &str, name: &str) -> String {
    headers
        .lines()
        .rev()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_owned())
        })
        .unwrap_or_default()
}
/// File name from `Content-Disposition: attachment; filename="report.xlsx"` (or `filename*=`).
fn disposition_file_name(disposition: &str) -> String {
    let lower = disposition.to_ascii_lowercase();
    if let Some(i) = lower.find("filename*=") {
        let value = disposition[i + 10..].split(';').next().unwrap_or("").trim();
        let encoded = value.rsplit('\'').next().unwrap_or(value);
        let bytes = url::form_urlencoded::parse(format!("x={encoded}").as_bytes())
            .next()
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        // form_urlencoded also turns '+' into spaces; names rarely contain '+'.
        if !bytes.is_empty() {
            return bytes;
        }
    }
    if let Some(i) = lower.find("filename=") {
        return disposition[i + 9..]
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"')
            .to_owned();
    }
    String::new()
}
fn is_text_type(content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    ct.starts_with("text/")
        || [
            "json",
            "xml",
            "javascript",
            "x-www-form-urlencoded",
            "yaml",
            "graphql",
            "html",
        ]
        .iter()
        .any(|t| ct.contains(t))
}
/// Decides between showing a response and offering it as a download.
fn looks_binary(content_type: &str, bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return true;
    }
    if content_type.is_empty() || is_text_type(content_type) {
        // Cut mid-character at the sniffing boundary is fine; only complete garbage counts.
        return std::str::from_utf8(head).is_err_and(|e| e.error_len().is_some());
    }
    true
}

/// Connection settings that belong to the app and the project rather than to one request.
#[derive(Clone, Debug, Default)]
pub struct Network {
    /// File curl reads cookies from and writes them back to, so sessions carry over.
    pub cookie_jar: Option<std::path::PathBuf>,
    /// Proxy URL; empty leaves it to curl's own environment (`HTTPS_PROXY` and friends).
    pub proxy: String,
    /// Extra certificate authority (PEM) to trust, for company-internal servers.
    pub ca_cert: String,
    /// Certificate (and key, when separate) presented to servers that ask for one (mTLS).
    pub client_cert: String,
    pub client_key: String,
}
pub fn execute(r: &Request, cancel: &Arc<AtomicBool>) -> Result<Response, String> {
    execute_with(r, cancel, &Network::default())
}
pub fn execute_with(
    r: &Request,
    cancel: &Arc<AtomicBool>,
    net: &Network,
) -> Result<Response, String> {
    let r = &Request {
        url: crate::with_scheme(&r.url),
        ..r.with_auth()
    };
    let parsed = url::Url::parse(&r.url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only HTTP(S) URLs are allowed".into());
    }
    if r.method.is_empty()
        || !r
            .method
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b'-')
    {
        return Err("Invalid HTTP method".into());
    }
    if r.body.len() as u64 > LIMIT {
        return Err("Request body limit is 10 MiB".into());
    }
    if r.body_mode == BodyMode::Binary && !r.binary_path.is_empty() {
        let size = std::fs::metadata(&r.binary_path)
            .map_err(|e| format!("Cannot read {}: {e}", r.binary_path))?
            .len();
        if size > LIMIT {
            return Err("Request body limit is 10 MiB".into());
        }
    }
    // What goes to curl's stdin; multipart and binary bodies are handed to curl as files instead.
    let stdin_body: Vec<u8> = match r.body_mode {
        BodyMode::Raw => r.body.clone().into_bytes(),
        BodyMode::UrlEncoded => r.urlencoded_body().into_bytes(),
        _ => vec![],
    };
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let body_path = dir.path().join("body");
    let header_path = dir.path().join("headers");
    let mut cmd = crate::process::command(if cfg!(target_os = "macos") {
        "/usr/bin/curl"
    } else {
        "curl"
    });
    // -q MUST be first: ignore ~/.curlrc. No shell and no arbitrary flags from pasted commands.
    cmd.args([
        "-q",
        "--silent",
        "--show-error",
        "--globoff",
        "--compressed",
        "--proto",
        "=http,https",
        "--proto-redir",
        "=http,https",
        "--max-time",
        &r.timeout.clamp(1, 300).to_string(),
        "--connect-timeout",
        "10",
        "--max-filesize",
        &RESPONSE_LIMIT.to_string(),
        "--dump-header",
    ])
    .arg(&header_path)
    .arg("--output")
    .arg(&body_path)
    .args(["--write-out", "%{http_code}"]);
    if r.method == "HEAD" {
        cmd.arg("--head");
    } else {
        cmd.args(["--request", &r.method]);
    }
    if r.follow_redirects {
        cmd.args(["--location", "--max-redirs", "10"]);
    }
    if r.insecure {
        cmd.arg("--insecure");
    }
    if let Some(jar) = &net.cookie_jar {
        cmd.arg("--cookie").arg(jar).arg("--cookie-jar").arg(jar);
    }
    let proxy = net.proxy.trim();
    if !proxy.is_empty() {
        if proxy.starts_with('-') || proxy.contains(char::is_whitespace) {
            return Err("The proxy must be a URL such as http://proxy.example:8080".into());
        }
        cmd.arg("--proxy").arg(proxy);
    }
    for (flag, path, what) in [
        ("--cacert", &net.ca_cert, "certificate authority"),
        ("--cert", &net.client_cert, "client certificate"),
        ("--key", &net.client_key, "client key"),
    ] {
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        if !std::path::Path::new(path).is_file() {
            return Err(format!(
                "Cannot read the {what} file ({path}). Check App settings > Network."
            ));
        }
        cmd.arg(flag).arg(path);
    }
    for h in r.headers.iter().filter(|h| h.enabled && !h.name.is_empty()) {
        if h.name.contains(['\r', '\n', ':']) || h.value.contains(['\r', '\n']) {
            return Err("Headers cannot contain line breaks or a colon in the name".into());
        }
        // curl writes the multipart Content-Type itself, boundary included.
        if r.body_mode == BodyMode::FormData && h.name.eq_ignore_ascii_case("content-type") {
            continue;
        }
        cmd.arg("--header").arg(format!("{}: {}", h.name, h.value));
    }
    if !r.has_header("content-type")
        && let Some(content_type) = r.default_content_type()
    {
        cmd.arg("--header")
            .arg(format!("Content-Type: {content_type}"));
    }
    match r.body_mode {
        BodyMode::Raw | BodyMode::UrlEncoded if !stdin_body.is_empty() => {
            cmd.args(["--data-binary", "@-"]);
        }
        BodyMode::Binary if !r.binary_path.is_empty() => {
            cmd.arg("--data-binary").arg(format!("@{}", r.binary_path));
        }
        BodyMode::FormData => {
            for field in r.form_fields() {
                if field.name.contains(['\r', '\n']) || field.value.contains(['\r', '\n']) {
                    return Err("Form fields cannot contain line breaks".into());
                }
                if field.file {
                    if !std::path::Path::new(&field.value).is_file() {
                        return Err(format!(
                            "Cannot read the file for \"{}\" ({}). Choose it again in the Body tab.",
                            field.name, field.value
                        ));
                    }
                    // Quoting keeps `;` and `,` in file names from being read as curl options.
                    let quoted = field.value.replace('\\', "\\\\").replace('"', "\\\"");
                    cmd.arg("--form")
                        .arg(format!("{}=@\"{quoted}\"", field.name));
                } else {
                    cmd.arg("--form-string")
                        .arg(format!("{}={}", field.name, field.value));
                }
            }
        }
        _ => {}
    }
    cmd.arg("--url")
        .arg(&r.url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let start = Instant::now();
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Cannot launch curl: {e}"))?;
    let input = child.stdin.take();
    let writer = std::thread::spawn(move || {
        if let Some(mut input) = input {
            let _ = input.write_all(&stdin_body);
        }
    });
    loop {
        let oversized = [&body_path, &header_path].iter().any(|p| {
            std::fs::metadata(p).is_ok_and(|m| {
                m.len()
                    > if *p == &body_path {
                        RESPONSE_LIMIT
                    } else {
                        LIMIT
                    }
            })
        });
        if cancel.load(Ordering::Relaxed)
            || oversized
            || start.elapsed() > Duration::from_secs(r.timeout.clamp(1, 300) as u64 + 2)
        {
            let _ = child.kill();
            let _ = child.wait();
            let _ = writer.join();
            return Err(if oversized {
                "Response exceeds 200 MiB limit"
            } else if cancel.load(Ordering::Relaxed) {
                "Request cancelled"
            } else {
                "Request timed out"
            }
            .into());
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = writer.join();
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let mut bytes = vec![];
    std::fs::File::open(&body_path)
        .map_err(|e| e.to_string())?
        .take(RESPONSE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let headers = std::fs::read_to_string(&header_path).map_err(|e| e.to_string())?;
    let status = headers
        .lines()
        .rfind(|s| s.starts_with("HTTP/"))
        .unwrap_or("No status")
        .to_owned();
    if bytes.len() as u64 > RESPONSE_LIMIT {
        return Err("Response exceeds 200 MiB limit".into());
    }
    let content_type = header_value(&headers, "content-type");
    let binary = looks_binary(&content_type, &bytes);
    let truncated = !binary && bytes.len() > TEXT_LIMIT;
    let body = if binary {
        String::new()
    } else {
        String::from_utf8_lossy(&bytes[..bytes.len().min(TEXT_LIMIT)]).into_owned()
    };
    Ok(Response {
        status,
        file_name: disposition_file_name(&header_value(&headers, "content-disposition")),
        content_type,
        headers,
        body,
        elapsed: start.elapsed(),
        bytes: bytes.len(),
        raw: bytes,
        truncated,
        binary,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    #[test]
    fn real_post_and_non_200() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut data = vec![];
            let mut b = [0; 4096];
            loop {
                let n = s.read(&mut b).unwrap();
                if n == 0 {
                    break;
                }
                data.extend_from_slice(&b[..n]);
                if data.ends_with(b"hello") {
                    break;
                }
            }
            let data = String::from_utf8(data).unwrap();
            assert!(data.starts_with("POST /test?x=1"));
            assert!(data.to_lowercase().contains("x-ronda: yes"));
            assert!(data.ends_with("hello"));
            s.write_all(b"HTTP/1.1 422 Unprocessable Entity\r\nContent-Length: 12\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"ok\":false}").unwrap();
        });
        let r = Request {
            method: "POST".into(),
            url: format!("http://{addr}/test?x=1"),
            body: "hello".into(),
            body_mode: BodyMode::Raw,
            headers: vec![crate::Header::new("X-Ronda", "yes")],
            ..Request::default()
        };
        let response = execute(&r, &Arc::new(AtomicBool::new(false))).unwrap();
        server.join().unwrap();
        assert!(response.status.contains("422"));
        assert_eq!(response.body, "{\"ok\":false}");
    }
    #[test]
    fn cookies_carry_over_through_the_jar_and_auth_reaches_the_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut heads = vec![];
            for n in 0..2 {
                let (mut s, _) = listener.accept().unwrap();
                s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut data = vec![];
                let mut b = [0; 4096];
                while !data.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = s.read(&mut b).unwrap();
                    data.extend_from_slice(&b[..n]);
                }
                heads.push(String::from_utf8(data).unwrap().to_lowercase());
                let cookie = if n == 0 {
                    "Set-Cookie: sid=abc123; Path=/\r\n"
                } else {
                    ""
                };
                s.write_all(format!("HTTP/1.1 200 OK\r\n{cookie}Content-Length: 2\r\nConnection: close\r\n\r\nok").as_bytes()).unwrap();
            }
            heads
        });
        let dir = tempfile::tempdir().unwrap();
        let net = Network {
            cookie_jar: Some(dir.path().join("jar.txt")),
            ..Network::default()
        };
        let mut r = Request {
            url: format!("http://{addr}/login"),
            ..Request::default()
        };
        r.auth = crate::Auth {
            kind: crate::AuthKind::Bearer,
            token: "tok".into(),
            ..Default::default()
        };
        let cancel = Arc::new(AtomicBool::new(false));
        execute_with(&r, &cancel, &net).unwrap();
        execute_with(&r, &cancel, &net).unwrap();
        let heads = server.join().unwrap();
        assert!(!heads[0].contains("cookie: sid"));
        assert!(heads[1].contains("cookie: sid=abc123"));
        assert!(heads[0].contains("authorization: bearer tok"));
        // Bad network settings are reported instead of being passed to curl.
        let bad = Network {
            proxy: "--evil".into(),
            ..Network::default()
        };
        assert!(
            execute_with(&r, &cancel, &bad)
                .unwrap_err()
                .contains("proxy")
        );
        let missing = Network {
            ca_cert: "/nonexistent/ca.pem".into(),
            ..Network::default()
        };
        assert!(
            execute_with(&r, &cancel, &missing)
                .unwrap_err()
                .contains("certificate authority")
        );
    }
    /// Serves one request and returns it (head and body) as lossy text.
    fn capture(request: Request) -> (String, Vec<u8>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/up", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut data = vec![];
            let mut b = [0; 4096];
            let (mut head_end, mut length) = (None, 0);
            loop {
                let n = s.read(&mut b).unwrap_or(0);
                data.extend_from_slice(&b[..n]);
                if head_end.is_none()
                    && let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n")
                {
                    head_end = Some(i + 4);
                    let head = String::from_utf8_lossy(&data[..i]).to_lowercase();
                    length = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                }
                if n == 0 || head_end.is_some_and(|h| data.len() >= h + length) {
                    break;
                }
            }
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
            data
        });
        let r = Request { url, ..request };
        execute(&r, &Arc::new(AtomicBool::new(false))).unwrap();
        let data = server.join().unwrap();
        let split = data.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        (
            String::from_utf8_lossy(&data[..split]).to_lowercase(),
            data[split..].to_vec(),
        )
    }
    #[test]
    fn body_modes_reach_the_wire_like_postman() {
        let header = |n: &str, v: &str| crate::Header::new(n, v);
        let base = Request {
            method: "POST".into(),
            ..Request::default()
        };
        // raw JSON gets its Content-Type automatically
        let (head, body) = capture(Request {
            body_mode: BodyMode::Raw,
            body: "{\"a\":1}".into(),
            ..base.clone()
        });
        assert!(head.contains("content-type: application/json"), "{head}");
        assert_eq!(body, b"{\"a\":1}");
        // urlencoded fields are percent-encoded, disabled rows skipped
        let mut off = header("skip", "me");
        off.enabled = false;
        let (head, body) = capture(Request {
            body_mode: BodyMode::UrlEncoded,
            urlencoded: vec![header("name", "Arif R"), off, header("age", "35")],
            ..base.clone()
        });
        assert!(
            head.contains("content-type: application/x-www-form-urlencoded"),
            "{head}"
        );
        assert_eq!(body, b"name=Arif+R&age=35");
        // a body switched off sends nothing, even with text left in the raw editor
        let (head, body) = capture(Request {
            body_mode: BodyMode::None,
            body: "stale".into(),
            ..base.clone()
        });
        assert!(
            body.is_empty() && !head.contains("content-type: application/json"),
            "{head}"
        );
        // multipart: text part, file part, and the boundary curl chose wins over a pasted header
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note;1.txt");
        std::fs::write(&file, "FILEDATA").unwrap();
        let mut upload = header("doc", &file.to_string_lossy());
        upload.file = true;
        let (head, body) = capture(Request {
            body_mode: BodyMode::FormData,
            form: vec![header("note", "@not-a-file"), upload],
            headers: vec![header("Content-Type", "application/json")],
            ..base.clone()
        });
        let body = String::from_utf8_lossy(&body);
        assert!(
            head.contains("content-type: multipart/form-data; boundary="),
            "{head}"
        );
        assert!(
            body.contains("name=\"note\"") && body.contains("@not-a-file"),
            "{body}"
        );
        assert!(
            body.contains("name=\"doc\"") && body.contains("FILEDATA"),
            "{body}"
        );
        // binary: file bytes as-is
        let blob = dir.path().join("blob.bin");
        std::fs::write(&blob, [0u8, 159, 146, 150, 255]).unwrap();
        let (head, body) = capture(Request {
            body_mode: BodyMode::Binary,
            binary_path: blob.to_string_lossy().into_owned(),
            ..base
        });
        assert!(
            head.contains("content-type: application/octet-stream"),
            "{head}"
        );
        assert_eq!(body, [0u8, 159, 146, 150, 255]);
    }
    #[test]
    fn spreadsheet_download_is_kept_as_bytes_with_the_server_file_name() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/export", listener.local_addr().unwrap());
        let payload: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        let sent = payload.clone();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut b = [0; 2048];
            let _ = s.read(&mut b);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\r\nContent-Disposition: attachment; filename=\"Laporan Mei.xlsx\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                sent.len()
            );
            s.write_all(head.as_bytes()).unwrap();
            s.write_all(&sent).unwrap();
        });
        let r = Request {
            url,
            ..Request::default()
        };
        let response = execute(&r, &Arc::new(AtomicBool::new(false))).unwrap();
        server.join().unwrap();
        assert!(response.binary);
        assert!(response.body.is_empty());
        assert_eq!(response.raw, payload);
        assert_eq!(response.file_name, "Laporan Mei.xlsx");
        assert_eq!(response.bytes, 5000);
    }
    #[test]
    fn text_and_file_name_helpers() {
        assert!(!looks_binary(
            "application/json; charset=utf-8",
            b"{\"a\":1}"
        ));
        assert!(!looks_binary("", "halo dunia — ✓".as_bytes()));
        assert!(looks_binary("", &[b'a', 0, b'b']));
        assert!(looks_binary("application/pdf", b"%PDF-1.7"));
        assert!(looks_binary("text/plain", &[0xff, 0xfe, 0xfd]));
        assert_eq!(
            disposition_file_name("attachment; filename=\"a b.pdf\""),
            "a b.pdf"
        );
        assert_eq!(
            disposition_file_name("attachment; filename*=UTF-8''Laporan%20Akhir.xlsx"),
            "Laporan Akhir.xlsx"
        );
        assert_eq!(disposition_file_name("inline"), "");
    }
    #[test]
    fn missing_upload_file_is_reported_before_sending() {
        let r = Request {
            method: "POST".into(),
            url: "http://127.0.0.1:9/x".into(),
            body_mode: BodyMode::Binary,
            binary_path: "/definitely/not/here.bin".into(),
            ..Request::default()
        };
        let err = execute(&r, &Arc::new(AtomicBool::new(false))).unwrap_err();
        assert!(err.contains("Cannot read"), "{err}");
    }
    #[test]
    fn cancellation() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let r = Request {
            url: format!("http://{}", listener.local_addr().unwrap()),
            ..Request::default()
        };
        let c = Arc::new(AtomicBool::new(true));
        assert!(execute(&r, &c).unwrap_err().contains("cancelled"));
    }
    #[test]
    fn rejects_other_protocol() {
        let r = Request {
            url: "file:///etc/passwd".into(),
            ..Request::default()
        };
        assert!(execute(&r, &Arc::new(AtomicBool::new(false))).is_err());
    }
}
