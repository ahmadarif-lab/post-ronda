//! Looking for a newer release: reads a release feed (a GitHub repository's latest release or a
//! small JSON file of your own) and picks the download that fits this computer.
use serde_json::Value;

/// One downloadable file of a release.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    /// Page describing the release, when the feed has one.
    pub page: String,
    pub notes: String,
    pub assets: Vec<Asset>,
}
impl Release {
    /// The file to download on this computer, falling back to the release page.
    pub fn download_for_this_computer(&self) -> Option<String> {
        pick_asset(&self.assets, std::env::consts::OS, std::env::consts::ARCH)
            .map(|a| a.url.clone())
            .or_else(|| (!self.page.is_empty()).then(|| self.page.clone()))
    }
}
/// The feed address as typed turns into what is actually fetched: a GitHub repository (or its
/// releases page) means "the latest release of that repository"; anything else is used as is.
pub fn feed_url(input: &str) -> String {
    let input = input.trim();
    let path = input
        .strip_prefix("https://github.com/")
        .or_else(|| input.strip_prefix("http://github.com/"))
        .or_else(|| input.strip_prefix("https://www.github.com/"));
    let Some(path) = path else {
        return input.to_owned();
    };
    let mut parts = path.trim_end_matches('/').split('/');
    match (parts.next(), parts.next()) {
        (Some(owner), Some(repo)) if !owner.is_empty() && !repo.is_empty() => {
            let repo = repo.trim_end_matches(".git");
            format!("https://api.github.com/repos/{owner}/{repo}/releases/latest")
        }
        _ => input.to_owned(),
    }
}
/// Reads a GitHub release (`tag_name`, `assets`) or a plain feed
/// (`{"version": "1.2.3", "url": "…", "notes": "…", "downloads": {"file name": "url"}}`).
pub fn parse_release(json: &str) -> Result<Release, String> {
    let v: Value = serde_json::from_str(json)
        .map_err(|_| "The update address did not return JSON".to_owned())?;
    let text = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| v[*k].as_str())
            .unwrap_or("")
            .to_owned()
    };
    let version = text(&["tag_name", "version", "latest"]);
    if version.is_empty() {
        return Err("The feed has no version (expected tag_name or version)".into());
    }
    let mut assets: Vec<Asset> = v["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            Some(Asset {
                name: a["name"].as_str()?.to_owned(),
                url: a["browser_download_url"]
                    .as_str()
                    .or(a["url"].as_str())?
                    .to_owned(),
            })
        })
        .collect();
    if let Some(downloads) = v["downloads"].as_object() {
        assets.extend(downloads.iter().filter_map(|(name, url)| {
            Some(Asset {
                name: name.clone(),
                url: url.as_str()?.to_owned(),
            })
        }));
    }
    let direct = text(&["url", "download_url"]);
    if !direct.is_empty() {
        let name = direct.rsplit('/').next().unwrap_or("download").to_owned();
        assets.push(Asset { name, url: direct });
    }
    Ok(Release {
        version,
        page: text(&["html_url", "page"]),
        notes: text(&["body", "notes"]),
        assets,
    })
}
/// Where releases are looked up unless the user sets another address.
pub const DEFAULT_FEED: &str = "https://github.com/ahmadarif-lab/post-ronda";
fn numbers(version: &str) -> (Vec<u64>, bool) {
    let v = version.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = match v.split_once(['-', '+']) {
        Some((core, _)) => (core, v.contains('-')),
        None => (v, false),
    };
    (
        core.split('.')
            .map(|p| p.trim().parse().unwrap_or(0))
            .collect(),
        pre,
    )
}
/// Whether `candidate` is a later version than `current` (a pre-release is never later than the
/// same number without the suffix).
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (mut a, mut b) = (numbers(candidate).0, numbers(current).0);
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    a > b
}
/// The release file for an operating system and CPU, judged by the file names.
pub fn pick_asset<'a>(assets: &'a [Asset], os: &str, arch: &str) -> Option<&'a Asset> {
    let os_words: &[&str] = match os {
        "macos" => &["macos", "darwin", "osx"],
        "windows" => &["windows", "win64", "win32"],
        _ => &["linux"],
    };
    let arch_words: &[&str] = match arch {
        "aarch64" | "arm64" => &["aarch64", "arm64"],
        _ => &["x86_64", "x64", "amd64"],
    };
    let other_arch: &[&str] = match arch {
        "aarch64" | "arm64" => &["x86_64", "x64", "amd64"],
        _ => &["aarch64", "arm64"],
    };
    let packages = [
        ".zip",
        ".tar.gz",
        ".tgz",
        ".dmg",
        ".msi",
        ".exe",
        ".appimage",
        ".deb",
    ];
    let for_os: Vec<&Asset> = assets
        .iter()
        .filter(|a| {
            let name = a.name.to_lowercase();
            os_words.iter().any(|w| name.contains(w)) && packages.iter().any(|e| name.ends_with(e))
        })
        .collect();
    let has = |a: &Asset, words: &[&str]| words.iter().any(|w| a.name.to_lowercase().contains(w));
    for_os
        .iter()
        .find(|a| has(a, arch_words))
        .or_else(|| for_os.iter().find(|a| !has(a, other_arch)))
        .copied()
}
/// Fetches and reads the feed. Needs `curl`, like requests do.
pub fn check(url: &str) -> Result<Release, String> {
    let url = feed_url(url);
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("The update address must start with https://".into());
    }
    let out = crate::process::command(if cfg!(target_os = "macos") {
        "/usr/bin/curl"
    } else {
        "curl"
    })
    .args([
        "-q",
        "--silent",
        "--show-error",
        "--fail",
        "--location",
        "--max-time",
        "15",
        "--max-filesize",
        "2097152",
        "--proto",
        "=http,https",
        "--header",
        "Accept: application/vnd.github+json",
        "--header",
        &format!("User-Agent: Post-Ronda/{}", env!("CARGO_PKG_VERSION")),
        "--url",
        &url,
    ])
    .output()
    .map_err(|e| format!("Cannot run curl: {e}"))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        return Err(if why.contains("404") {
            "No release found at that address (a private repository, or nothing published yet)"
                .into()
        } else {
            why.trim().trim_start_matches("curl: ").to_owned()
        });
    }
    parse_release(&String::from_utf8_lossy(&out.stdout))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_compare_by_number_and_prereleases_are_not_newer() {
        assert!(is_newer("v0.3.0", "0.2.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("v0.2", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(!is_newer("0.2.0-beta.1", "0.2.0"));
        assert!(is_newer("0.3.0-beta.1", "0.2.0"));
    }
    #[test]
    fn a_feed_on_a_server_is_fetched_and_errors_are_explained() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for status in ["200 OK", "404 Not Found"] {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = [0; 2048];
                let _ = s.read(&mut buf).unwrap();
                let body = r#"{"version":"9.9.9","url":"http://x/Post-Ronda-linux-x86_64.tar.gz"}"#;
                let body = if status.starts_with("200") {
                    body
                } else {
                    "{}"
                };
                s.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
            }
        });
        let release = check(&format!("http://{addr}/feed.json")).unwrap();
        assert!(is_newer(&release.version, env!("CARGO_PKG_VERSION")));
        let err = check(&format!("http://{addr}/missing")).unwrap_err();
        assert!(err.contains("No release found"), "{err}");
        server.join().unwrap();
        assert!(
            check("ftp://example.com/x")
                .unwrap_err()
                .contains("https://")
        );
    }
    #[test]
    fn github_addresses_become_the_latest_release_api() {
        let api = "https://api.github.com/repos/me/app/releases/latest";
        assert_eq!(feed_url("https://github.com/me/app"), api);
        assert_eq!(feed_url("https://github.com/me/app/"), api);
        assert_eq!(feed_url("https://github.com/me/app.git"), api);
        assert_eq!(feed_url("https://github.com/me/app/releases"), api);
        assert_eq!(feed_url(api), api);
        assert_eq!(
            feed_url(" https://example.com/feed.json "),
            "https://example.com/feed.json"
        );
    }
    #[test]
    fn releases_are_read_from_github_or_a_plain_feed() {
        let github = r#"{"tag_name":"v0.3.0","html_url":"https://github.com/me/app/releases/v0.3.0","body":"Fixes",
          "assets":[{"name":"Post-Ronda-macos-arm64.zip","browser_download_url":"https://x/mac-arm.zip"},
                    {"name":"post-ronda-linux-x86_64.tar.gz","browser_download_url":"https://x/linux-x64.tgz"},
                    {"name":"Post-Ronda-windows-x64.zip","browser_download_url":"https://x/win.zip"},
                    {"name":"Post-Ronda-windows-x64.zip.sha256","browser_download_url":"https://x/win.sha"}]}"#;
        let r = parse_release(github).unwrap();
        assert_eq!((r.version.as_str(), r.notes.as_str()), ("v0.3.0", "Fixes"));
        assert_eq!(
            pick_asset(&r.assets, "macos", "aarch64").unwrap().url,
            "https://x/mac-arm.zip"
        );
        assert_eq!(
            pick_asset(&r.assets, "linux", "x86_64").unwrap().url,
            "https://x/linux-x64.tgz"
        );
        assert_eq!(
            pick_asset(&r.assets, "windows", "x86_64").unwrap().url,
            "https://x/win.zip"
        );
        // No Linux arm64 file: the x86_64 one must not be offered.
        assert!(pick_asset(&r.assets, "linux", "aarch64").is_none());
        let plain = r#"{"version":"1.2.3","url":"https://example.com/get/Post-Ronda-macos.zip","notes":"n"}"#;
        let r = parse_release(plain).unwrap();
        assert_eq!(r.version, "1.2.3");
        assert_eq!(
            pick_asset(&r.assets, "macos", "aarch64").unwrap().url,
            "https://example.com/get/Post-Ronda-macos.zip"
        );
        assert!(parse_release("{}").is_err());
        assert!(parse_release("<html>").is_err());
    }
}
