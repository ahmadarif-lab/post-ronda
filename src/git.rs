//! Git for a project: the commands behind Pull and Commit & Push, the credentials a project may
//! carry of its own, and the automatic push / pull built on the same two commands.
//!
//! Credentials typed in the app never touch the project folder (they would be committed) and
//! never appear on a command line: an HTTPS token reaches Git through `GIT_ASKPASS`, which points
//! back at this program (see [`answer_askpass`]), and an SSH key through `GIT_SSH_COMMAND`.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Environment variables that carry the HTTPS login to the askpass copy of this program.
const ASKPASS_USER: &str = "POST_RONDA_ASKPASS_USER";
const ASKPASS_PASS: &str = "POST_RONDA_ASKPASS_PASS";

/// Where Git gets its login from.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Auth {
    /// Whatever Git on this computer is set up with (credential manager, ssh-agent, ...).
    #[default]
    System,
    /// An HTTPS username and access token typed in the app.
    Https,
    /// A private key file typed in the app.
    Ssh,
}

/// Git settings of one project. Kept outside the project folder.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub auth: Auth,
    pub username: String,
    pub token: String,
    pub ssh_key: String,
    /// Commit author; empty means "whatever Git is configured with".
    pub author_name: String,
    pub author_email: String,
    /// Commit and push after every save.
    pub auto_push: bool,
    /// Pull from time to time and when the project opens.
    pub auto_pull: bool,
    pub pull_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auth: Auth::System,
            username: String::new(),
            token: String::new(),
            ssh_key: String::new(),
            author_name: String::new(),
            author_email: String::new(),
            auto_push: false,
            auto_pull: false,
            pull_minutes: 2,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|s| serde_json::from_slice(&s).ok())
            .unwrap_or_default()
    }
    /// Written through a temporary file, which is readable by the owner only.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        use std::io::Write;
        let parent = path.parent().ok_or("Invalid Git settings path")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut f = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        f.write_all(&serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        f.persist(path).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// What a Git action did.
#[derive(Debug)]
pub struct Outcome {
    /// Text for the status bar; empty when an automatic action had nothing to do.
    pub message: String,
    /// The checked-out commit moved, so the files on disk are not what the app holds.
    pub changed: bool,
}

impl Outcome {
    fn said(message: &str) -> Self {
        Self {
            message: message.into(),
            changed: false,
        }
    }
    fn nothing() -> Self {
        Self::said("")
    }
}

/// The answer to a Git credential prompt, if this process was started by Git as its askpass
/// helper. `main` calls this first and exits when it returns true.
pub fn answer_askpass() -> bool {
    let (Some(user), Some(pass)) = (
        std::env::var_os(ASKPASS_USER),
        std::env::var_os(ASKPASS_PASS),
    ) else {
        return false;
    };
    let prompt = std::env::args().nth(1).unwrap_or_default();
    println!(
        "{}",
        askpass_reply(&prompt, &user.to_string_lossy(), &pass.to_string_lossy())
    );
    true
}

fn askpass_reply<'a>(prompt: &str, user: &'a str, pass: &'a str) -> &'a str {
    if prompt.to_ascii_lowercase().starts_with("username") {
        user
    } else {
        pass
    }
}

fn git_command(project: &Path, settings: &Settings) -> Result<std::process::Command, String> {
    let mut command = crate::process::command("git");
    command
        .current_dir(project)
        .env("GIT_TERMINAL_PROMPT", "0")
        // Parsed below: keep Git's messages in one language.
        .env("LC_ALL", "C");
    match settings.auth {
        Auth::System => {}
        Auth::Https => {
            if settings.token.trim().is_empty() {
                return Err("Enter the access token in the Git settings of this project.".into());
            }
            let exe = std::env::current_exe().map_err(|e| format!("Git: {e}"))?;
            let user = match settings.username.trim() {
                "" => "git",
                name => name,
            };
            command
                // An empty value resets the list: no stored login is tried before ours.
                .args(["-c", "credential.helper="])
                .env("GIT_ASKPASS", exe)
                .env(ASKPASS_USER, user)
                .env(ASKPASS_PASS, settings.token.trim());
        }
        Auth::Ssh => {
            let key = settings.ssh_key.trim();
            if key.is_empty() {
                return Err("Choose the SSH key file in the Git settings of this project.".into());
            }
            if !Path::new(key).is_file() {
                return Err(format!("SSH key not found: {key}"));
            }
            if key.contains('"') {
                return Err("The SSH key path may not contain a quote.".into());
            }
            command.env(
                "GIT_SSH_COMMAND",
                format!("ssh -i \"{key}\" -o IdentitiesOnly=yes -o BatchMode=yes"),
            );
        }
    }
    if !settings.author_name.trim().is_empty() {
        command.args(["-c", &format!("user.name={}", settings.author_name.trim())]);
    }
    if !settings.author_email.trim().is_empty() {
        command.args([
            "-c",
            &format!("user.email={}", settings.author_email.trim()),
        ]);
    }
    Ok(command)
}

/// The `origin` URL of the project's own repository, or empty when it has none.
pub fn origin_of(project: &Path) -> String {
    crate::process::command("git")
        .current_dir(project)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// Runs one action: `init`, `remote`, `test`, `pull`, `push`, or the automatic `auto-pull` and
/// `auto-push`, which do nothing (and say nothing) when there is nothing to do.
pub fn run_action(
    project: &Path,
    action: &str,
    remote: &str,
    settings: &Settings,
) -> Result<Outcome, String> {
    let run = |args: &[&str]| -> Result<String, String> {
        let out = git_command(project, settings)?
            .args(args)
            .output()
            .map_err(|e| format!("Git: {e}"))?;
        if out.status.success() {
            Ok(format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
            .trim()
            .into())
        } else {
            Err(format!(
                "Git: {}{}",
                String::from_utf8_lossy(&out.stderr),
                String::from_utf8_lossy(&out.stdout)
            )
            .trim()
            .into())
        }
    };
    let head = || run(&["rev-parse", "HEAD"]).unwrap_or_default();
    let has_origin = || run(&["remote", "get-url", "origin"]).is_ok();
    let has_upstream = || run(&["rev-parse", "--abbrev-ref", "@{u}"]).is_ok();
    // Brings in what others pushed. A conflict leaves the project exactly as it was.
    let rebase = || -> Result<(), String> {
        match run(&["pull", "--rebase"]) {
            Ok(_) => Ok(()),
            Err(e) => {
                if run(&["rebase", "--abort"]).is_ok() {
                    return Err(
                        "Someone else changed the same part of the collection. Nothing was \
                         lost; resolve it in your terminal (git pull --rebase), then push."
                            .into(),
                    );
                }
                Err(e)
            }
        }
    };
    if action != "init" && !project.join(".git").exists() {
        return Err("Initialize Git in this project first.".into());
    }
    let auto = action.starts_with("auto-");
    match action {
        "init" => {
            if !project.join(".git").exists() {
                run(&["init", "-b", "main"])?;
            }
            Ok(Outcome::said(
                "Git initialized. Configure origin in Settings.",
            ))
        }
        "remote" => {
            if remote.trim().is_empty() || remote.starts_with('-') {
                return Err("Enter a Git remote URL".into());
            }
            if has_origin() {
                run(&["remote", "set-url", "origin", remote])?;
            } else {
                run(&["remote", "add", "origin", remote])?;
            }
            Ok(Outcome::said("Git origin configured"))
        }
        "test" => {
            if !has_origin() {
                return Err("Set the remote URL (origin) first.".into());
            }
            run(&["ls-remote", "origin"])?;
            Ok(Outcome::said(
                "Connection works: the remote accepted the login",
            ))
        }
        "pull" | "auto-pull" => {
            if auto && !(has_origin() && has_upstream()) {
                return Ok(Outcome::nothing());
            }
            if !run(&["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
                if auto {
                    return Ok(Outcome::nothing());
                }
                return Err("Save and commit local changes before Pull. Use Push to commit and upload, or commit in your terminal.".into());
            }
            let before = head();
            rebase()?;
            let changed = head() != before;
            Ok(Outcome {
                message: match (changed, auto) {
                    (true, _) => "Pulled latest collection".into(),
                    (false, true) => String::new(),
                    (false, false) => "Already up to date".into(),
                },
                changed,
            })
        }
        "push" | "auto-push" => {
            if auto && !has_origin() {
                return Ok(Outcome::nothing());
            }
            let mut files = vec!["add", "--", "collection.json"];
            if project.join("environments.json").exists() {
                files.push("environments.json");
            }
            run(&files)?;
            let diff = run(&["diff", "--cached", "--name-only"])?;
            if diff
                .lines()
                .any(|s| s != "collection.json" && s != "environments.json")
            {
                return Err(
                    "Other files are staged. Commit them in your terminal before using Push."
                        .into(),
                );
            }
            let committed = !diff.is_empty();
            if committed {
                run(&["commit", "-m", "Update API collection"])?;
            }
            if auto && !committed {
                let ahead = run(&["rev-list", "--count", "@{u}..HEAD"]);
                if ahead.is_ok_and(|n| n.trim() == "0") {
                    return Ok(Outcome::nothing());
                }
            }
            let before = head();
            if let Err(rejected) = run(&["push", "--set-upstream", "origin", "HEAD"]) {
                // Most often the remote moved on: take its changes in, then try once more.
                if !has_upstream() {
                    return Err(rejected);
                }
                match rebase() {
                    Ok(()) => {
                        run(&["push", "--set-upstream", "origin", "HEAD"])?;
                    }
                    // Not a conflict (offline, bad login...): the push error says why.
                    Err(e) if e.starts_with("Git:") => return Err(rejected),
                    Err(e) => return Err(e),
                }
            }
            Ok(Outcome {
                message: "Collection pushed".into(),
                // Rebasing onto what others pushed rewrites our commit.
                changed: head_content_changed(project, settings, &before),
            })
        }
        _ => Err("Unknown Git action".into()),
    }
}

/// Whether files differ between `before` and the current HEAD (true after a rebase that brought
/// in somebody else's changes, false when only our own commit was pushed).
fn head_content_changed(project: &Path, settings: &Settings, before: &str) -> bool {
    if before.is_empty() {
        return false;
    }
    git_command(project, settings)
        .ok()
        .and_then(|mut c| {
            c.args([
                "diff",
                "--quiet",
                before,
                "HEAD",
                "--",
                "collection.json",
                "environments.json",
            ])
            .status()
            .ok()
        })
        .is_some_and(|status| status.code() == Some(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }
    /// A project folder with an identity, collection files, and `origin` as its remote.
    fn project(root: &Path, name: &str, origin: &Path) -> std::path::PathBuf {
        let dir = root.join(name);
        std::fs::create_dir(&dir).unwrap();
        run_action(&dir, "init", "", &Settings::default()).unwrap();
        git(&dir, &["config", "user.name", name]);
        git(&dir, &["config", "user.email", "test@postronda.invalid"]);
        run_action(
            &dir,
            "remote",
            origin.to_str().unwrap(),
            &Settings::default(),
        )
        .unwrap();
        dir
    }
    fn origin(root: &Path) -> std::path::PathBuf {
        let path = root.join("origin.git");
        Command::new("git")
            .args(["init", "--bare", "-b", "main"])
            .arg(&path)
            .output()
            .unwrap();
        path
    }
    fn cfg() -> Settings {
        Settings::default()
    }

    #[test]
    fn roundtrip_and_dirty_pull_guard() {
        let root = tempfile::tempdir().unwrap();
        let origin = origin(root.path());
        let a = project(root.path(), "a", &origin);
        std::fs::write(a.join("collection.json"), "{}").unwrap();
        std::fs::write(
            a.join("environments.json"),
            "{\"active\":null,\"items\":[]}",
        )
        .unwrap();
        assert_eq!(origin_of(&a), origin.to_str().unwrap());
        run_action(&a, "push", "", &cfg()).unwrap();
        run_action(&a, "pull", "", &cfg()).unwrap();
        std::fs::write(a.join("collection.json"), "{\"changed\":true}").unwrap();
        assert!(
            run_action(&a, "pull", "", &cfg())
                .err()
                .unwrap()
                .contains("local changes")
        );
        // The automatic pull leaves local edits alone without a word.
        assert_eq!(run_action(&a, "auto-pull", "", &cfg()).unwrap().message, "");
        run_action(&a, "push", "", &cfg()).unwrap();
        assert_eq!(
            git(&origin, &["show", "HEAD:collection.json"]),
            "{\"changed\":true}"
        );
    }

    #[test]
    fn auto_actions_are_quiet_without_a_remote_or_news() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("p");
        std::fs::create_dir(&dir).unwrap();
        run_action(&dir, "init", "", &cfg()).unwrap();
        std::fs::write(dir.join("collection.json"), "{}").unwrap();
        for action in ["auto-push", "auto-pull"] {
            let out = run_action(&dir, action, "", &cfg()).unwrap();
            assert_eq!((out.message.as_str(), out.changed), ("", false));
        }
        // Nothing was committed on the way.
        assert!(
            !Command::new("git")
                .current_dir(&dir)
                .args(["rev-parse", "--verify", "HEAD"])
                .output()
                .unwrap()
                .status
                .success()
        );
    }

    #[test]
    fn a_teammates_push_arrives_and_ours_rebases_onto_it() {
        let root = tempfile::tempdir().unwrap();
        let origin = origin(root.path());
        let a = project(root.path(), "a", &origin);
        let b = project(root.path(), "b", &origin);
        std::fs::write(a.join("collection.json"), "{\"v\":1}").unwrap();
        run_action(&a, "auto-push", "", &cfg()).unwrap();
        // b has nothing yet and no upstream: the first push brings it level.
        run_action(&b, "pull", "", &cfg()).unwrap_err();
        git(&b, &["pull", "origin", "main"]);
        git(&b, &["branch", "--set-upstream-to=origin/main", "main"]);
        // Nothing new: quiet.
        let quiet = run_action(&b, "auto-pull", "", &cfg()).unwrap();
        assert_eq!((quiet.message.as_str(), quiet.changed), ("", false));
        // a pushes again; b's automatic pull picks it up.
        std::fs::write(a.join("collection.json"), "{\"v\":2}").unwrap();
        run_action(&a, "auto-push", "", &cfg()).unwrap();
        let news = run_action(&b, "auto-pull", "", &cfg()).unwrap();
        assert!(news.changed);
        assert_eq!(
            std::fs::read_to_string(b.join("collection.json")).unwrap(),
            "{\"v\":2}"
        );
        // Both edit different files at once; b pushes second and still succeeds.
        std::fs::write(a.join("collection.json"), "{\"v\":3}").unwrap();
        run_action(&a, "auto-push", "", &cfg()).unwrap();
        std::fs::write(b.join("environments.json"), "{\"b\":1}").unwrap();
        let out = run_action(&b, "auto-push", "", &cfg()).unwrap();
        assert_eq!(out.message, "Collection pushed");
        assert!(out.changed, "a's collection.json came in with the rebase");
        assert_eq!(
            std::fs::read_to_string(b.join("collection.json")).unwrap(),
            "{\"v\":3}"
        );
        assert_eq!(
            git(&origin, &["show", "HEAD:environments.json"]),
            "{\"b\":1}"
        );
    }

    #[test]
    fn a_conflict_is_reported_and_nothing_is_lost() {
        let root = tempfile::tempdir().unwrap();
        let origin = origin(root.path());
        let a = project(root.path(), "a", &origin);
        let b = project(root.path(), "b", &origin);
        std::fs::write(a.join("collection.json"), "{\"v\":1}").unwrap();
        run_action(&a, "push", "", &cfg()).unwrap();
        git(&b, &["pull", "origin", "main"]);
        git(&b, &["branch", "--set-upstream-to=origin/main", "main"]);
        std::fs::write(a.join("collection.json"), "{\"v\":\"from a\"}").unwrap();
        run_action(&a, "push", "", &cfg()).unwrap();
        std::fs::write(b.join("collection.json"), "{\"v\":\"from b\"}").unwrap();
        let err = run_action(&b, "auto-push", "", &cfg()).err().unwrap();
        assert!(err.contains("same part"), "{err}");
        // b is back to its own commit, no rebase left half way.
        assert_eq!(
            std::fs::read_to_string(b.join("collection.json")).unwrap(),
            "{\"v\":\"from b\"}"
        );
        assert!(git(&b, &["status", "--porcelain"]).is_empty());
        assert!(!b.join(".git/rebase-merge").exists());
    }

    #[test]
    fn credentials_become_environment_not_arguments() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        let https = Settings {
            auth: Auth::Https,
            username: "me".into(),
            token: "s3cret".into(),
            ..Settings::default()
        };
        let command = git_command(dir, &https).unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["-c", "credential.helper="]);
        let envs: std::collections::HashMap<_, _> = command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert_eq!(envs[ASKPASS_PASS].as_deref(), Some("s3cret"));
        assert_eq!(envs[ASKPASS_USER].as_deref(), Some("me"));
        assert!(envs.contains_key("GIT_ASKPASS"));
        // An empty token or a missing key is explained, not attempted.
        assert!(
            git_command(
                dir,
                &Settings {
                    auth: Auth::Https,
                    ..Settings::default()
                }
            )
            .is_err()
        );
        let ssh = |key: &str| Settings {
            auth: Auth::Ssh,
            ssh_key: key.into(),
            ..Settings::default()
        };
        assert!(git_command(dir, &ssh("")).is_err());
        assert!(
            git_command(dir, &ssh("/no/such/key"))
                .unwrap_err()
                .contains("not found")
        );
        let key = dir.join("id_test");
        std::fs::write(&key, "x").unwrap();
        let command = git_command(dir, &ssh(key.to_str().unwrap())).unwrap();
        assert!(command.get_envs().any(|(k, v)| k == "GIT_SSH_COMMAND"
            && v.is_some_and(|v| v.to_string_lossy().contains("IdentitiesOnly"))));
    }

    #[test]
    fn askpass_answers_username_and_password_prompts() {
        assert_eq!(
            askpass_reply("Username for 'https://github.com': ", "me", "pw"),
            "me"
        );
        assert_eq!(
            askpass_reply("Password for 'https://me@github.com': ", "me", "pw"),
            "pw"
        );
    }

    #[test]
    fn settings_roundtrip_and_tolerate_old_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("git").join("x.json");
        assert_eq!(Settings::load(&path), Settings::default());
        let s = Settings {
            auto_push: true,
            pull_minutes: 5,
            token: "t".into(),
            ..Settings::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        std::fs::write(&path, "{\"auto_pull\":true}").unwrap();
        assert!(Settings::load(&path).auto_pull);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            s.save(&path).unwrap();
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
                0
            );
        }
    }
}
