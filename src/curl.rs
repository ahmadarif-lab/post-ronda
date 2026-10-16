use crate::{BodyMode, Header, RawKind, Request};

/// Tokenize POSIX shell quoting without invoking a shell. Substitutions stay literal.
pub fn words(s: &str) -> Result<Vec<String>, String> {
    let mut chars = s.chars().peekable();
    let mut out = vec![];
    let mut word = String::new();
    let mut active = false;
    let mut quote = None;
    let mut ansi = false;
    let mut bytes: Vec<u8> = vec![];
    while let Some(c) = chars.next() {
        match quote {
            Some('\'') => {
                if !(ansi && c == '\\') && !bytes.is_empty() {
                    word.push_str(&String::from_utf8_lossy(&bytes));
                    bytes.clear();
                }
                if c == '\'' {
                    quote = None;
                    ansi = false;
                } else if ansi && c == '\\' {
                    let e = chars.next().ok_or("Incomplete ANSI escape")?;
                    if !matches!(e, 'x' | '0'..='7') && !bytes.is_empty() {
                        word.push_str(&String::from_utf8_lossy(&bytes));
                        bytes.clear();
                    }
                    match e {
                        'n' => word.push('\n'),
                        'r' => word.push('\r'),
                        't' => word.push('\t'),
                        'a' => word.push('\x07'),
                        'b' => word.push('\x08'),
                        'e' | 'E' => word.push('\x1b'),
                        'f' => word.push('\x0c'),
                        'v' => word.push('\x0b'),
                        '\\' | '\'' | '"' | '?' => word.push(e),
                        'x' | 'u' | 'U' => {
                            let max = match e {
                                'x' => 2,
                                'u' => 4,
                                _ => 8,
                            };
                            let mut digits = String::new();
                            while digits.len() < max
                                && chars.peek().is_some_and(|d| d.is_ascii_hexdigit())
                            {
                                digits.extend(chars.next());
                            }
                            let n = u32::from_str_radix(&digits, 16)
                                .map_err(|_| format!("Invalid \\{e} escape"))?;
                            if e == 'x' {
                                bytes.push(n as u8);
                            } else {
                                word.push(char::from_u32(n).ok_or("Invalid Unicode escape")?);
                            }
                        }
                        '0'..='7' => {
                            let mut digits = e.to_string();
                            while digits.len() < 3
                                && chars.peek().is_some_and(|d| matches!(d, '0'..='7'))
                            {
                                digits.extend(chars.next());
                            }
                            bytes.push(u32::from_str_radix(&digits, 8).unwrap_or(0) as u8);
                        }
                        _ => {
                            word.push('\\');
                            word.push(e);
                        }
                    }
                } else {
                    word.push(c);
                }
            }
            Some('"') => {
                if c == '"' {
                    quote = None;
                } else if c == '\\' {
                    let e = chars.next().ok_or("Incomplete escape")?;
                    if e == '\n' {
                    } else if matches!(e, '"' | '\\' | '$' | '`') {
                        word.push(e);
                    } else {
                        word.push('\\');
                        word.push(e);
                    }
                } else {
                    word.push(c);
                }
            }
            _ => match c {
                '$' if chars.peek() == Some(&'\'') => {
                    chars.next();
                    quote = Some('\'');
                    ansi = true;
                    active = true;
                }
                '\'' | '"' => {
                    quote = Some(c);
                    active = true;
                }
                '\\' => {
                    let e = chars.next().ok_or("Incomplete escape")?;
                    if e != '\n' {
                        word.push(e);
                        active = true;
                    }
                }
                c if c.is_whitespace() => {
                    if active {
                        out.push(std::mem::take(&mut word));
                        active = false;
                    }
                }
                ';' | '|' | '&' | '>' | '<' => {
                    return Err("Paste a single cURL command, without shell operators".into());
                }
                _ => {
                    word.push(c);
                    active = true;
                }
            },
        }
    }
    if quote.is_some() {
        return Err("Unclosed quote in cURL command".into());
    }
    if active {
        out.push(word);
    }
    Ok(out)
}

/// Browsers also copy cURL in the Windows cmd dialect: `^` escapes and `^`-terminated lines.
fn normalize(s: &str) -> String {
    let s = s.replace("\r\n", "\n");
    if !(s.contains("^\n") || s.contains("^\"")) {
        return s;
    }
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '^' {
            match chars.next() {
                Some('\n') | None => {}
                Some(n) => out.push(n),
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn parse(s: &str) -> Result<Request, String> {
    let args = words(normalize(s).trim())?;
    if !matches!(
        args.first().map(String::as_str),
        Some("curl" | "curl.exe" | "/usr/bin/curl")
    ) {
        return Err("Command must start with curl (use browser Copy as cURL / bash)".into());
    }
    let mut r = Request::default();
    let mut method = None;
    let mut head = false;
    let mut data = vec![];
    let mut fields: Vec<Header> = vec![];
    let mut json = false;
    let mut get = false;
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        let (flag, inline) = if a.starts_with("--") {
            a.split_once('=')
                .map_or((a.as_str(), None), |(a, b)| (a, Some(b)))
        } else if a.len() > 2
            && matches!(
                a.get(..2).unwrap_or(""),
                "-X" | "-H" | "-d" | "-b" | "-u" | "-A" | "-e" | "-m" | "-F"
            )
        {
            (&a[..2], Some(&a[2..]))
        } else {
            (a.as_str(), None)
        };
        let needs_value = matches!(
            flag,
            "-X" | "--request"
                | "--connect-timeout"
                | "--max-redirs"
                | "-x"
                | "--proxy"
                | "--retry"
                | "-H"
                | "--header"
                | "-d"
                | "--data"
                | "--data-raw"
                | "--data-binary"
                | "--data-ascii"
                | "--json"
                | "-b"
                | "--cookie"
                | "-u"
                | "--user"
                | "-A"
                | "--user-agent"
                | "-e"
                | "--referer"
                | "--url"
                | "-m"
                | "--max-time"
                | "--data-urlencode"
                | "-F"
                | "--form"
                | "--form-string"
        );
        let value = if needs_value {
            if let Some(v) = inline {
                v.to_owned()
            } else {
                i += 1;
                args.get(i)
                    .ok_or_else(|| format!("Missing value for {flag}"))?
                    .clone()
            }
        } else {
            String::new()
        };
        match flag {
            "-X" | "--request" => method = Some(value.to_uppercase()),
            "-H" | "--header" => {
                // curl sends `Name;` as a header with an empty value.
                let (k, v) = match value.split_once(':') {
                    Some(kv) => kv,
                    None => (
                        value
                            .strip_suffix(';')
                            .ok_or("Header must have Name: value")?,
                        "",
                    ),
                };
                if k.trim().is_empty() {
                    // HTTP/2 pseudo-headers (":authority: …") arrive with an empty name; skip them.
                    if value.starts_with(':') {
                        i += 1;
                        continue;
                    }
                    return Err("Header name is empty".into());
                }
                r.headers.push(Header::new(k.trim(), v.trim()));
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii" | "--json" => {
                if value.starts_with('@') && flag != "--data-raw" {
                    return Err(
                        "File uploads via @file are not supported; paste the body directly".into(),
                    );
                }
                json |= flag == "--json";
                data.push(value);
            }
            "-F" | "--form" | "--form-string" => {
                let (name, content) = value
                    .split_once('=')
                    .ok_or("Form field must be name=value")?;
                if flag != "--form-string" && content.starts_with(['@', '<']) {
                    return Err(
                        "File uploads via -F are not supported; add files in the Body tab".into(),
                    );
                }
                fields.push(Header::new(name, content));
            }
            "--data-urlencode" => {
                if value.contains('@') && !value.contains('=') {
                    return Err("File input is not supported".into());
                }
                let (k, v) = value.split_once('=').unwrap_or(("", &value));
                let encoded: String = url::form_urlencoded::byte_serialize(v.as_bytes()).collect();
                data.push(if k.is_empty() {
                    encoded
                } else {
                    format!("{k}={encoded}")
                });
            }
            "-b" | "--cookie" => {
                if !value.contains('=') {
                    return Err("Cookie file input is unsupported; paste a Cookie header".into());
                }
                r.headers.push(Header::new("Cookie", &value));
            }
            "-u" | "--user" => r.headers.push(Header::new(
                "Authorization",
                &format!("Basic {}", base64(value.as_bytes())),
            )),
            "-A" | "--user-agent" => r.headers.push(Header::new("User-Agent", &value)),
            "-e" | "--referer" => r.headers.push(Header::new("Referer", &value)),
            "--url" => {
                if !r.url.is_empty() {
                    return Err("Only one URL per request is supported".into());
                }
                r.url = value;
            }
            "-m" | "--max-time" => {
                r.timeout = value
                    .parse::<u32>()
                    .map_err(|_| "Timeout must be an integer in seconds")?
                    .clamp(1, 300)
            }
            "-L" | "--location" => r.follow_redirects = true,
            "-k" | "--insecure" => r.insecure = true,
            "-I" | "--head" => head = true,
            "-G" | "--get" => get = true,
            "--compressed" | "--globoff" | "-g" | "--silent" | "-s" | "--show-error" | "-S"
            | "-sS" | "-v" | "--verbose" | "-i" | "--include" | "-f" | "--fail" | "--http1.1"
            | "--http2" | "--http3" | "-4" | "-6" | "--no-keepalive" | "--tr-encoding"
            | "--path-as-is" | "-N" | "--no-buffer" | "--connect-timeout" | "--max-redirs"
            | "-x" | "--proxy" | "--retry" => {}
            _ if a.starts_with('-') => {
                return Err(format!(
                    "Unsupported cURL option: {a}. Nothing was imported."
                ));
            }
            _ => {
                if !r.url.is_empty() {
                    return Err("Only one URL per request is supported".into());
                }
                r.url = a.clone();
            }
        }
        i += 1;
    }
    r.url = crate::with_scheme(&r.url);
    let mut url = url::Url::parse(&r.url).map_err(|_| "Enter a valid absolute HTTP(S) URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only HTTP and HTTPS are supported".into());
    }
    let joined = data.join("&");
    if get && !joined.is_empty() {
        let q = url
            .query()
            .map(|q| format!("{q}&{joined}"))
            .unwrap_or(joined.clone());
        url.set_query(Some(&q));
        r.url = url.to_string();
        data.clear();
    }
    r.method = method.unwrap_or_else(|| {
        if head {
            "HEAD"
        } else if get || (data.is_empty() && fields.is_empty()) {
            "GET"
        } else {
            "POST"
        }
        .into()
    });
    let content_type = |r: &Request| {
        r.headers
            .iter()
            .position(|h| h.name.eq_ignore_ascii_case("content-type"))
    };
    if !fields.is_empty() {
        // curl writes its own multipart boundary, so a pasted multipart header is redundant.
        if let Some(i) = content_type(&r)
            && r.headers[i]
                .value
                .to_ascii_lowercase()
                .starts_with("multipart/")
        {
            r.headers.remove(i);
        }
        r.body_mode = BodyMode::FormData;
        r.form = fields;
    } else if let Some(parts) = content_type(&r)
        .map(|i| r.headers[i].value.clone())
        .filter(|ct| ct.to_ascii_lowercase().starts_with("multipart/form-data"))
        .and_then(|ct| multipart_boundary(&ct))
        .filter(|_| !data.is_empty())
        .and_then(|boundary| parse_multipart(&joined, &boundary))
    {
        // A browser's "Copy as cURL" for an upload carries the multipart body as raw text.
        // Turn it back into form fields; curl writes a fresh boundary when sending.
        if let Some(i) = content_type(&r) {
            r.headers.remove(i);
        }
        r.body_mode = BodyMode::FormData;
        r.form = parts;
    } else if !data.is_empty() {
        let body = joined;
        let sniffed_json = body.trim_start().starts_with(['{', '[']);
        let ct = content_type(&r).map(|i| r.headers[i].value.to_ascii_lowercase());
        let urlencoded = ct
            .as_deref()
            .is_none_or(|c| c.contains("x-www-form-urlencoded"));
        if urlencoded && !sniffed_json && !json && body.contains('=') {
            r.body_mode = BodyMode::UrlEncoded;
            r.urlencoded = url::form_urlencoded::parse(body.as_bytes())
                .map(|(k, v)| Header::new(&k, &v))
                .collect();
            if let Some(i) = content_type(&r) {
                r.headers.remove(i);
            }
        } else {
            r.body_mode = BodyMode::Raw;
            r.raw_kind = match ct.as_deref() {
                Some(c) if !c.contains("x-www-form-urlencoded") => RawKind::from_content_type(c),
                _ if json || sniffed_json => RawKind::Json,
                _ => RawKind::Text,
            };
            r.body = body;
            if content_type(&r).is_none() {
                r.headers.push(Header::new(
                    "Content-Type",
                    if json {
                        "application/json"
                    } else {
                        "application/x-www-form-urlencoded"
                    },
                ));
            }
        }
    }
    if json
        && !r
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("accept"))
    {
        r.headers.push(Header::new("Accept", "application/json"));
    }
    r.name = url.path().trim_start_matches('/').to_owned();
    if r.name.is_empty() {
        r.name = url.host_str().unwrap_or("Request").into();
    }
    Ok(r)
}
fn multipart_boundary(content_type: &str) -> Option<String> {
    let lower = content_type.to_ascii_lowercase();
    let at = lower.find("boundary=")? + "boundary=".len();
    let value = content_type[at..]
        .split(';')
        .next()?
        .trim()
        .trim_matches('"');
    (!value.is_empty()).then(|| value.to_owned())
}
/// Value of `key="…"` inside a Content-Disposition line (not matching `filename=` for `name=`).
fn disposition_param(head: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let mut from = 0;
    while let Some(i) = head[from..].find(&needle) {
        let start = from + i;
        let boundary_ok = head[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        if boundary_ok {
            let rest = &head[start + needle.len()..];
            return Some(rest[..rest.find('"')?].to_owned());
        }
        from = start + needle.len();
    }
    None
}
/// Splits a multipart/form-data body into rows. Parts with a `filename` become File rows that
/// remember the file name (the bytes themselves are not recoverable from pasted text).
fn parse_multipart(body: &str, boundary: &str) -> Option<Vec<Header>> {
    let delimiter = format!("--{boundary}");
    let mut fields = Vec::new();
    for part in body.split(&delimiter).skip(1) {
        let part = part
            .strip_prefix("\r\n")
            .or_else(|| part.strip_prefix('\n'))
            .unwrap_or(part);
        if part.starts_with("--") {
            break;
        }
        let (head, content) = part
            .split_once("\r\n\r\n")
            .or_else(|| part.split_once("\n\n"))?;
        let content = content
            .strip_suffix("\r\n")
            .or_else(|| content.strip_suffix('\n'))
            .unwrap_or(content);
        let disposition = head
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("content-disposition:"))?;
        let Some(name) = disposition_param(disposition, "name") else {
            continue;
        };
        match disposition_param(disposition, "filename") {
            Some(file) => {
                let mut row = Header::new(&name, &file);
                row.file = true;
                fields.push(row);
            }
            None => fields.push(Header::new(&name, content)),
        }
    }
    (!fields.is_empty()).then_some(fields)
}
fn base64(bytes: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((c.get(1).copied().unwrap_or(0) as u32) << 8)
            | c.get(2).copied().unwrap_or(0) as u32;
        out.push(alphabet[((n >> 18) & 63) as usize] as char);
        out.push(alphabet[((n >> 12) & 63) as usize] as char);
        out.push(if c.len() > 1 {
            alphabet[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            alphabet[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
pub fn export(r: &Request) -> String {
    let r = &r.with_auth();
    let mut s = format!("curl -X {} {}", quote(&r.method), quote(&r.url));
    for h in r.headers.iter().filter(|h| {
        h.enabled
            && !h.name.is_empty()
            && !(r.body_mode == BodyMode::FormData && h.name.eq_ignore_ascii_case("content-type"))
    }) {
        s.push_str(&format!(
            " \\\n  -H {}",
            quote(&format!("{}: {}", h.name, h.value))
        ));
    }
    match r.body_mode {
        BodyMode::Raw if !r.body.is_empty() => {
            if !r.has_header("content-type") {
                s.push_str(&format!(
                    " \\\n  -H {}",
                    quote(&format!("Content-Type: {}", r.raw_kind.content_type()))
                ));
            }
            s.push_str(&format!(" \\\n  --data-raw {}", quote(&r.body)));
        }
        BodyMode::UrlEncoded => {
            for h in r
                .urlencoded
                .iter()
                .filter(|h| h.enabled && !h.name.trim().is_empty())
            {
                s.push_str(&format!(
                    " \\\n  --data-urlencode {}",
                    quote(&format!("{}={}", h.name, h.value))
                ));
            }
        }
        BodyMode::FormData => {
            for h in r.form_fields() {
                s.push_str(&format!(
                    " \\\n  {} {}",
                    if h.file { "--form" } else { "--form-string" },
                    quote(&format!(
                        "{}={}{}",
                        h.name,
                        if h.file { "@" } else { "" },
                        h.value
                    ))
                ));
            }
        }
        BodyMode::Binary if !r.binary_path.is_empty() => {
            if !r.has_header("content-type") {
                s.push_str(" \\\n  -H 'Content-Type: application/octet-stream'");
            }
            s.push_str(&format!(
                " \\\n  --data-binary {}",
                quote(&format!("@{}", r.binary_path))
            ));
        }
        _ => {}
    }
    if r.follow_redirects {
        s.push_str(" -L");
    }
    if r.insecure {
        s.push_str(" -k");
    }
    s.push_str(&format!(" --max-time {}", r.timeout));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_curl() {
        let r = parse("curl 'https://example.com/api?q=1' \\\n -H 'accept: application/json' --data-raw '{\"name\":\"ronda\"}' --compressed").unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.body, "{\"name\":\"ronda\"}");
        assert_eq!(r.headers[0].name, "accept");
        let again = parse(&export(&r)).unwrap();
        assert_eq!(r, again);
    }
    #[test]
    fn ansi_and_quotes() {
        assert_eq!(
            words("curl $'a\\nb' 'it'\\''s'").unwrap(),
            vec!["curl", "a\nb", "it's"]
        );
    }
    #[test]
    fn chrome_bash_unicode_and_extra_flags() {
        let r = parse("curl 'https://a.test/x' \\\r\n  -H ':authority: a.test' \\\r\n  -H 'accept: */*' \\\r\n  --http2 --connect-timeout 5 \\\r\n  --data-raw $'{\"n\":\"caf\\u00e9 \\xe2\\x9c\\x93\"}'").unwrap();
        assert_eq!(r.body, "{\"n\":\"café ✓\"}");
        assert_eq!(r.headers.len(), 2);
        assert_eq!(r.method, "POST");
    }
    #[test]
    fn empty_value_headers_and_long_chrome_request() {
        let r = parse(
            r#"curl --url 'https://dev.greatdayhr.co/_/v2/activity/createActivity?' \
  -H 'sec-ch-ua: "Google Chrome";v="155", "Chromium";v="155"' \
  -H 'ngsw-bypass;' \
  -H 'Content-Type: application/json; charset=UTF-8' \
  -H 'Key;' \
  --data-raw '{"location":"{\"latitude\":-6.33}","attOn":"online"}'"#,
        )
        .unwrap();
        assert_eq!(r.method, "POST");
        let bypass = r.headers.iter().find(|h| h.name == "ngsw-bypass").unwrap();
        assert_eq!(bypass.value, "");
        assert!(r.headers.iter().any(|h| h.name == "Key"));
        assert_eq!(
            r.headers
                .iter()
                .find(|h| h.name == "sec-ch-ua")
                .unwrap()
                .value,
            "\"Google Chrome\";v=\"155\", \"Chromium\";v=\"155\""
        );
        assert!(r.body.contains(r#"\"latitude\""#));
    }
    #[test]
    fn chrome_windows_cmd() {
        let r = parse("curl \"https://a.test/x\" ^\r\n  -H ^\"accept: */*^\" ^\r\n  --data-raw ^\"{^\\^\"a^\\^\":1}^\"").unwrap();
        assert_eq!(r.url, "https://a.test/x");
        assert_eq!(r.body, "{\"a\":1}");
    }
    #[test]
    fn body_modes_are_detected_from_a_pasted_command() {
        let r = parse("curl https://a.test -d 'name=Arif&age=35' -d 'x=a b'").unwrap();
        assert_eq!(r.body_mode, BodyMode::UrlEncoded);
        assert_eq!(r.urlencoded.len(), 3);
        assert!(!r.has_header("content-type"));
        let r = parse("curl https://a.test -H 'Content-Type: application/xml' --data-raw '<a/>'")
            .unwrap();
        assert_eq!((r.body_mode, r.raw_kind), (BodyMode::Raw, RawKind::Xml));
        let r = parse("curl https://a.test --json '{\"a\":1}'").unwrap();
        assert_eq!((r.body_mode, r.raw_kind), (BodyMode::Raw, RawKind::Json));
        let r = parse("curl https://a.test -F 'a=1' --form-string 'b=@x'").unwrap();
        assert_eq!(r.body_mode, BodyMode::FormData);
        assert_eq!(r.form[1].value, "@x");
        assert_eq!(r.method, "POST");
        assert!(parse("curl https://a.test -F 'f=@/etc/passwd'").is_err());
        assert!(parse("curl https://a.test --data-binary @/etc/passwd").is_err());
        // exporting then importing keeps urlencoded and multipart bodies
        for src in [
            "curl https://a.test -d 'k=v&n=1'",
            "curl https://a.test -F a=1 -F b=2",
        ] {
            let r = parse(src).unwrap();
            assert_eq!(parse(&export(&r)).unwrap(), r, "{src}");
        }
    }
    #[test]
    fn browser_multipart_upload_becomes_form_data() {
        let r = parse(concat!(
            "curl --url 'https://a.test/upload?token=1' \\\n",
            "  -H 'Accept: application/json' \\\n",
            "  -H 'Content-Type: multipart/form-data; boundary=----WebKitFormBoundaryX' \\\n",
            "  --data-raw $'------WebKitFormBoundaryX\\r\\nContent-Disposition: form-data; name=\"file\"; ",
            "filename=\"img_1.jpg\"\\r\\nContent-Type: image/png\\r\\n\\r\\n\\xef\\xbf\\xbdPNG\\r\\n\\u001a\\n\\u0000IEND\\r\\n",
            "------WebKitFormBoundaryX\\r\\nContent-Disposition: form-data; name=\"note\"\\r\\n\\r\\nhello world\\r\\n",
            "------WebKitFormBoundaryX--\\r\\n'"
        ))
        .unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.body_mode, BodyMode::FormData);
        assert_eq!(r.form.len(), 2);
        assert!(r.form[0].file && r.form[0].name == "file" && r.form[0].value == "img_1.jpg");
        assert!(!r.form[1].file && r.form[1].name == "note" && r.form[1].value == "hello world");
        // curl supplies its own boundary, so the pasted multipart header is dropped
        assert!(!r.has_header("content-type") && r.has_header("accept"));
        assert!(r.body.is_empty());
    }
    #[test]
    fn rejects_shell_and_files() {
        for s in [
            "curl https://a.test ; touch /tmp/x",
            "curl https://a.test --data @secret",
            "curl https://a.test -o file",
            "curl 'oops",
        ] {
            assert!(parse(s).is_err(), "{s}");
        }
    }
    #[test]
    fn basic_and_get() {
        let r = parse("curl https://a.test -u user:pass -G --data-urlencode 'q=a b'").unwrap();
        assert_eq!(r.method, "GET");
        assert!(r.url.ends_with("?q=a+b"));
        assert_eq!(r.headers[0].value, "Basic dXNlcjpwYXNz");
    }
    #[test]
    fn literal_shell_substitution() {
        let r = parse("curl https://a.test --data-raw '$(whoami)'").unwrap();
        assert_eq!(r.body, "$(whoami)");
    }
}
