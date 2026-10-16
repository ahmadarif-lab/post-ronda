//! Looking for a newer release: reads a release feed (a GitHub repository's latest release or a
//! small JSON file of your own) and picks the download that fits this computer.
use serde_json::Value;

/// One downloadable file of a release.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    /// Size in bytes when the feed says (0 when unknown), for the progress display.
    pub size: u64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    /// Page describing the release, when the feed has one.
    pub page: String,
    pub notes: String,
    pub assets: Vec<Asset>,
    /// SHA-256 of files, by file name, when the feed lists them.
    pub checksums: std::collections::BTreeMap<String, String>,
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
                size: a["size"].as_u64().unwrap_or(0),
            })
        })
        .collect();
    if let Some(downloads) = v["downloads"].as_object() {
        assets.extend(downloads.iter().filter_map(|(name, url)| {
            Some(Asset {
                name: name.clone(),
                url: url.as_str()?.to_owned(),
                size: 0,
            })
        }));
    }
    let mut checksums = std::collections::BTreeMap::new();
    let direct = text(&["url", "download_url"]);
    if !direct.is_empty() {
        let name = direct.rsplit('/').next().unwrap_or("download").to_owned();
        assets.push(Asset {
            name: name.clone(),
            url: direct,
            size: 0,
        });
        if let Some(hash) = v["sha256"].as_str() {
            checksums.insert(name, hash.trim().to_lowercase());
        }
    }
    if let Some(map) = v["checksums"].as_object() {
        for (name, hash) in map {
            if let Some(hash) = hash.as_str() {
                checksums.insert(name.clone(), hash.trim().to_lowercase());
            }
        }
    }
    Ok(Release {
        version,
        page: text(&["html_url", "page"]),
        notes: text(&["body", "notes"]),
        assets,
        checksums,
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
/// The release file to hand to a person for an operating system and CPU: an installer when
/// there is one, otherwise a plain archive.
pub fn pick_asset<'a>(assets: &'a [Asset], os: &str, arch: &str) -> Option<&'a Asset> {
    pick(
        assets,
        os,
        arch,
        &[
            ".pkg",
            ".msi",
            ".exe",
            ".dmg",
            ".appimage",
            ".deb",
            ".zip",
            ".tar.gz",
            ".tgz",
        ],
    )
}
/// The release file the app can unpack and install over itself: an archive, never an installer.
pub fn pick_update_asset<'a>(assets: &'a [Asset], os: &str, arch: &str) -> Option<&'a Asset> {
    pick(assets, os, arch, &[".zip", ".tar.gz", ".tgz"])
}
/// Judges by file name: the operating system and CPU words, and the first of `kinds` (in order
/// of preference) that matches.
fn pick<'a>(assets: &'a [Asset], os: &str, arch: &str, kinds: &[&str]) -> Option<&'a Asset> {
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
    let has = |a: &Asset, words: &[&str]| words.iter().any(|w| a.name.to_lowercase().contains(w));
    for kind in kinds {
        let for_os: Vec<&Asset> = assets
            .iter()
            .filter(|a| {
                let name = a.name.to_lowercase();
                // A Windows installer is recognised by its extension, whatever it is called.
                let by_extension =
                    os == "windows" && (name.ends_with(".exe") || name.ends_with(".msi"));
                (has(a, os_words) || by_extension) && name.ends_with(kind)
            })
            .collect();
        let found = for_os
            .iter()
            .find(|a| has(a, arch_words))
            .or_else(|| for_os.iter().find(|a| !has(a, other_arch)))
            .copied();
        if found.is_some() {
            return found;
        }
    }
    None
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

// ---- Installing a release ------------------------------------------------------------------

/// SHA-256, for checking that a download is the file that was published.
pub struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    filled: usize,
    total: u64,
}
impl Default for Sha256 {
    fn default() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            block: [0; 64],
            filled: 0,
            total: 0,
        }
    }
}
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];
impl Sha256 {
    fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes([
                block[4 * i],
                block[4 * i + 1],
                block[4 * i + 2],
                block[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }
    pub fn update(&mut self, mut data: &[u8]) {
        self.total += data.len() as u64;
        while !data.is_empty() {
            let take = (64 - self.filled).min(data.len());
            self.block[self.filled..self.filled + take].copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled == 64 {
                let block = self.block;
                Self::compress(&mut self.state, &block);
                self.filled = 0;
            }
        }
    }
    pub fn finish(mut self) -> String {
        let bits = self.total * 8;
        self.update(&[0x80]);
        while self.filled != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        self.state.iter().map(|w| format!("{w:08x}")).collect()
    }
}
/// SHA-256 of a file, as lowercase hex.
pub fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::default();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(hasher.finish());
        }
        hasher.update(&buffer[..n]);
    }
}
fn curl() -> std::process::Command {
    crate::process::command(if cfg!(target_os = "macos") {
        "/usr/bin/curl"
    } else {
        "curl"
    })
}
/// Downloads `url` to `dest`, calling `progress` with the bytes received so far.
fn download(url: &str, dest: &std::path::Path, progress: &dyn Fn(u64)) -> Result<(), String> {
    // Downloads must be encrypted; plain http is accepted only for a server on this computer.
    let local = url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost");
    if !url.starts_with("https://") && !local {
        return Err("The download address must start with https://".into());
    }
    let mut child = curl()
        .args([
            "-q",
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--max-time",
            "900",
            "--max-filesize",
            "314572800",
            "--proto",
            "=https,http",
            "--proto-redir",
            "=https",
            "--header",
            &format!("User-Agent: Post-Ronda/{}", env!("CARGO_PKG_VERSION")),
            "--output",
        ])
        .arg(dest)
        .arg("--url")
        .arg(url)
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Cannot run curl: {e}"))?;
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => {
                if status.success() {
                    return Ok(());
                }
                let mut why = String::new();
                if let Some(mut err) = child.stderr.take() {
                    use std::io::Read;
                    let _ = err.read_to_string(&mut why);
                }
                return Err(format!(
                    "Download failed: {}",
                    why.trim().trim_start_matches("curl: ")
                ));
            }
            None => {
                progress(std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0));
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    }
}
/// The published SHA-256 of an asset: listed in the feed, or in a `<name>.sha256` file next to it.
fn published_checksum(release: &Release, asset: &Asset) -> Option<String> {
    if let Some(hash) = release.checksums.get(&asset.name) {
        return Some(hash.clone());
    }
    let sidecar = release
        .assets
        .iter()
        .find(|a| a.name == format!("{}.sha256", asset.name))?;
    let out = curl()
        .args([
            "-q",
            "--silent",
            "--fail",
            "--location",
            "--max-time",
            "20",
            "--max-filesize",
            "4096",
            "--url",
            &sidecar.url,
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let token = text.split_whitespace().next()?.to_lowercase();
    (out.status.success() && token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit()))
        .then_some(token)
}
/// What to start once the app has quit, to come back as the new version.
#[derive(Clone, Debug, PartialEq)]
pub struct Relaunch {
    pub program: String,
    pub args: Vec<String>,
}
fn extract(package: &std::path::Path, into: &std::path::Path) -> Result<(), String> {
    let name = package.to_string_lossy().to_lowercase();
    let mut command = if name.ends_with(".zip") {
        if cfg!(target_os = "macos") {
            let mut c = crate::process::command("ditto");
            c.args(["-x", "-k"]).arg(package).arg(into);
            c
        } else if cfg!(target_os = "windows") {
            let mut c = crate::process::command("tar");
            c.arg("-xf").arg(package).arg("-C").arg(into);
            c
        } else {
            let mut c = crate::process::command("unzip");
            c.arg("-q").arg(package).arg("-d").arg(into);
            c
        }
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let mut c = crate::process::command("tar");
        c.arg("-xzf").arg(package).arg("-C").arg(into);
        c
    } else {
        return Err("This download is not a package the app can install by itself".into());
    };
    let out = command
        .output()
        .map_err(|e| format!("Cannot unpack the download: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Cannot unpack the download: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}
/// The first file or folder under `dir` (a few levels deep) for which `wanted` is true.
fn find_in(
    dir: &std::path::Path,
    depth: usize,
    wanted: &dyn Fn(&std::path::Path) -> bool,
) -> Option<std::path::PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if wanted(&path) {
            return Some(path);
        }
        if depth > 0
            && path.is_dir()
            && !path.extension().is_some_and(|e| e == "app")
            && let Some(found) = find_in(&path, depth - 1, wanted)
        {
            return Some(found);
        }
    }
    None
}
/// Puts the unpacked release in place of the running app and says how to start it again.
/// `unpacked` is the folder the package was extracted to; `current` is the running executable.
pub fn swap_in(unpacked: &std::path::Path, current: &std::path::Path) -> Result<Relaunch, String> {
    let shown = current.display().to_string();
    if shown.contains("/target/") || shown.contains("\\target\\") {
        return Err("This is a development build. Updating works for installed copies.".into());
    }
    let denied = |e: std::io::Error| {
        format!(
            "Cannot replace the app ({e}). Move it to a folder you can write to, or download the update by hand."
        )
    };
    if cfg!(target_os = "macos") {
        // .../Post Ronda.app/Contents/MacOS/PostRonda
        let bundle = current
            .ancestors()
            .nth(3)
            .filter(|p| p.extension().is_some_and(|e| e == "app"))
            .ok_or("The app is not running from an application bundle")?;
        let new_app = find_in(unpacked, 2, &|p| p.extension().is_some_and(|e| e == "app"))
            .ok_or("The download does not contain the app")?;
        let backup = bundle.with_extension("app.previous");
        let _ = std::fs::remove_dir_all(&backup);
        std::fs::rename(bundle, &backup).map_err(denied)?;
        let copied = crate::process::command("ditto")
            .arg(&new_app)
            .arg(bundle)
            .output();
        if !copied.as_ref().is_ok_and(|o| o.status.success()) {
            let _ = std::fs::remove_dir_all(bundle);
            let _ = std::fs::rename(&backup, bundle);
            return Err("Cannot copy the new version into place; the old one was kept".into());
        }
        let _ = std::fs::remove_dir_all(&backup);
        Ok(Relaunch {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "sleep 1; open -n \"$0\"".into(),
                bundle.display().to_string(),
            ],
        })
    } else if cfg!(target_os = "windows") {
        let new_exe = find_in(unpacked, 3, &|p| {
            p.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("PostRonda.exe"))
        })
        .ok_or("The download does not contain PostRonda.exe")?;
        // A running program cannot be overwritten on Windows, but it can be renamed.
        let old = current.with_extension("exe.old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(current, &old).map_err(denied)?;
        if let Err(e) = std::fs::copy(&new_exe, current) {
            let _ = std::fs::remove_file(current);
            let _ = std::fs::rename(&old, current);
            return Err(denied(e));
        }
        Ok(Relaunch {
            program: "cmd".into(),
            args: vec![
                "/C".into(),
                format!(
                    "ping -n 2 127.0.0.1 >nul & start \"\" \"{}\"",
                    current.display()
                ),
            ],
        })
    } else {
        let new_exe = find_in(unpacked, 3, &|p| {
            p.is_file() && p.file_name().is_some_and(|n| n == "post-ronda")
        })
        .ok_or("The download does not contain the program")?;
        let staged = current.with_extension("new");
        std::fs::copy(&new_exe, &staged).map_err(denied)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
        }
        // Renaming over a running program is allowed on Linux.
        std::fs::rename(&staged, current).map_err(|e| {
            let _ = std::fs::remove_file(&staged);
            denied(e)
        })?;
        Ok(Relaunch {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "sleep 1; exec \"$0\"".into(),
                current.display().to_string(),
            ],
        })
    }
}
/// Downloads the release for this computer, checks it and replaces the running app with it.
/// `status` receives short messages for the settings page.
pub fn install(release: &Release, status: &dyn Fn(String)) -> Result<Relaunch, String> {
    let asset = pick_update_asset(
        &release.assets,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
    .ok_or("This release has no download for this computer")?;
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let work = tempfile::Builder::new()
        .prefix("post-ronda-update")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let package = work.path().join(&asset.name);
    let megabytes = |bytes: u64| format!("{:.1}", bytes as f64 / 1_048_576.0);
    status("Downloading…".into());
    download(&asset.url, &package, &|done| {
        status(if asset.size > 0 {
            format!(
                "Downloading… {} of {} MB",
                megabytes(done),
                megabytes(asset.size)
            )
        } else {
            format!("Downloading… {} MB", megabytes(done))
        });
    })?;
    status("Checking the download…".into());
    if let Some(expected) = published_checksum(release, asset)
        && sha256_file(&package)? != expected
    {
        return Err(
            "The download does not match its published checksum, so it was not installed".into(),
        );
    }
    status("Installing…".into());
    let unpacked = work.path().join("unpacked");
    std::fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
    extract(&package, &unpacked)?;
    swap_in(&unpacked, &current)
}
/// Removes what the last update left behind (the renamed old program on Windows).
pub fn clean_up_after_update() {
    if cfg!(target_os = "windows")
        && let Ok(current) = std::env::current_exe()
    {
        let _ = std::fs::remove_file(current.with_extension("exe.old"));
    }
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
                    {"name":"Post-Ronda-windows-x64.zip.sha256","browser_download_url":"https://x/win.sha"},
                    {"name":"Post-Ronda-Setup-x64.exe","browser_download_url":"https://x/win-setup.exe"},
                    {"name":"Post-Ronda-macos-arm64.pkg","browser_download_url":"https://x/mac.pkg"}]}"#;
        let r = parse_release(github).unwrap();
        assert_eq!((r.version.as_str(), r.notes.as_str()), ("v0.3.0", "Fixes"));
        // A person is offered the installer; the app updates itself from the archive.
        let url = |a: Option<&Asset>| a.unwrap().url.clone();
        assert_eq!(
            url(pick_asset(&r.assets, "macos", "aarch64")),
            "https://x/mac.pkg"
        );
        assert_eq!(
            url(pick_update_asset(&r.assets, "macos", "aarch64")),
            "https://x/mac-arm.zip"
        );
        assert_eq!(
            url(pick_asset(&r.assets, "windows", "x86_64")),
            "https://x/win-setup.exe"
        );
        assert_eq!(
            url(pick_update_asset(&r.assets, "windows", "x86_64")),
            "https://x/win.zip"
        );
        assert_eq!(
            url(pick_asset(&r.assets, "linux", "x86_64")),
            "https://x/linux-x64.tgz"
        );
        assert_eq!(
            url(pick_update_asset(&r.assets, "linux", "x86_64")),
            "https://x/linux-x64.tgz"
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

    #[test]
    fn sha256_matches_known_values() {
        let hash = |s: &str| {
            let mut h = Sha256::default();
            h.update(s.as_bytes());
            h.finish()
        };
        assert_eq!(
            hash(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = "a".repeat(1000);
        let mut split = Sha256::default();
        split.update(&long.as_bytes()[..333]);
        split.update(&long.as_bytes()[333..]);
        assert_eq!(split.finish(), hash(&long));
    }
    #[test]
    fn a_download_is_fetched_and_checked_and_swapped_in() {
        use std::io::{Read, Write};
        let payload = b"new version";
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0; 2048];
            let _ = s.read(&mut buf).unwrap();
            s.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                )
                .as_bytes(),
            )
            .unwrap();
            s.write_all(payload).unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pkg");
        download(&format!("http://{addr}/pkg"), &file, &|_| {}).unwrap();
        server.join().unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), payload);
        assert_eq!(sha256_file(&file).unwrap().len(), 64);
        assert!(
            download("http://example.com/x", &file, &|_| {})
                .unwrap_err()
                .contains("https")
        );
    }
    #[test]
    fn development_builds_are_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("target").join("debug").join("post-ronda");
        assert!(
            swap_in(dir.path(), &current)
                .unwrap_err()
                .contains("development build")
        );
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn the_app_bundle_is_replaced_and_relaunched() {
        let dir = tempfile::tempdir().unwrap();
        let make = |root: &std::path::Path, content: &str| {
            let exe = root.join("Post Ronda.app/Contents/MacOS/PostRonda");
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, content).unwrap();
            exe
        };
        let current = make(&dir.path().join("Applications"), "old");
        let unpacked = dir.path().join("unpacked");
        make(&unpacked, "new");
        let relaunch = swap_in(&unpacked, &current).unwrap();
        assert_eq!(std::fs::read_to_string(&current).unwrap(), "new");
        assert!(
            !dir.path()
                .join("Applications/Post Ronda.app.previous")
                .exists()
        );
        assert_eq!(relaunch.program, "/bin/sh");
        assert!(relaunch.args[2].ends_with("Post Ronda.app"));
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn the_program_is_replaced_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("bin").join("post-ronda");
        std::fs::create_dir_all(current.parent().unwrap()).unwrap();
        std::fs::write(&current, "old").unwrap();
        let unpacked = dir.path().join("unpacked/post-ronda-linux-x86_64/bin");
        std::fs::create_dir_all(&unpacked).unwrap();
        std::fs::write(unpacked.join("post-ronda"), "new").unwrap();
        swap_in(&dir.path().join("unpacked"), &current).unwrap();
        assert_eq!(std::fs::read_to_string(&current).unwrap(), "new");
        assert!(!current.with_extension("new").exists());
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn the_exe_is_renamed_aside_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("PostRonda.exe");
        std::fs::write(&current, "old").unwrap();
        let unpacked = dir.path().join("unpacked/Post-Ronda-windows-x64");
        std::fs::create_dir_all(&unpacked).unwrap();
        std::fs::write(unpacked.join("PostRonda.exe"), "new").unwrap();
        swap_in(&dir.path().join("unpacked"), &current).unwrap();
        assert_eq!(std::fs::read_to_string(&current).unwrap(), "new");
        assert_eq!(
            std::fs::read_to_string(current.with_extension("exe.old")).unwrap(),
            "old"
        );
    }
}
