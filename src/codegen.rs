use crate::{BodyMode, Header, Request};

/// Language name and the clients available for it, in the order shown in the dialog.
pub const LANGUAGES: &[(&str, &[&str])] = &[
    ("Shell", &["curl", "httpie", "wget"]),
    ("HTTP", &["HTTP/1.1"]),
    ("JavaScript", &["fetch", "axios"]),
    ("Node.js", &["fetch", "axios"]),
    ("Python", &["requests", "http.client"]),
    ("PHP", &["cURL"]),
    ("Go", &["Native"]),
    ("Rust", &["reqwest"]),
    ("Ruby", &["net/http"]),
    ("Swift", &["URLSession"]),
    ("Java", &["HttpClient"]),
    ("C#", &["HttpClient"]),
    ("PowerShell", &["Invoke-RestMethod"]),
];

/// The request as the generators should describe it: implied Content-Type spelled out, a
/// urlencoded body flattened to text, and stale raw text dropped when the body is off.
fn prepare(r: &Request) -> Request {
    let mut p = r.clone();
    match r.body_mode {
        BodyMode::None => p.body.clear(),
        BodyMode::Raw => {}
        BodyMode::UrlEncoded => {
            p.body = r.urlencoded_body();
            p.body_mode = BodyMode::Raw;
        }
        BodyMode::FormData | BodyMode::Binary => {
            p.body.clear();
            if r.body_mode == BodyMode::FormData {
                p.headers.retain(|h| !is_content_type(h));
            }
        }
    }
    if !p.has_header("content-type")
        && let Some(content_type) = r.default_content_type()
        && (p.body_mode == BodyMode::Raw && !p.body.is_empty() || p.body_mode == BodyMode::Binary)
    {
        p.headers.push(Header::new("Content-Type", content_type));
    }
    p
}
fn comment_prefix(language: &str) -> &'static str {
    match language {
        "Shell" | "Python" | "Ruby" | "PowerShell" => "#",
        _ => "//",
    }
}
pub fn generate(r: &Request, language: usize, client: usize) -> String {
    let r = &Request {
        url: crate::with_scheme(&r.url),
        ..r.with_auth()
    };
    let (name, clients) = LANGUAGES[language.min(LANGUAGES.len() - 1)];
    let client = client.min(clients.len() - 1);
    let code = generate_prepared(r, name, clients[client]);
    let supported = match (name, clients[client]) {
        ("Shell", "curl") | ("Shell", "httpie") | ("JavaScript", _) | ("Node.js", _) => true,
        ("Python", "requests") | ("PHP", _) => true,
        ("Shell", "wget") => r.body_mode == BodyMode::Binary,
        _ => false,
    };
    if matches!(r.body_mode, BodyMode::FormData | BodyMode::Binary) && !supported {
        let what = if r.body_mode == BodyMode::FormData {
            "form-data"
        } else {
            "binary"
        };
        return format!(
            "{} {what} bodies are not generated for this client; use Shell > curl.\n{code}",
            comment_prefix(name)
        );
    }
    code
}
fn generate_prepared(original: &Request, name: &str, client: &str) -> String {
    let prepared = prepare(original);
    let r = &prepared;
    match (name, client) {
        ("Shell", "curl") => crate::curl::export(original),
        ("Shell", "httpie") => httpie(r, original),
        ("Shell", _) => wget(r, original),
        ("HTTP", _) => http(r),
        ("JavaScript", "fetch") => js_fetch(r, original, false),
        ("Node.js", "fetch") => js_fetch(r, original, true),
        ("JavaScript", _) => js_axios(r, original, "import axios from \"axios\";", false),
        ("Node.js", _) => js_axios(r, original, "const axios = require(\"axios\");", true),
        ("Python", "requests") => py_requests(r, original),
        ("Python", _) => py_http_client(r),
        ("PHP", _) => php(r, original),
        ("Go", _) => go(r),
        ("Rust", _) => rust(r),
        ("Ruby", _) => ruby(r),
        ("Swift", _) => swift(r),
        ("Java", _) => java(r),
        ("C#", _) => csharp(r),
        _ => powershell(r),
    }
}

fn headers(r: &Request) -> Vec<&Header> {
    r.headers
        .iter()
        .filter(|h| h.enabled && !h.name.trim().is_empty())
        .collect()
}
/// Double-quoted string literal shared by C-like languages.
fn dq(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn sq_posix(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
fn sq_escaped(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}
fn sq_doubled(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
fn is_content_type(h: &Header) -> bool {
    h.name.trim().eq_ignore_ascii_case("content-type")
}
fn parts(r: &Request) -> (bool, String, String, u16, String) {
    match url::Url::parse(&r.url) {
        Ok(u) => {
            let https = u.scheme() == "https";
            let mut path = u.path().to_owned();
            if let Some(q) = u.query() {
                path.push('?');
                path.push_str(q);
            }
            (
                https,
                u.host_str().unwrap_or_default().to_owned(),
                path,
                u.port_or_known_default().unwrap_or(80),
                u.as_str().to_owned(),
            )
        }
        Err(_) => (false, String::new(), r.url.clone(), 80, r.url.clone()),
    }
}

fn httpie(r: &Request, original: &Request) -> String {
    let mut s = String::from("http");
    if original.body_mode == BodyMode::FormData {
        s.push_str(" --form");
    }
    if r.follow_redirects {
        s.push_str(" --follow");
    }
    if r.insecure {
        s.push_str(" --verify=no");
    }
    s.push_str(&format!(" {} {}", r.method, sq_posix(&r.url)));
    for h in headers(r) {
        s.push_str(&format!(
            " \\\n  {}",
            sq_posix(&format!("{}:{}", h.name.trim(), h.value))
        ));
    }
    if !r.body.is_empty() {
        s.push_str(&format!(" \\\n  --raw {}", sq_posix(&r.body)));
    }
    if original.body_mode == BodyMode::FormData {
        for h in original.form_fields() {
            let item = if h.file {
                format!("{}@{}", h.name, h.value)
            } else {
                format!("{}={}", h.name, h.value)
            };
            s.push_str(&format!(" \\\n  {}", sq_posix(&item)));
        }
    }
    if original.body_mode == BodyMode::Binary {
        s.push_str(&format!(" < {}", sq_posix(&original.binary_path)));
    }
    s
}
fn wget(r: &Request, original: &Request) -> String {
    let mut s = format!("wget --quiet \\\n  --method {}", r.method);
    if r.insecure {
        s.push_str(" \\\n  --no-check-certificate");
    }
    for h in headers(r) {
        s.push_str(&format!(
            " \\\n  --header {}",
            sq_posix(&format!("{}: {}", h.name.trim(), h.value))
        ));
    }
    if !r.body.is_empty() {
        s.push_str(&format!(" \\\n  --body-data {}", sq_posix(&r.body)));
    }
    if original.body_mode == BodyMode::Binary {
        s.push_str(&format!(
            " \\\n  --body-file {}",
            sq_posix(&original.binary_path)
        ));
    }
    s.push_str(&format!(
        " \\\n  --output-document - \\\n  {}",
        sq_posix(&r.url)
    ));
    s
}
fn http(r: &Request) -> String {
    let (_, host, path, port, _) = parts(r);
    let mut s = format!("{} {} HTTP/1.1\n", r.method, path);
    let default_port = port == 80 || port == 443;
    s.push_str(&format!(
        "Host: {}\n",
        if default_port {
            host
        } else {
            format!("{host}:{port}")
        }
    ));
    for h in headers(r) {
        s.push_str(&format!("{}: {}\n", h.name.trim(), h.value));
    }
    if !r.body.is_empty() {
        s.push_str(&format!("\n{}", r.body));
    }
    s
}
/// Statements that build a multipart/binary body for JavaScript, and the expression to send.
fn js_body(original: &Request, node: bool) -> (String, Option<String>) {
    match original.body_mode {
        BodyMode::FormData => {
            let mut pre = String::from("const form = new FormData();\n");
            for h in original.form_fields() {
                if h.file && node {
                    pre.push_str(&format!(
                        "form.append({}, new Blob([fs.readFileSync({})]), {});\n",
                        dq(&h.name),
                        dq(&h.value),
                        dq(h.value.rsplit(['/', '\\']).next().unwrap_or(&h.value))
                    ));
                } else if h.file {
                    pre.push_str(&format!(
                        "form.append({}, fileInput.files[0]); // {}\n",
                        dq(&h.name),
                        h.value
                    ));
                } else {
                    pre.push_str(&format!(
                        "form.append({}, {});\n",
                        dq(&h.name),
                        dq(&h.value)
                    ));
                }
            }
            if node && original.form_fields().any(|h| h.file) {
                pre.insert_str(0, "const fs = require(\"fs\");\n");
            }
            pre.push('\n');
            (pre, Some("form".into()))
        }
        BodyMode::Binary if node => (
            "const fs = require(\"fs\");\n\n".into(),
            Some(format!("fs.readFileSync({})", dq(&original.binary_path))),
        ),
        BodyMode::Binary => (
            String::new(),
            Some(format!("file /* contents of {} */", original.binary_path)),
        ),
        _ => (String::new(), None),
    }
}
fn js_fetch(r: &Request, original: &Request, node: bool) -> String {
    let (pre, extra_body) = js_body(original, node);
    let mut s = format!(
        "{pre}const response = await fetch({}, {{\n  method: {},\n",
        dq(&r.url),
        dq(&r.method)
    );
    let hs = headers(r);
    if !hs.is_empty() {
        s.push_str("  headers: {\n");
        for h in hs {
            s.push_str(&format!("    {}: {},\n", dq(h.name.trim()), dq(&h.value)));
        }
        s.push_str("  },\n");
    }
    if !r.body.is_empty() {
        s.push_str(&format!("  body: {},\n", dq(&r.body)));
    } else if let Some(body) = extra_body {
        s.push_str(&format!("  body: {body},\n"));
    }
    s.push_str("});\nconst data = await response.text();\nconsole.log(data);");
    s
}
fn js_axios(r: &Request, original: &Request, import: &str, node: bool) -> String {
    let (pre, extra_body) = js_body(original, node);
    let mut s = format!(
        "{import}\n{}\nconst response = await axios({{\n  method: {},\n  url: {},\n",
        if pre.is_empty() {
            String::new()
        } else {
            pre.to_string()
        },
        dq(&r.method.to_lowercase()),
        dq(&r.url)
    );
    let hs = headers(r);
    if !hs.is_empty() {
        s.push_str("  headers: {\n");
        for h in hs {
            s.push_str(&format!("    {}: {},\n", dq(h.name.trim()), dq(&h.value)));
        }
        s.push_str("  },\n");
    }
    if !r.body.is_empty() {
        s.push_str(&format!("  data: {},\n", dq(&r.body)));
    } else if let Some(body) = extra_body {
        s.push_str(&format!("  data: {body},\n"));
    }
    s.push_str("});\nconsole.log(response.data);");
    s
}
fn py_requests(r: &Request, original: &Request) -> String {
    let mut s = format!(
        "import requests\n\nresponse = requests.request(\n    {},\n    {},\n",
        dq(&r.method),
        dq(&r.url)
    );
    let hs = headers(r);
    if !hs.is_empty() {
        s.push_str("    headers={\n");
        for h in hs {
            s.push_str(&format!(
                "        {}: {},\n",
                dq(h.name.trim()),
                dq(&h.value)
            ));
        }
        s.push_str("    },\n");
    }
    if !r.body.is_empty() {
        s.push_str(&format!("    data={}.encode(\"utf-8\"),\n", dq(&r.body)));
    }
    match original.body_mode {
        BodyMode::FormData => {
            let text: Vec<_> = original.form_fields().filter(|h| !h.file).collect();
            let files: Vec<_> = original.form_fields().filter(|h| h.file).collect();
            if !text.is_empty() {
                s.push_str("    data={\n");
                for h in text {
                    s.push_str(&format!("        {}: {},\n", dq(&h.name), dq(&h.value)));
                }
                s.push_str("    },\n");
            }
            if !files.is_empty() {
                s.push_str("    files={\n");
                for h in files {
                    s.push_str(&format!(
                        "        {}: open({}, \"rb\"),\n",
                        dq(&h.name),
                        dq(&h.value)
                    ));
                }
                s.push_str("    },\n");
            }
        }
        BodyMode::Binary => {
            s.push_str(&format!(
                "    data=open({}, \"rb\"),\n",
                dq(&original.binary_path)
            ));
        }
        _ => {}
    }
    s.push_str(&format!(
        "    allow_redirects={},\n",
        if r.follow_redirects { "True" } else { "False" }
    ));
    if r.insecure {
        s.push_str("    verify=False,\n");
    }
    s.push_str(&format!(
        "    timeout={},\n)\nprint(response.text)",
        r.timeout
    ));
    s
}
fn py_http_client(r: &Request) -> String {
    let (https, host, path, port, _) = parts(r);
    let class = if https {
        "HTTPSConnection"
    } else {
        "HTTPConnection"
    };
    let mut s = format!(
        "import http.client\n\nconn = http.client.{class}({}, {port}, timeout={})\n",
        dq(&host),
        r.timeout
    );
    s.push_str("headers = {\n");
    for h in headers(r) {
        s.push_str(&format!("    {}: {},\n", dq(h.name.trim()), dq(&h.value)));
    }
    s.push_str("}\n");
    let body = if r.body.is_empty() {
        "None".to_owned()
    } else {
        dq(&r.body)
    };
    s.push_str(&format!(
        "conn.request({}, {}, {body}, headers)\nres = conn.getresponse()\nprint(res.read().decode(\"utf-8\"))",
        dq(&r.method),
        dq(&path)
    ));
    s
}
fn php(r: &Request, original: &Request) -> String {
    let mut s = format!(
        "<?php\n$ch = curl_init();\ncurl_setopt_array($ch, [\n    CURLOPT_URL => {},\n    CURLOPT_CUSTOMREQUEST => {},\n    CURLOPT_RETURNTRANSFER => true,\n    CURLOPT_FOLLOWLOCATION => {},\n    CURLOPT_TIMEOUT => {},\n",
        sq_escaped(&r.url),
        sq_escaped(&r.method),
        r.follow_redirects,
        r.timeout
    );
    if r.insecure {
        s.push_str("    CURLOPT_SSL_VERIFYPEER => false,\n    CURLOPT_SSL_VERIFYHOST => 0,\n");
    }
    let hs = headers(r);
    if !hs.is_empty() {
        s.push_str("    CURLOPT_HTTPHEADER => [\n");
        for h in hs {
            s.push_str(&format!(
                "        {},\n",
                sq_escaped(&format!("{}: {}", h.name.trim(), h.value))
            ));
        }
        s.push_str("    ],\n");
    }
    if !r.body.is_empty() {
        s.push_str(&format!(
            "    CURLOPT_POSTFIELDS => {},\n",
            sq_escaped(&r.body)
        ));
    }
    match original.body_mode {
        BodyMode::FormData => {
            s.push_str("    CURLOPT_POSTFIELDS => [\n");
            for h in original.form_fields() {
                if h.file {
                    s.push_str(&format!(
                        "        {} => new CURLFile({}),\n",
                        sq_escaped(&h.name),
                        sq_escaped(&h.value)
                    ));
                } else {
                    s.push_str(&format!(
                        "        {} => {},\n",
                        sq_escaped(&h.name),
                        sq_escaped(&h.value)
                    ));
                }
            }
            s.push_str("    ],\n");
        }
        BodyMode::Binary => {
            s.push_str(&format!(
                "    CURLOPT_POSTFIELDS => file_get_contents({}),\n",
                sq_escaped(&original.binary_path)
            ));
        }
        _ => {}
    }
    s.push_str("]);\n$response = curl_exec($ch);\ncurl_close($ch);\necho $response;");
    s
}
fn go(r: &Request) -> String {
    let mut s = String::from("package main\n\nimport (\n\t\"fmt\"\n\t\"io\"\n\t\"net/http\"\n");
    if !r.body.is_empty() {
        s.push_str("\t\"strings\"\n");
    }
    s.push_str(")\n\nfunc main() {\n");
    let body = if r.body.is_empty() {
        "nil".to_owned()
    } else {
        format!("strings.NewReader({})", dq(&r.body))
    };
    s.push_str(&format!(
        "\treq, err := http.NewRequest({}, {}, {body})\n\tif err != nil {{\n\t\tpanic(err)\n\t}}\n",
        dq(&r.method),
        dq(&r.url)
    ));
    for h in headers(r) {
        s.push_str(&format!(
            "\treq.Header.Set({}, {})\n",
            dq(h.name.trim()),
            dq(&h.value)
        ));
    }
    s.push_str("\n\tres, err := http.DefaultClient.Do(req)\n\tif err != nil {\n\t\tpanic(err)\n\t}\n\tdefer res.Body.Close()\n\n\tdata, _ := io.ReadAll(res.Body)\n\tfmt.Println(string(data))\n}");
    s
}
fn rust(r: &Request) -> String {
    let method = match r.method.as_str() {
        m @ ("GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS") => {
            format!("reqwest::Method::{m}")
        }
        m => format!("reqwest::Method::from_bytes(b{}).unwrap()", dq(m)),
    };
    let mut s = String::from("let client = reqwest::Client::builder()\n");
    if r.insecure {
        s.push_str("    .danger_accept_invalid_certs(true)\n");
    }
    s.push_str(&format!(
        "    .redirect({})\n    .timeout(std::time::Duration::from_secs({}))\n    .build()?;\nlet response = client\n    .request({method}, {})\n",
        if r.follow_redirects {
            "reqwest::redirect::Policy::default()"
        } else {
            "reqwest::redirect::Policy::none()"
        },
        r.timeout,
        dq(&r.url)
    ));
    for h in headers(r) {
        s.push_str(&format!(
            "    .header({}, {})\n",
            dq(h.name.trim()),
            dq(&h.value)
        ));
    }
    if !r.body.is_empty() {
        s.push_str(&format!("    .body({})\n", dq(&r.body)));
    }
    s.push_str("    .send()\n    .await?;\nprintln!(\"{}\", response.text().await?);");
    s
}
fn ruby(r: &Request) -> String {
    let (https, _, _, _, _) = parts(r);
    let mut class = r.method.to_lowercase();
    class[..1].make_ascii_uppercase();
    let mut s = format!(
        "require \"net/http\"\nrequire \"uri\"\n\nuri = URI({})\nhttp = Net::HTTP.new(uri.host, uri.port)\nhttp.read_timeout = {}\n",
        dq(&r.url),
        r.timeout
    );
    if https {
        s.push_str("http.use_ssl = true\n");
    }
    if r.insecure {
        s.push_str("http.verify_mode = OpenSSL::SSL::VERIFY_NONE\n");
    }
    s.push_str(&format!("\nrequest = Net::HTTP::{class}.new(uri)\n"));
    for h in headers(r) {
        s.push_str(&format!(
            "request[{}] = {}\n",
            dq(h.name.trim()),
            dq(&h.value)
        ));
    }
    if !r.body.is_empty() {
        s.push_str(&format!("request.body = {}\n", dq(&r.body)));
    }
    s.push_str("\nresponse = http.request(request)\nputs response.body");
    s
}
fn swift(r: &Request) -> String {
    let mut s = format!(
        "import Foundation\n\nvar request = URLRequest(url: URL(string: {})!)\nrequest.httpMethod = {}\nrequest.timeoutInterval = {}\n",
        dq(&r.url),
        dq(&r.method),
        r.timeout
    );
    for h in headers(r) {
        s.push_str(&format!(
            "request.setValue({}, forHTTPHeaderField: {})\n",
            dq(&h.value),
            dq(h.name.trim())
        ));
    }
    if !r.body.is_empty() {
        s.push_str(&format!(
            "request.httpBody = {}.data(using: .utf8)\n",
            dq(&r.body)
        ));
    }
    s.push_str("\nlet (data, _) = try await URLSession.shared.data(for: request)\nprint(String(decoding: data, as: UTF8.self))");
    s
}
fn java(r: &Request) -> String {
    let mut s = String::from(
        "import java.net.URI;\nimport java.net.http.HttpClient;\nimport java.net.http.HttpRequest;\nimport java.net.http.HttpResponse;\n\n",
    );
    s.push_str(&format!(
        "HttpClient client = HttpClient.newBuilder()\n    .followRedirects(HttpClient.Redirect.{})\n    .build();\nHttpRequest request = HttpRequest.newBuilder()\n    .uri(URI.create({}))\n    .timeout(java.time.Duration.ofSeconds({}))\n",
        if r.follow_redirects { "NORMAL" } else { "NEVER" },
        dq(&r.url),
        r.timeout
    ));
    for h in headers(r) {
        s.push_str(&format!(
            "    .header({}, {})\n",
            dq(h.name.trim()),
            dq(&h.value)
        ));
    }
    let publisher = if r.body.is_empty() {
        "HttpRequest.BodyPublishers.noBody()".to_owned()
    } else {
        format!("HttpRequest.BodyPublishers.ofString({})", dq(&r.body))
    };
    s.push_str(&format!(
        "    .method({}, {publisher})\n    .build();\nHttpResponse<String> response = client.send(request, HttpResponse.BodyHandlers.ofString());\nSystem.out.println(response.body());",
        dq(&r.method)
    ));
    s
}
fn csharp(r: &Request) -> String {
    let mut s = format!(
        "using var client = new HttpClient();\nvar request = new HttpRequestMessage(new HttpMethod({}), {});\n",
        dq(&r.method),
        dq(&r.url)
    );
    let has_body = !r.body.is_empty();
    if has_body {
        s.push_str(&format!(
            "request.Content = new StringContent({}, System.Text.Encoding.UTF8);\n",
            dq(&r.body)
        ));
    }
    for h in headers(r) {
        if is_content_type(h) {
            if has_body {
                s.push_str(&format!(
                    "request.Content.Headers.ContentType = System.Net.Http.Headers.MediaTypeHeaderValue.Parse({});\n",
                    dq(&h.value)
                ));
            }
        } else {
            s.push_str(&format!(
                "request.Headers.TryAddWithoutValidation({}, {});\n",
                dq(h.name.trim()),
                dq(&h.value)
            ));
        }
    }
    s.push_str("\nvar response = await client.SendAsync(request);\nConsole.WriteLine(await response.Content.ReadAsStringAsync());");
    s
}
fn powershell(r: &Request) -> String {
    let mut s = String::new();
    let mut content_type = None;
    let hs: Vec<_> = headers(r)
        .into_iter()
        .filter(|h| {
            if is_content_type(h) {
                content_type = Some(h.value.clone());
                false
            } else {
                true
            }
        })
        .collect();
    if !hs.is_empty() {
        s.push_str("$headers = @{\n");
        for h in &hs {
            s.push_str(&format!(
                "    {} = {}\n",
                sq_doubled(h.name.trim()),
                sq_doubled(&h.value)
            ));
        }
        s.push_str("}\n");
    }
    if !r.body.is_empty() {
        s.push_str(&format!("$body = {}\n", sq_doubled(&r.body)));
    }
    s.push_str(&format!(
        "Invoke-RestMethod -Uri {} -Method {}",
        sq_doubled(&r.url),
        r.method
    ));
    if !hs.is_empty() {
        s.push_str(" -Headers $headers");
    }
    if let Some(ct) = content_type {
        s.push_str(&format!(" -ContentType {}", sq_doubled(&ct)));
    }
    if !r.body.is_empty() {
        s.push_str(" -Body $body");
    }
    if r.insecure {
        s.push_str(" -SkipCertificateCheck");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Request {
        Request {
            method: "POST".into(),
            url: "https://api.test:8443/v1/items?q=a b".into(),
            headers: vec![
                Header::new("Content-Type", "application/json"),
                Header::new("X-Token", "it's \"x\""),
            ],
            body: "{\"a\":\"line\\n\"}".into(),
            body_mode: BodyMode::Raw,
            ..Default::default()
        }
    }
    #[test]
    fn every_language_and_client_produces_code_with_url_and_body() {
        let r = sample();
        for (li, (name, clients)) in LANGUAGES.iter().enumerate() {
            for ci in 0..clients.len() {
                let code = generate(&r, li, ci);
                assert!(!code.is_empty(), "{name}");
                assert!(
                    code.contains("api.test") || code.contains("Host: api.test:8443"),
                    "{name} {}: {code}",
                    clients[ci]
                );
                assert!(code.contains("X-Token"), "{name} {}", clients[ci]);
            }
        }
    }
    #[test]
    fn quoting_is_language_correct() {
        let r = sample();
        assert!(generate(&r, 0, 1).contains("'X-Token:it'\\''s \"x\"'"));
        assert!(generate(&r, 5, 0).contains("'X-Token: it\\'s \"x\"'"));
        assert!(generate(&r, 12, 0).contains("'it''s \"x\"'"));
        assert!(generate(&r, 6, 0).contains("\"it's \\\"x\\\"\""));
    }
    #[test]
    fn body_modes_are_described_correctly() {
        let mut r = sample();
        r.body_mode = BodyMode::UrlEncoded;
        r.urlencoded = vec![Header::new("name", "Arif Rahman"), Header::new("age", "35")];
        r.headers.clear();
        let python = generate(&r, 4, 0);
        assert!(python.contains("name=Arif+Rahman&age=35"), "{python}");
        assert!(python.contains("application/x-www-form-urlencoded"));
        // Raw body text left over from an earlier mode is not sent.
        r.body_mode = BodyMode::None;
        for (li, (_, clients)) in LANGUAGES.iter().enumerate() {
            for ci in 0..clients.len() {
                assert!(!generate(&r, li, ci).contains("line"), "{li} {ci}");
            }
        }
        r.body_mode = BodyMode::FormData;
        r.form = vec![Header::new("note", "hi"), {
            let mut f = Header::new("doc", "/tmp/a.pdf");
            f.file = true;
            f
        }];
        let curl = generate(&r, 0, 0);
        assert!(
            curl.contains("--form-string 'note=hi'") && curl.contains("--form 'doc=@/tmp/a.pdf'")
        );
        assert!(!curl.contains("Content-Type"), "{curl}");
        let py = generate(&r, 4, 0);
        assert!(py.contains("open(\"/tmp/a.pdf\", \"rb\")") && py.contains("\"note\": \"hi\""));
        assert!(generate(&r, 6, 0).starts_with("// form-data bodies are not generated"));
        r.body_mode = BodyMode::Binary;
        r.binary_path = "/tmp/blob.bin".into();
        assert!(generate(&r, 0, 0).contains("--data-binary '@/tmp/blob.bin'"));
        assert!(generate(&r, 4, 0).contains("data=open(\"/tmp/blob.bin\", \"rb\")"));
        assert!(generate(&r, 4, 0).contains("application/octet-stream"));
    }
    #[test]
    fn raw_http_has_host_and_body() {
        let code = generate(&sample(), 1, 0);
        assert!(code.starts_with("POST /v1/items?q=a%20b HTTP/1.1\nHost: api.test:8443\n"));
        assert!(code.ends_with("{\"a\":\"line\\n\"}"));
    }
}
