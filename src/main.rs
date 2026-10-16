#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#[cfg(debug_assertions)]
mod capture;
mod style;
mod view;
use eframe::egui::{self, Color32, RichText, TextEdit};
use post_ronda::collection::FolderMark;
use post_ronda::{
    Header, Request,
    collection::{self, Collection},
    curl,
    http::{self, Response},
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

enum Message {
    Response(Box<Request>, Result<Response, String>),
    Git(Result<String, String>),
    /// Result of an update check; the flag says the user asked for it (not the start-up check).
    Update(bool, Result<post_ronda::update::Release, String>),
}
/// Where the update check stands, as shown in App settings > About.
#[derive(Clone, Debug, Default)]
enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(post_ronda::update::Release),
    Failed(String),
}
/// How much of the collection a disk write may take from the live (possibly edited) state.
#[derive(Clone, Copy)]
enum SaveScope {
    /// Everything as it is now, including unsaved edits.
    Everything,
    /// Only this request's edits; other requests keep their last saved content.
    Request(usize),
    /// Names, folders, settings and environments; edits to request content stay unsaved.
    Structural,
}
/// Short-lived confirmation shown at the top of the window after an action.
struct Toast {
    text: String,
    error: bool,
    at: Instant,
}
/// A project the user has opened before: a folder holding `collection.json` (and, optionally,
/// `environments.json` and a git repository).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ProjectEntry {
    path: PathBuf,
    name: String,
}
/// What is being dragged in the sidebar.
#[derive(Clone, Debug, PartialEq)]
enum Dragged {
    Request(usize),
    /// Full folder path.
    Folder(String),
}
/// Where a drag may be dropped.
#[derive(Clone, Debug, PartialEq)]
enum DropTarget {
    Before(usize),
    After(usize),
    IntoFolder(String),
    /// Next to a folder, as a sibling of it.
    BeforeFolder(String),
    AfterFolder(String),
    Root,
}
/// What the shared name dialog is editing.
#[derive(Clone, Debug, PartialEq)]
enum NameTarget {
    Request(usize),
    Folder(String),
    Collection,
    NewFolder(String),
}
struct App {
    collection: Collection,
    selected: usize,
    project: PathBuf,
    tab: usize,
    response_tab: usize,
    response: Option<Response>,
    response_text: String,
    pretty: bool,
    notice: String,
    error: bool,
    import_open: bool,
    curl_text: String,
    search: String,
    remote: String,
    settings: bool,
    tx: mpsc::Sender<Message>,
    rx: mpsc::Receiver<Message>,
    running: bool,
    git_running: bool,
    cancel: Arc<AtomicBool>,
    dirty: Option<Instant>,
    history: Vec<Request>,
    history_view: bool,
    confirm_delete: bool,
    prefs: style::Preferences,
    prefs_tab: usize,
    update_state: UpdateState,
    update_checked: bool,
    /// Show the password / token fields of the Auth tab in clear text.
    show_secret: bool,
    /// Settings button: first asks whether app or project settings are meant.
    settings_chooser: bool,
    project_settings: bool,
    page: usize,
    sidebar_width: f32,
    sidebar_visible: bool,
    split_ratio: f32,
    split_ratio_v: f32,
    code_line_len: Vec<usize>,
    /// Selected text of the response viewer as (line, column) endpoints, in drag order. Kept here
    /// rather than in egui because the viewer only draws the lines in view.
    code_sel: Option<((usize, usize), (usize, usize))>,
    code_anchor: Option<(usize, usize)>,
    code_selecting: bool,
    /// Find in the response: query, matches as (line, start, end) in characters, current one.
    /// Highlighted row of the `{{` variable suggestions under the URL field.
    var_sel: usize,
    find_open: bool,
    find_focus: bool,
    find_query: String,
    find_for: String,
    find_matches: Vec<(usize, usize, usize)>,
    find_index: usize,
    find_jump: bool,
    response_rect: egui::Rect,
    /// Selected request and tab count when the tab strip last scrolled the active tab into view.
    tab_scroll_seen: (usize, usize),
    tabs: Vec<usize>,
    /// The tab opened by a single click in the sidebar and not touched since: the next such
    /// click replaces it instead of adding another tab.
    preview: Option<usize>,
    environments: post_ronda::environment::Environments,
    editing_environment: Option<usize>,
    collection_tab: usize,
    draft_header: Header,
    /// Request index and table currently shown as Bulk Edit text.
    bulk_target: Option<(usize, &'static str)>,
    bulk_buf: String,
    code_lines: Vec<egui::text::LayoutJob>,
    palette_dark: bool,
    response_cache: std::collections::BTreeMap<usize, Response>,
    search_open: bool,
    palette_query: String,
    palette_prev_query: String,
    /// Filter typed in the project switcher.
    project_filter: String,
    palette_sel: usize,
    palette_ready: bool,
    focus_url: bool,
    codegen_open: bool,
    codegen_lang: usize,
    codegen_client: usize,
    codegen_interpolate: bool,
    save_open: bool,
    save_focus: bool,
    save_name: String,
    save_folder: String,
    save_new_folder: Option<String>,
    /// Requests created in this session that have not been through the Save dialog yet.
    drafts: std::collections::BTreeSet<usize>,
    sent_at: Option<Instant>,
    rename_open: bool,
    rename_target: NameTarget,
    rename_name: String,
    delete_folder: Option<String>,
    delete_count: usize,
    info_open: bool,
    info_index: usize,
    delete_label: String,
    toast: Option<Toast>,
    import_error: String,
    focus_filter: bool,
    /// Projects shown in the switcher, in the order they were added.
    projects: Vec<ProjectEntry>,
    np_open: bool,
    np_name: String,
    np_parent: PathBuf,
    np_import: Option<Collection>,
    /// Collections dropped together, waiting for the import dialog to be free.
    import_queue: Vec<(Collection, Vec<String>)>,
    /// What the collection in the import dialog could not bring along (scripts, some auth).
    np_notes: Vec<String>,
    /// For an import: add to the open project instead of creating a new one.
    np_merge: bool,
    np_error: String,
    /// Each request as last saved, so "Revert Changes" and "Close Saved" know what changed.
    baseline: std::collections::BTreeMap<usize, Request>,
    /// Collection settings and environments as last written, to notice changes that autosave.
    meta_saved: Collection,
    env_saved: post_ronda::environment::Environments,
    close_prompt: bool,
    force_close: bool,
}
fn state_dir() -> PathBuf {
    // Tests point this at a throwaway folder so they never touch real preferences or projects.
    if let Some(dir) = std::env::var_os("POST_RONDA_STATE_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Post Ronda")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join("Post Ronda")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or(home.join(".local/share"))
            .join("post-ronda")
    }
}
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Post Ronda")
            .with_inner_size([1280., 800.])
            .with_decorations(false)
            .with_icon(window_icon())
            .with_min_inner_size([900., 600.]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Post Ronda",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
fn window_icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../assets/window-icon.png"))
        .expect("bundled window icon")
        .into_rgba8();
    let (width, height) = img.dimensions();
    egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    }
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        #[allow(unused_mut)]
        let mut prefs = style::Preferences::load(&state_dir().join("preferences.json"));
        #[cfg(debug_assertions)]
        if let Ok(theme) = std::env::var("POST_RONDA_THEME") {
            prefs.appearance = match theme.as_str() {
                "light" => style::Appearance::Light,
                "system" => style::Appearance::System,
                _ => style::Appearance::Dark,
            };
        }
        style::setup(&cc.egui_ctx, &prefs);
        let project = std::env::args_os()
            .nth(1)
            .map(PathBuf::from)
            .or_else(|| {
                std::fs::read_to_string(state_dir().join("last-project"))
                    .ok()
                    .map(PathBuf::from)
            })
            .unwrap_or(state_dir().join("workspace"));
        let loaded = std::fs::read_to_string(project.join("collection.json"));
        let mut notice = "Ready · Everything stays on your computer".to_owned();
        let mut error = false;
        let collection = match loaded {
            Ok(s) => match collection::import(&s) {
                Ok(c) => c,
                Err(e) => {
                    notice = format!(
                        "Cannot open collection: {e}. Original file will not be overwritten until you save."
                    );
                    error = true;
                    Collection::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Collection::default(),
            Err(e) => {
                notice = e.to_string();
                error = true;
                Collection::default()
            }
        };
        let environments =
            match post_ronda::environment::Environments::load(&project.join("environments.json")) {
                Ok(e) => e,
                Err(e) => {
                    notice = e;
                    error = true;
                    Default::default()
                }
            };
        let (tx, rx) = mpsc::channel();
        #[allow(unused_mut)]
        let mut app = Self {
            collection,
            selected: 0,
            project,
            tab: 0,
            response_tab: 0,
            response: None,
            response_text: String::new(),
            pretty: true,
            notice,
            error,
            import_open: false,
            curl_text: String::new(),
            search: String::new(),
            remote: String::new(),
            settings: false,
            tx,
            rx,
            running: false,
            git_running: false,
            cancel: Arc::new(AtomicBool::new(false)),
            dirty: None,
            history: vec![],
            history_view: false,
            confirm_delete: false,
            prefs,
            prefs_tab: 0,
            update_state: UpdateState::Idle,
            update_checked: false,
            show_secret: false,
            settings_chooser: false,
            project_settings: false,
            page: 0,
            sidebar_width: 250.,
            sidebar_visible: true,
            split_ratio: 0.48,
            split_ratio_v: 0.5,
            code_line_len: vec![],
            code_sel: None,
            code_anchor: None,
            code_selecting: false,
            var_sel: 0,
            find_open: false,
            find_focus: false,
            find_query: String::new(),
            find_for: String::new(),
            find_matches: vec![],
            find_index: 0,
            find_jump: false,
            response_rect: egui::Rect::NOTHING,
            tab_scroll_seen: (usize::MAX, 0),
            tabs: vec![0],
            preview: None,
            environments,
            editing_environment: None,
            collection_tab: 0,
            draft_header: Header::new("", ""),
            bulk_target: None,
            bulk_buf: String::new(),
            code_lines: vec![],
            palette_dark: cc.egui_ctx.theme() == egui::Theme::Dark,
            response_cache: Default::default(),
            search_open: false,
            palette_query: String::new(),
            palette_prev_query: String::new(),
            project_filter: String::new(),
            palette_sel: 0,
            palette_ready: false,
            focus_url: true,
            codegen_open: false,
            codegen_lang: 0,
            codegen_client: 0,
            codegen_interpolate: true,
            save_open: false,
            save_focus: false,
            save_name: String::new(),
            save_folder: String::new(),
            save_new_folder: None,
            drafts: Default::default(),
            sent_at: None,
            rename_open: false,
            rename_target: NameTarget::Collection,
            rename_name: String::new(),
            delete_folder: None,
            delete_count: 0,
            info_open: false,
            info_index: 0,
            delete_label: String::new(),
            toast: None,
            import_error: String::new(),
            focus_filter: false,
            projects: vec![],
            np_open: false,
            np_name: String::new(),
            np_parent: PathBuf::new(),
            np_import: None,
            import_queue: vec![],
            np_notes: vec![],
            np_merge: false,
            np_error: String::new(),
            baseline: Default::default(),
            meta_saved: Default::default(),
            env_saved: Default::default(),
            close_prompt: false,
            force_close: false,
        };
        app.baseline = app
            .collection
            .requests
            .iter()
            .cloned()
            .enumerate()
            .collect();
        app.meta_saved = Collection {
            requests: vec![],
            ..app.collection.clone()
        };
        app.env_saved = app.environments.clone();
        app.remote = origin_of(&app.project);
        app.history = load_history(&app.project);
        #[cfg(debug_assertions)]
        {
            // POST_RONDA_THEME_INDEX=n previews theme n of the active mode;
            // POST_RONDA_STACKED=1 previews the stacked layout.
            if let Some(n) = std::env::var("POST_RONDA_THEME_INDEX")
                .ok()
                .and_then(|v| v.parse().ok())
            {
                app.prefs.dark_theme = n;
                app.prefs.light_theme = n;
                style::apply(&cc.egui_ctx, &app.prefs);
            }
            if std::env::var_os("POST_RONDA_STACKED").is_some() {
                app.prefs.stacked_layout = true;
            }
        }
        #[cfg(debug_assertions)]
        match std::env::var("POST_RONDA_CAPTURE_VIEW")
            .unwrap_or_default()
            .as_str()
        {
            "headers" => app.tab = 2,
            "search" => app.search_open = true,
            "codegen" => {
                app.codegen_open = true;
                // POST_RONDA_CODEGEN="language,client,request" picks what the dialog shows.
                if let Ok(spec) = std::env::var("POST_RONDA_CODEGEN") {
                    let n: Vec<usize> = spec.split(',').filter_map(|v| v.parse().ok()).collect();
                    app.codegen_lang = n.first().copied().unwrap_or(0);
                    app.codegen_client = n.get(1).copied().unwrap_or(0);
                    app.selected = n.get(2).copied().unwrap_or(0);
                }
            }
            "delete" => app.confirm_delete = true,
            "emptyfolders" => {
                let end = app.collection.requests.len();
                app.collection.folders =
                    vec![FolderMark::new("Satu", end), FolderMark::new("Dua", end)];
            }
            merge @ ("importchoice" | "importmerge") => {
                let sample = app.collection.clone();
                app.begin_new_project(Some(sample));
                app.np_merge = merge == "importmerge";
            }
            "newproject" => app.begin_new_project(None),
            "import" => app.import_open = true,
            "pinned" => {
                app.collection.pinned = vec![
                    "Users".into(),
                    "Users / Create user".into(),
                    "Getting started / Echo request".into(),
                ];
            }
            "vars" => app.tab = 4,
            view if view.starts_with("body-") => {
                // POST_RONDA_CAPTURE_VIEW=body-form | body-urlencoded | body-bulk | body-raw | body-binary
                use post_ronda::BodyMode;
                let sel = app.selected;
                app.tab = 1;
                let r = &mut app.collection.requests[sel];
                let mut file = Header::new("avatar", "/Users/me/photos/profile.png");
                file.file = true;
                match view {
                    "body-form" => {
                        r.body_mode = BodyMode::FormData;
                        r.form = vec![Header::new("name", "Arif"), file];
                    }
                    "body-urlencoded" | "body-bulk" => {
                        r.body_mode = BodyMode::UrlEncoded;
                        r.urlencoded = vec![Header::new("name", "Arif"), Header::new("age", "35")];
                        if view == "body-bulk" {
                            app.bulk_buf = "name:Arif\nage:35".into();
                            app.bulk_target = Some((sel, "urlencoded"));
                        }
                    }
                    "body-binary" => {
                        r.body_mode = BodyMode::Binary;
                        r.binary_path = "/Users/me/photos/profile.png".into();
                    }
                    _ => {
                        r.body_mode = BodyMode::Raw;
                        r.body = "{\"name\":\"Arif\",\"age\":35}".into();
                    }
                }
            }
            "close" => app.close_prompt = true,
            "import-notes" => {
                let (c, notes) = collection::import_with_notes(
                    r#"{"info":{"name":"Wakaf"},"event":[{}],"auth":{"type":"digest"},"item":[{"name":"A","request":{"method":"GET","url":"http://a.test"}}]}"#,
                )
                .unwrap();
                app.begin_new_project(Some(c));
                app.np_notes = notes;
            }
            "tabs" => {
                app.tabs = vec![0, 1, 2];
                app.selected = 2;
            }
            "many-tabs" => {
                let template = app.collection.requests[0].clone();
                for i in 0..14 {
                    let mut r = template.clone();
                    r.name = format!("Folder / Request number {i}");
                    r.method =
                        ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD"][i % 7].into();
                    app.collection.requests.push(r);
                }
                app.tabs = (0..app.collection.requests.len()).collect();
                app.selected = app.collection.requests.len() - 5;
                app.preview = Some(app.selected);
            }
            "toast" => {
                app.codegen_open = true;
                app.notify(Ok("Copied to clipboard!".into()));
            }
            "loading" => {
                app.running = true;
                app.sent_at = Some(Instant::now());
            }
            "save" => {
                app.new_request();
                app.save_request();
            }
            "themes" => {
                app.settings = true;
                app.prefs_tab = 1;
            }
            "settings-chooser" => app.settings_chooser = true,
            "settings-about" => {
                app.settings = true;
                app.prefs_tab = 5;
            }
            "settings-network" => {
                app.settings = true;
                app.prefs_tab = 3;
            }
            "settings-display" => {
                app.settings = true;
                app.prefs_tab = 2;
            }
            "auth-tab" => {
                app.tab = 3;
                let sel = app.selected;
                app.collection.requests[sel].auth = post_ronda::Auth {
                    kind: post_ronda::AuthKind::ApiKey,
                    key: "X-API-Key".into(),
                    value: "{{token}}".into(),
                    ..Default::default()
                };
            }
            "project-git" => app.open_project_settings(5),
            "project-vars" => app.open_project_settings(2),
            "environments" => {
                app.page = 2;
                app.editing_environment = app.environments.active;
            }
            _ => {}
        }
        app.projects = Self::load_registry();
        app.register_current();
        app
    }
    fn notify(&mut self, result: Result<String, String>) {
        match result {
            Ok(s) => {
                self.notice = s;
                self.error = false;
            }
            Err(e) => {
                self.notice = e;
                self.error = true;
            }
        }
        // Routine progress stays in the status bar; results of an action also get a toast.
        if !matches!(
            self.notice.as_str(),
            "Response received" | "Request completed in another tab"
        ) {
            self.toast = Some(Toast {
                text: self.notice.clone(),
                error: self.error,
                at: Instant::now(),
            });
        }
    }
    fn save(&mut self) -> Result<(), String> {
        self.write(SaveScope::Everything)
    }
    /// True when the request differs from what was last written (names are saved on rename).
    fn request_unsaved(&self, index: usize) -> bool {
        self.baseline.get(&index).is_some_and(|saved| {
            let mut saved = saved.clone();
            saved.name = self.collection.requests[index].name.clone();
            saved != self.collection.requests[index]
        })
    }
    /// Settings, environments, folders or names that changed and should autosave.
    fn pending_structural(&self) -> bool {
        let (c, m) = (&self.collection, &self.meta_saved);
        c.name != m.name
            || c.description != m.description
            || c.variables != m.variables
            || c.headers != m.headers
            || c.default_timeout != m.default_timeout
            || c.follow_redirects != m.follow_redirects
            || c.insecure != m.insecure
            || c.folders != m.folders
            || c.pinned != m.pinned
            || self.environments != self.env_saved
            || c.requests
                .iter()
                .enumerate()
                .any(|(i, r)| self.baseline.get(&i).is_some_and(|b| b.name != r.name))
    }
    fn draft_has_content(&self, index: usize) -> bool {
        let r = &self.collection.requests[index];
        !(r.url.is_empty() && r.body.is_empty() && r.headers.is_empty())
    }
    /// Requests whose edits would be lost on exit: edited saved ones and drafts with content.
    fn unsaved_count(&self) -> usize {
        (0..self.collection.requests.len())
            .filter(|&i| {
                if self.drafts.contains(&i) {
                    self.draft_has_content(i)
                } else {
                    self.request_unsaved(i)
                }
            })
            .count()
    }
    fn write(&mut self, scope: SaveScope) -> Result<(), String> {
        std::fs::create_dir_all(&self.project).map_err(|e| e.to_string())?;
        if let Ok(original) = std::fs::read(self.project.join("collection.json"))
            && std::str::from_utf8(&original).map_or(true, |s| collection::import(s).is_err())
        {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            std::fs::write(
                self.project
                    .join(format!("collection.recovery-{stamp}.json")),
                original,
            )
            .map_err(|e| e.to_string())?;
        }
        let mut saved = self.collection.clone();
        saved.requests.clear();
        let mut written = std::collections::BTreeMap::new();
        for (i, current) in self.collection.requests.iter().enumerate() {
            if self.drafts.contains(&i) {
                continue;
            }
            let take_current = match scope {
                SaveScope::Everything => true,
                SaveScope::Request(target) => target == i,
                SaveScope::Structural => false,
            };
            let request = if take_current || !self.request_unsaved(i) {
                current.clone()
            } else {
                let mut kept = self.baseline[&i].clone();
                kept.name = current.name.clone();
                kept
            };
            saved.requests.push(request.clone());
            written.insert(i, request);
        }
        let bytes =
            serde_json::to_vec_pretty(&collection::export(&saved)).map_err(|e| e.to_string())?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.project).map_err(|e| e.to_string())?;
        use std::io::Write;
        temp.write_all(&bytes).map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        temp.persist(self.project.join("collection.json"))
            .map_err(|e| e.to_string())?;
        self.environments
            .save(&self.project.join("environments.json"))?;
        self.dirty = None;
        self.baseline = written;
        self.meta_saved = Collection {
            requests: vec![],
            ..self.collection.clone()
        };
        self.env_saved = self.environments.clone();
        std::fs::create_dir_all(state_dir()).map_err(|e| e.to_string())?;
        std::fs::write(
            state_dir().join("last-project"),
            self.project.to_string_lossy().as_bytes(),
        )
        .map_err(|e| e.to_string())?;
        // A renamed collection is renamed in the project switcher too.
        self.register_current();
        Ok(())
    }
    fn projects_file() -> PathBuf {
        state_dir().join("projects.json")
    }
    fn load_registry() -> Vec<ProjectEntry> {
        std::fs::read(Self::projects_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    fn save_registry(&self) {
        let _ = std::fs::create_dir_all(state_dir());
        if let Ok(bytes) = serde_json::to_vec_pretty(&self.projects) {
            let _ = std::fs::write(Self::projects_file(), bytes);
        }
    }
    /// Name of the collection stored in a project folder, or the folder's own name.
    fn project_name(path: &std::path::Path) -> String {
        std::fs::read_to_string(path.join("collection.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["info"]["name"].as_str().map(str::to_owned))
            .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Project".into())
    }
    /// Makes sure the open project is in the switcher with its current name.
    fn register_current(&mut self) {
        let entry = ProjectEntry {
            path: self.project.clone(),
            name: self.collection.name.clone(),
        };
        match self.projects.iter_mut().find(|p| p.path == entry.path) {
            Some(existing) if *existing == entry => return,
            Some(existing) => *existing = entry,
            None => self.projects.push(entry),
        }
        self.save_registry();
    }
    /// Reads a project folder. A folder without `collection.json` is a new, empty project.
    fn read_project(
        path: &std::path::Path,
    ) -> Result<(Collection, post_ronda::environment::Environments, bool), String> {
        let (collection, fresh) = match std::fs::read_to_string(path.join("collection.json")) {
            Ok(s) => {
                let c = collection::import(&s)?;
                let fresh = c.requests == [Request::default()];
                (c, fresh)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Collection::default(), true),
            Err(e) => return Err(e.to_string()),
        };
        let environments =
            post_ronda::environment::Environments::load(&path.join("environments.json"))?;
        Ok((collection, environments, fresh))
    }
    /// Replaces everything that belongs to the open project; nothing of the previous one
    /// (tabs, drafts, baselines, cached responses, search) is carried over.
    fn load_project(&mut self, path: PathBuf) -> Result<(), String> {
        let (collection, environments, fresh) = Self::read_project(&path)?;
        self.project = path;
        self.collection = collection;
        self.environments = environments;
        self.selected = 0;
        self.tabs = vec![0];
        self.preview = None;
        // Git belongs to the project: show its own origin, not the one typed for another.
        self.remote = origin_of(&self.project);
        self.history = load_history(&self.project);
        self.tab = 0;
        self.page = 0;
        self.settings = false;
        self.response = None;
        self.response_cache.clear();
        self.drafts = if fresh {
            [0].into()
        } else {
            Default::default()
        };
        self.baseline = self
            .collection
            .requests
            .iter()
            .cloned()
            .enumerate()
            .filter(|(i, _)| !self.drafts.contains(i))
            .collect();
        self.meta_saved = Collection {
            requests: vec![],
            ..self.collection.clone()
        };
        self.env_saved = self.environments.clone();
        self.dirty = None;
        self.search.clear();
        self.bulk_target = None;
        self.editing_environment = None;
        self.confirm_delete = false;
        self.rename_open = false;
        self.info_open = false;
        self.save_open = false;
        self.running = false;
        self.response_body();
        self.register_current();
        let _ = std::fs::create_dir_all(state_dir());
        let _ = std::fs::write(
            state_dir().join("last-project"),
            self.project.to_string_lossy().as_bytes(),
        );
        Ok(())
    }
    /// Switches to another project, but only when nothing would be lost by leaving.
    fn switch_project(&mut self, path: PathBuf) {
        if path == self.project {
            return;
        }
        if self.unsaved_count() > 0 {
            self.notify(Err(
                "Save your unsaved requests first (⌘S), then switch project".into(),
            ));
            return;
        }
        if self.running {
            self.notify(Err(
                "Wait for the running request to finish, then switch project".into(),
            ));
            return;
        }
        if (self.dirty.is_some() || self.pending_structural())
            && let Err(e) = self.write(SaveScope::Structural)
        {
            self.notify(Err(e));
            return;
        }
        let name = Self::project_name(&path);
        match self.load_project(path) {
            Ok(()) => self.notify(Ok(format!("Switched to {name}"))),
            Err(e) => self.notify(Err(e)),
        }
    }
    fn open_project(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open project folder")
            .pick_folder()
        {
            self.switch_project(path);
        }
    }
    fn default_projects_dir() -> PathBuf {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("Documents")
            .join("Post Ronda")
    }
    /// Opens the "New project" dialog, optionally pre-filled with an imported collection.
    fn begin_new_project(&mut self, import: Option<Collection>) {
        self.np_name = import
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "My project".into());
        self.np_import = import;
        self.np_notes.clear();
        self.np_merge = false;
        self.np_error.clear();
        if self.np_parent.as_os_str().is_empty() {
            self.np_parent = Self::default_projects_dir();
        }
        self.np_open = true;
    }
    /// Picks a collection file and opens the import dialog, where the user decides between a
    /// project of its own and adding the contents to the open project.
    fn import_collection_file(&mut self, merge: bool) {
        let Some(file) = rfd::FileDialog::new()
            .set_title("Import a Postman collection or an OpenAPI file")
            .add_filter("Postman collection or OpenAPI", &["json", "yaml", "yml"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|s| collection::import_with_notes(&s))
        {
            Ok((c, notes)) => {
                self.begin_new_project(Some(c));
                self.np_notes = notes;
                self.np_merge = merge;
            }
            Err(e) => self.notify(Err(e)),
        }
    }
    /// Files or folders dropped on the window: collections open the import dialog, environments
    /// are added to the project, a project folder is opened.
    fn handle_dropped(&mut self, files: Vec<egui::DroppedFileHandle>) {
        let mut collections = vec![];
        let (mut environments, mut errors) = (0, vec![]);
        for file in files {
            let path = file.path().to_path_buf();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if path.is_dir() {
                self.switch_project(path);
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) => {
                    errors.push(format!("{name}: {e}"));
                    continue;
                }
            };
            let is_environment = serde_json::from_str::<serde_json::Value>(&text).is_ok_and(|v| {
                (v.get("values").is_some()
                    || v.get("environment")
                        .is_some_and(|e| e.get("values").is_some()))
                    && v.get("item").is_none()
            });
            if is_environment {
                match post_ronda::environment::Environment::from_json(&text) {
                    Ok(e) => {
                        self.environments.items.push(e);
                        self.editing_environment = Some(self.environments.items.len() - 1);
                        environments += 1;
                    }
                    Err(e) => errors.push(format!("{name}: {e}")),
                }
            } else {
                match collection::import_with_notes(&text) {
                    Ok(imported) => collections.push(imported),
                    Err(_) => errors.push(format!(
                        "{name} is not a Postman collection, OpenAPI file or environment"
                    )),
                }
            }
        }
        let mut collections = collections.into_iter();
        if let Some((first, notes)) = collections.next() {
            self.begin_new_project(Some(first));
            self.np_notes = notes;
            self.import_queue.extend(collections);
        }
        if let Some(e) = errors.into_iter().next() {
            self.notify(Err(e));
        } else if environments > 0 {
            self.notify(Ok(format!("{environments} environment(s) imported")));
        }
    }
    /// Adds the requests, folders and variables of `imported` to the open project. Returns how
    /// many requests were added.
    fn merge_collection(&mut self, imported: Collection) -> Result<usize, String> {
        if let Some(clash) = imported.variables.iter().find(|v| {
            self.collection
                .variables
                .iter()
                .any(|existing| existing.name == v.name && existing.value != v.value)
        }) {
            return Err(format!(
                "Variable \"{}\" already has a different value here. Import it as a new project, or rename the variable first.",
                clash.name
            ));
        }
        // An untouched project only holds its hidden placeholder request; that goes away.
        let placeholder = self.collection.requests.len() == 1
            && self.drafts.contains(&0)
            && !self.draft_has_content(0);
        let added = imported.requests.len();
        let base = self.collection.requests.len();
        for mut r in imported.requests {
            // Collection-level headers would be lost otherwise, so each request carries them.
            let inherited: Vec<_> = imported
                .headers
                .iter()
                .filter(|h| {
                    !r.headers
                        .iter()
                        .any(|rh| rh.name.eq_ignore_ascii_case(&h.name))
                })
                .cloned()
                .collect();
            r.headers.extend(inherited);
            self.collection.requests.push(r);
        }
        for v in imported.variables {
            if !self.collection.variables.iter().any(|e| e.name == v.name) {
                self.collection.variables.push(v);
            }
        }
        for mark in imported.folders {
            if !self.collection.folders.iter().any(|m| m.path == mark.path) {
                self.collection
                    .folders
                    .push(FolderMark::new(mark.path, base + mark.before));
            }
        }
        if placeholder {
            self.remove_request(0);
        }
        self.selected = self.selected.min(self.collection.requests.len() - 1);
        self.dirty = Some(Instant::now());
        Ok(added)
    }
    fn create_project(&mut self) -> Result<(), String> {
        let name = self.np_name.trim().to_owned();
        if name.is_empty() {
            return Err("Give the project a name".into());
        }
        if self.unsaved_count() > 0 {
            return Err("Save your unsaved requests first (⌘S), then create the project".into());
        }
        let folder: String = name
            .chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                    '-'
                } else {
                    c
                }
            })
            .collect();
        let dir = self.np_parent.join(folder.trim());
        if dir.join("collection.json").exists() {
            return Err(format!(
                "{} already holds a project. Pick another name or location, or open it instead.",
                dir.display()
            ));
        }
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut collection = self.np_import.take().unwrap_or(Collection {
            requests: vec![],
            ..Collection::default()
        });
        collection.name = name;
        let bytes = serde_json::to_vec_pretty(&collection::export(&collection))
            .map_err(|e| e.to_string())?;
        std::fs::write(dir.join("collection.json"), bytes).map_err(|e| e.to_string())?;
        if (self.dirty.is_some() || self.pending_structural())
            && let Err(e) = self.write(SaveScope::Structural)
        {
            return Err(e);
        }
        self.load_project(dir)
    }
    fn remove_project_entry(&mut self, path: &std::path::Path) {
        if path != self.project {
            self.projects.retain(|p| p.path != path);
            self.save_registry();
        }
    }
    /// Collection as a plain Postman v2.1 file, ready to import into Postman.
    fn export_postman(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{}.postman_collection.json", self.collection.name))
            .add_filter("Postman Collection", &["json"])
            .save_file()
        {
            let result = serde_json::to_vec_pretty(&collection::export_postman(&self.collection))
                .map_err(|e| e.to_string())
                .and_then(|bytes| std::fs::write(path, bytes).map_err(|e| e.to_string()));
            self.notify(result.map(|_| "Exported as a Postman collection".into()));
        }
    }
    fn export_file(&mut self, docs: bool) {
        let ext = if docs { "md" } else { "json" };
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(if docs { "README.md" } else { "collection.json" })
            .add_filter("Export", &[ext])
            .save_file()
        {
            let s = if docs {
                collection::markdown(&self.collection)
            } else {
                serde_json::to_string_pretty(&collection::export(&self.collection)).unwrap()
            };
            self.notify(
                std::fs::write(path, s)
                    .map(|_| "Exported".into())
                    .map_err(|e| e.to_string()),
            );
        }
    }
    fn send(&mut self, ctx: &egui::Context) {
        if self.running {
            return;
        }
        // Sending is interaction: the tab stays.
        self.preview = None;
        let r = self.collection.requests[self.selected].clone();
        let vars = self
            .environments
            .active
            .and_then(|i| self.environments.items.get(i))
            .map(|e| e.variables.as_slice())
            .unwrap_or(&[]);
        let executable = match post_ronda::environment::resolve(
            &r,
            &self.collection.variables,
            vars,
            &self.collection.headers,
        ) {
            Ok(r) => r,
            Err(e) => {
                self.notify(Err(e));
                return;
            }
        };
        self.history.insert(0, r.clone());
        self.history.truncate(50);
        save_history(&self.project, &self.history);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.running = true;
        self.sent_at = Some(Instant::now());
        self.notice = "Sending request…".into();
        self.error = false;
        let net = http::Network {
            cookie_jar: self
                .prefs
                .keep_cookies
                .then(|| cookie_file(&self.project))
                .inspect(|jar| {
                    if let Some(dir) = jar.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                }),
            proxy: self.prefs.proxy.clone(),
            ca_cert: self.prefs.ca_cert.clone(),
            client_cert: self.prefs.client_cert.clone(),
            client_key: self.prefs.client_key.clone(),
        };
        std::thread::spawn(move || {
            let result = http::execute_with(&executable, &cancel, &net);
            let _ = tx.send(Message::Response(Box::new(r), result));
            ctx.request_repaint();
        });
    }
    fn git(&mut self, action: &str, ctx: &egui::Context) {
        if self.git_running {
            return;
        }
        if (self.dirty.is_some() || !self.project.join("collection.json").exists())
            && let Err(e) = self.write(SaveScope::Structural)
        {
            self.notify(Err(e));
            return;
        }
        self.git_running = true;
        let project = self.project.clone();
        let remote = self.remote.clone();
        let action = action.to_owned();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.notice = format!("Git {action}…");
        std::thread::spawn(move || {
            let result = git_action(&project, &action, &remote);
            let _ = tx.send(Message::Git(result));
            ctx.request_repaint();
        });
    }
    /// Imported JSON bodies arrive as one line; lay them out for reading (key order is kept).
    fn prettify_body(request: &mut Request) {
        let body = request.body.trim();
        if (body.starts_with('{') || body.starts_with('['))
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(body)
            && let Ok(pretty) = serde_json::to_string_pretty(&value)
        {
            request.body = pretty;
        }
    }
    fn paste_curl(&mut self, raw: &str) -> Result<(), String> {
        let mut parsed = curl::parse(raw)?;
        Self::prettify_body(&mut parsed);
        if !self.drafts.contains(&self.selected) {
            parsed.name = self.collection.requests[self.selected].name.clone();
        }
        self.collection.requests[self.selected] = parsed;
        self.response = None;
        self.response_cache.remove(&self.selected);
        // Open where the interesting part is: the body if there is one, otherwise the params.
        self.tab =
            if self.collection.requests[self.selected].body_mode != post_ronda::BodyMode::None {
                1
            } else {
                0
            };
        self.dirty = Some(Instant::now());
        self.notify(Ok("cURL imported".into()));
        Ok(())
    }
    fn new_request(&mut self) {
        self.new_request_in(None);
    }
    fn new_request_in(&mut self, folder: Option<&str>) {
        let number = (1..)
            .find(|n| {
                let name = format!("Untitled {n}");
                !self
                    .collection
                    .requests
                    .iter()
                    .any(|r| r.name.rsplit(" / ").next() == Some(name.as_str()))
            })
            .unwrap_or(1);
        self.collection.requests.push(Request {
            name: match folder.filter(|f| !f.is_empty()) {
                Some(folder) => format!("{folder} / Untitled {number}"),
                None => format!("Untitled {number}"),
            },
            timeout: self.collection.default_timeout,
            follow_redirects: self.collection.follow_redirects,
            insecure: self.collection.insecure,
            ..Default::default()
        });
        self.drafts.insert(self.collection.requests.len() - 1);
        self.activate(self.collection.requests.len() - 1);
        self.focus_url = true;
        self.dirty = Some(Instant::now());
    }
    fn unsaved_changes(&self, index: usize) -> bool {
        self.drafts.contains(&index) || self.request_unsaved(index)
    }
    /// Closes several tabs at once. Unsaved drafts are discarded, which shifts indexes, so the
    /// tab to keep focused (if any) is tracked through the removals.
    fn close_tabs(&mut self, list: &[usize], keep: Option<usize>) {
        self.preview = None;
        let mut drafts: Vec<usize> = list
            .iter()
            .copied()
            .filter(|i| self.drafts.contains(i))
            .collect();
        self.tabs
            .retain(|i| !list.contains(i) || drafts.contains(i));
        drafts.sort_unstable_by(|a, b| b.cmp(a));
        let keep = keep.map(|k| k - drafts.iter().filter(|&&d| d < k).count());
        for d in drafts {
            self.remove_request(d);
        }
        if let Some(k) = keep {
            self.activate(k);
        } else if !self.tabs.contains(&self.selected) {
            match self.tabs.last().copied() {
                Some(next) => self.activate(next),
                None => self.page = 1,
            }
        }
    }
    fn clone_request(&mut self, index: usize) {
        let mut copy = self.collection.requests[index].clone();
        copy.name = format!("{} (copy)", copy.name);
        self.collection.requests.push(copy);
        self.activate(self.collection.requests.len() - 1);
        self.dirty = Some(Instant::now());
        self.notify(Ok("Request cloned".into()));
    }
    fn in_folder(name: &str, folder: &str) -> bool {
        name == folder
            || name
                .strip_prefix(folder)
                .is_some_and(|rest| rest.starts_with(" / "))
    }
    /// Every request and explicit folder under `folder`, as indexes and paths.
    fn folder_members(&self, folder: &str) -> (Vec<usize>, Vec<String>) {
        let requests = (0..self.collection.requests.len())
            .filter(|&i| Self::in_folder(&self.collection.requests[i].name, folder))
            .collect();
        let folders = self
            .collection
            .folders
            .iter()
            .filter(|m| Self::in_folder(&m.path, folder))
            .map(|m| m.path.clone())
            .collect();
        (requests, folders)
    }
    fn add_folder(&mut self, path: String) {
        if !self.collection.folders.iter().any(|m| m.path == path) {
            let end = self.collection.requests.len();
            self.collection.folders.push(FolderMark::new(path, end));
        }
        self.dirty = Some(Instant::now());
    }
    fn clone_folder(&mut self, folder: &str) {
        let parent = folder.rsplit_once(" / ").map(|(p, _)| format!("{p} / "));
        let leaf = folder.rsplit(" / ").next().unwrap_or(folder);
        let mut target = format!("{}{leaf} (copy)", parent.clone().unwrap_or_default());
        let mut n = 2;
        while self.collection.folders.iter().any(|m| m.path == target)
            || self
                .collection
                .requests
                .iter()
                .any(|r| Self::in_folder(&r.name, &target))
        {
            target = format!("{}{leaf} (copy {n})", parent.clone().unwrap_or_default());
            n += 1;
        }
        let (requests, folders) = self.folder_members(folder);
        let swap = |name: &str| format!("{target}{}", &name[folder.len()..]);
        let copies: Vec<Request> = requests
            .iter()
            .filter(|i| !self.drafts.contains(i))
            .map(|&i| {
                let mut r = self.collection.requests[i].clone();
                r.name = swap(&r.name);
                r
            })
            .collect();
        self.collection.requests.extend(copies);
        let end = self.collection.requests.len();
        let copied_folders: Vec<FolderMark> = folders
            .iter()
            .map(|f| FolderMark::new(swap(f), end))
            .collect();
        self.collection.folders.extend(copied_folders);
        self.add_folder(target);
        self.notify(Ok("Folder cloned".into()));
    }
    fn rename_folder(&mut self, folder: &str, leaf: &str) {
        let target = match folder.rsplit_once(" / ") {
            Some((parent, _)) => format!("{parent} / {leaf}"),
            None => leaf.to_owned(),
        };
        if target == folder {
            return;
        }
        let swap = |name: &str| format!("{target}{}", &name[folder.len()..]);
        self.retarget_pins(folder, &target);
        for r in &mut self.collection.requests {
            if Self::in_folder(&r.name, folder) {
                r.name = swap(&r.name);
            }
        }
        for m in &mut self.collection.folders {
            if Self::in_folder(&m.path, folder) {
                m.path = swap(&m.path);
            }
        }
        self.dirty = Some(Instant::now());
    }
    fn delete_folder_now(&mut self, folder: &str) {
        let (mut requests, _) = self.folder_members(folder);
        requests.sort_unstable_by(|a, b| b.cmp(a));
        for i in requests {
            self.remove_request(i);
        }
        self.collection
            .folders
            .retain(|m| !Self::in_folder(&m.path, folder));
        self.collection
            .pinned
            .retain(|p| !Self::in_folder(p, folder));
        self.response = None;
        self.dirty = Some(Instant::now());
    }
    /// Looks for a newer release at the configured address, in the background.
    fn check_updates(&mut self, ctx: &egui::Context, manual: bool) {
        let url = self.prefs.update_url.trim().to_owned();
        if url.is_empty() {
            self.update_state = UpdateState::Failed("Set an update address first".into());
            return;
        }
        if matches!(self.update_state, UpdateState::Checking) {
            return;
        }
        self.update_state = UpdateState::Checking;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Message::Update(manual, post_ronda::update::check(&url)));
            ctx.request_repaint();
        });
    }
    /// Opens a web address (or downloads a file) in the default browser.
    fn open_url(&mut self, url: &str) {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return;
        }
        let mut command = if cfg!(target_os = "macos") {
            post_ronda::process::command("open")
        } else if cfg!(target_os = "windows") {
            post_ronda::process::command("explorer")
        } else {
            post_ronda::process::command("xdg-open")
        };
        command.arg(url);
        if let Err(e) = command.spawn() {
            self.notify(Err(format!("Could not open the browser: {e}")));
        }
    }
    fn reveal_project(&mut self) {
        let file = self.project.join("collection.json");
        let target = if file.exists() {
            file
        } else {
            self.project.clone()
        };
        let mut command = if cfg!(target_os = "macos") {
            let mut c = post_ronda::process::command("open");
            c.arg("-R").arg(&target);
            c
        } else if cfg!(target_os = "windows") {
            let mut c = post_ronda::process::command("explorer");
            c.arg(format!("/select,{}", target.display()));
            c
        } else {
            let mut c = post_ronda::process::command("xdg-open");
            c.arg(&self.project);
            c
        };
        if let Err(e) = command.spawn() {
            self.notify(Err(format!("Could not open the file manager: {e}")));
        }
    }
    /// Folder part of a request name (`"A / B / name"` -> `"A / B"`, root -> `""`).
    fn folder_of(&self, index: usize) -> String {
        self.collection.requests[index]
            .name
            .rsplit_once(" / ")
            .map(|(folder, _)| folder.to_owned())
            .unwrap_or_default()
    }
    /// Rebuilds the request list in `order` (new position -> old index) and remaps everything
    /// that holds an index: tabs, drafts, baselines, the selection and cached responses.
    fn apply_order(&mut self, order: Vec<usize>) {
        self.preview = None;
        let mut map = vec![0; order.len()];
        for (new, &old) in order.iter().enumerate() {
            map[old] = new;
        }
        // Indexes that no longer exist are dropped rather than trusted.
        let remap = |i: usize| map.get(i).copied();
        let old = std::mem::take(&mut self.collection.requests);
        self.collection.requests = order.iter().map(|&o| old[o].clone()).collect();
        self.tabs = self.tabs.iter().filter_map(|&i| remap(i)).collect();
        self.drafts = self.drafts.iter().filter_map(|&i| remap(i)).collect();
        self.baseline = std::mem::take(&mut self.baseline)
            .into_iter()
            .filter_map(|(i, r)| remap(i).map(|n| (n, r)))
            .collect();
        self.response_cache = std::mem::take(&mut self.response_cache)
            .into_iter()
            .filter_map(|(i, r)| remap(i).map(|n| (n, r)))
            .collect();
        let total = order.len();
        for mark in &mut self.collection.folders {
            mark.before = map.get(mark.before).copied().unwrap_or(total);
        }
        self.selected = remap(self.selected).unwrap_or(0);
        self.bulk_target = self
            .bulk_target
            .and_then(|(i, target)| remap(i).map(|n| (n, target)));
        if let NameTarget::Request(i) = self.rename_target {
            self.rename_target = NameTarget::Request(remap(i).unwrap_or(0));
        }
        self.info_index = remap(self.info_index).unwrap_or(0);
        self.dirty = Some(Instant::now());
    }
    /// Where in `order` (indexes still in their old numbering) a drop lands.
    fn drop_position(&self, order: &[usize], target: &DropTarget) -> Option<usize> {
        let end = order.len();
        Some(match target {
            DropTarget::Before(t) => order.iter().position(|i| i == t)?,
            DropTarget::After(t) => order.iter().position(|i| i == t)? + 1,
            DropTarget::Root => end,
            DropTarget::BeforeFolder(folder) => order
                .iter()
                .position(|&i| Self::in_folder(&self.collection.requests[i].name, folder))
                .unwrap_or(end),
            DropTarget::AfterFolder(folder) => order
                .iter()
                .rposition(|&i| Self::in_folder(&self.collection.requests[i].name, folder))
                .map_or(end, |p| p + 1),
            DropTarget::IntoFolder(folder) => order
                .iter()
                .rposition(|&i| Self::in_folder(&self.collection.requests[i].name, folder))
                .map_or(end, |p| p + 1),
        })
    }
    fn target_folder(&self, target: &DropTarget) -> String {
        match target {
            DropTarget::Before(i) | DropTarget::After(i) => self.folder_of(*i),
            DropTarget::IntoFolder(folder) => folder.clone(),
            DropTarget::BeforeFolder(folder) | DropTarget::AfterFolder(folder) => folder
                .rsplit_once(" / ")
                .map(|(parent, _)| parent.to_owned())
                .unwrap_or_default(),
            DropTarget::Root => String::new(),
        }
    }
    fn is_pinned(&self, path: &str) -> bool {
        self.collection.pinned.iter().any(|p| p == path)
    }
    fn toggle_pin(&mut self, path: &str) {
        if self.is_pinned(path) {
            self.collection.pinned.retain(|p| p != path);
        } else {
            self.collection.pinned.push(path.to_owned());
        }
    }
    /// Keeps pins attached when a request or folder is renamed or moved (`old` may be a folder
    /// whose contents are pinned too).
    fn retarget_pins(&mut self, old: &str, new: &str) {
        for pin in &mut self.collection.pinned {
            if Self::in_folder(pin, old) {
                *pin = format!("{new}{}", &pin[old.len()..]);
            }
        }
    }
    fn mark_index(&self, path: &str) -> Option<usize> {
        self.collection.folders.iter().position(|m| m.path == path)
    }
    fn move_request(&mut self, from: usize, target: DropTarget) {
        if matches!(target, DropTarget::Before(t) | DropTarget::After(t) if t == from) {
            return;
        }
        let folder = self.target_folder(&target);
        let name = &self.collection.requests[from].name;
        let leaf = name.rsplit(" / ").next().unwrap_or(name).to_owned();
        let mut order: Vec<usize> = (0..self.collection.requests.len())
            .filter(|&i| i != from)
            .collect();
        // Next to a folder that has nothing in it yet: the folder's own position decides.
        let mut anchor_fix = None;
        let at = match &target {
            DropTarget::BeforeFolder(g) | DropTarget::AfterFolder(g)
                if !order
                    .iter()
                    .any(|&i| Self::in_folder(&self.collection.requests[i].name, g)) =>
            {
                match self.mark_index(g) {
                    Some(k) => {
                        let before = self.collection.folders[k].before;
                        if matches!(target, DropTarget::AfterFolder(_)) {
                            anchor_fix = Some(k);
                        }
                        order
                            .iter()
                            .position(|&i| i == before)
                            .unwrap_or(order.len())
                    }
                    None => order.len(),
                }
            }
            _ => match self.drop_position(&order, &target) {
                Some(at) => at,
                None => return,
            },
        };
        order.insert(at, from);
        // Empty folders stay where they are when the request they preceded is dragged away.
        for (k, mark) in self.collection.folders.iter_mut().enumerate() {
            if mark.before == from && Some(k) != anchor_fix {
                mark.before = from + 1;
            }
        }
        if let Some(k) = anchor_fix {
            // The folder now sits just ahead of the moved request.
            self.collection.folders[k].before = from;
        }
        let old_name = self.collection.requests[from].name.clone();
        let new_name = if folder.is_empty() {
            leaf
        } else {
            format!("{folder} / {leaf}")
        };
        self.retarget_pins(&old_name, &new_name);
        self.collection.requests[from].name = new_name;
        self.apply_order(order);
    }
    fn move_folder(&mut self, folder: &str, target: DropTarget) {
        let into_itself = |g: &str| Self::in_folder(g, folder);
        match &target {
            DropTarget::IntoFolder(g)
            | DropTarget::BeforeFolder(g)
            | DropTarget::AfterFolder(g)
                if into_itself(g) =>
            {
                return;
            }
            DropTarget::Before(i) | DropTarget::After(i)
                if into_itself(&self.collection.requests[*i].name) =>
            {
                return;
            }
            _ => {}
        }
        let (members, _) = self.folder_members(folder);
        let parent = self.target_folder(&target);
        let leaf = folder.rsplit(" / ").next().unwrap_or(folder);
        let new_path = if parent.is_empty() {
            leaf.to_owned()
        } else {
            format!("{parent} / {leaf}")
        };
        let swap = |name: &str| format!("{new_path}{}", &name[folder.len()..]);
        let count = self.collection.requests.len();
        let first_in =
            |g: &str| (0..count).find(|&i| Self::in_folder(&self.collection.requests[i].name, g));
        let last_in =
            |g: &str| (0..count).rfind(|&i| Self::in_folder(&self.collection.requests[i].name, g));
        // Where the folder's marks (explicit/empty folders inside it) go, and, when the target
        // is itself an empty folder, which neighbour they must stay next to in the list.
        let mut neighbour: Option<(String, bool)> = None;
        let anchor = match &target {
            DropTarget::Before(i) => *i,
            DropTarget::After(i) => *i + 1,
            DropTarget::Root => count,
            DropTarget::IntoFolder(g) => match last_in(g) {
                Some(l) => l + 1,
                None => self
                    .mark_index(g)
                    .map_or(count, |k| self.collection.folders[k].before),
            },
            DropTarget::BeforeFolder(g) => match first_in(g) {
                Some(f) => f,
                None => match self.mark_index(g) {
                    Some(k) => {
                        neighbour = Some((g.clone(), false));
                        self.collection.folders[k].before
                    }
                    None => count,
                },
            },
            DropTarget::AfterFolder(g) => match last_in(g) {
                Some(l) => l + 1,
                None => match self.mark_index(g) {
                    Some(k) => {
                        neighbour = Some((g.clone(), true));
                        self.collection.folders[k].before
                    }
                    None => count,
                },
            },
        };
        // Marks inside the folder come along; with requests they sit at the top of the block.
        let (mut moved, mut kept): (Vec<FolderMark>, Vec<FolderMark>) =
            std::mem::take(&mut self.collection.folders)
                .into_iter()
                .partition(|m| Self::in_folder(&m.path, folder));
        for mark in &mut moved {
            mark.path = swap(&mark.path);
            mark.before = members.first().copied().unwrap_or(anchor);
        }
        let at = match &neighbour {
            Some((g, after)) => kept
                .iter()
                .position(|m| m.path == *g)
                .map(|k| if *after { k + 1 } else { k })
                .unwrap_or(kept.len()),
            None => kept.len(),
        };
        for (k, mark) in moved.into_iter().enumerate() {
            kept.insert((at + k).min(kept.len()), mark);
        }
        // Empty folders outside the moved one that preceded one of its requests keep their place.
        for mark in &mut kept {
            while mark.before < count && members.contains(&mark.before) {
                mark.before += 1;
            }
        }
        self.collection.folders = kept;
        if members.is_empty() {
            self.retarget_pins(folder, &new_path);
            self.dirty = Some(Instant::now());
            return;
        }
        let mut order: Vec<usize> = (0..count).filter(|i| !members.contains(i)).collect();
        let Some(at) = self.drop_position(&order, &target) else {
            return;
        };
        for (k, &member) in members.iter().enumerate() {
            order.insert(at + k, member);
        }
        for &member in &members {
            let renamed = swap(&self.collection.requests[member].name);
            self.collection.requests[member].name = renamed;
        }
        self.retarget_pins(folder, &new_path);
        self.apply_order(order);
    }
    fn apply_move(&mut self, dragged: Dragged, target: DropTarget) {
        match dragged {
            Dragged::Request(i) if i < self.collection.requests.len() => {
                self.move_request(i, target)
            }
            Dragged::Folder(path) => self.move_folder(&path, target),
            Dragged::Request(_) => {}
        }
    }
    /// Drops a request and keeps tabs, drafts and the selection pointing at the right rows.
    fn remove_request(&mut self, removed: usize) {
        self.preview = None;
        self.tabs.retain(|&i| i != removed);
        for i in &mut self.tabs {
            if *i > removed {
                *i -= 1;
            }
        }
        self.drafts = self
            .drafts
            .iter()
            .filter(|&&i| i != removed)
            .map(|&i| if i > removed { i - 1 } else { i })
            .collect();
        self.response_cache.clear();
        let removed_name = self.collection.requests[removed].name.clone();
        self.collection.pinned.retain(|p| *p != removed_name);
        self.baseline = std::mem::take(&mut self.baseline)
            .into_iter()
            .filter(|(i, _)| *i != removed)
            .map(|(i, r)| (if i > removed { i - 1 } else { i }, r))
            .collect();
        self.collection.requests.remove(removed);
        for mark in &mut self.collection.folders {
            if mark.before > removed {
                mark.before -= 1;
            }
        }
        if self.collection.requests.is_empty() {
            self.collection.requests.push(Request::default());
            self.drafts.insert(0);
        }
        if self.selected > removed {
            self.selected -= 1;
        }
        self.selected = self.selected.min(self.collection.requests.len() - 1);
        self.dirty = Some(Instant::now());
    }
    /// Save button and ⌘S: a request that was never saved asks for a name and folder first.
    fn save_request(&mut self) {
        if self.drafts.contains(&self.selected) {
            let name = &self.collection.requests[self.selected].name;
            self.save_name = name.rsplit(" / ").next().unwrap_or(name).to_owned();
            self.save_folder = name
                .rsplit_once(" / ")
                .map(|(folder, _)| folder.to_owned())
                .unwrap_or_default();
            self.save_new_folder = None;
            self.save_focus = true;
            self.save_open = true;
        } else {
            let result = self
                .write(SaveScope::Request(self.selected))
                .map(|_| "Saved locally".into());
            self.notify(result);
        }
    }
    fn confirm_save(&mut self) {
        let name = self.save_name.trim().replace(" / ", " - ");
        if name.is_empty() {
            self.notify(Err("Request name is required".into()));
            return;
        }
        self.collection.requests[self.selected].name = if self.save_folder.is_empty() {
            name
        } else {
            format!("{} / {name}", self.save_folder)
        };
        self.drafts.remove(&self.selected);
        let result = self
            .write(SaveScope::Request(self.selected))
            .map(|_| "Saved locally".into());
        self.notify(result);
        self.save_open = false;
    }
    fn response_body(&mut self) {
        self.response_text = self
            .response
            .as_ref()
            .map(|r| {
                if self.pretty {
                    serde_json::from_str::<serde_json::Value>(&r.body)
                        .ok()
                        .and_then(|v| serde_json::to_string_pretty(&v).ok())
                        .unwrap_or_else(|| r.body.clone())
                } else {
                    r.body.clone()
                }
            })
            .unwrap_or_default();
        self.code_lines = style::highlight(&self.response_text, self.palette());
        self.code_line_len = self
            .code_lines
            .iter()
            .map(|j| j.text.chars().count())
            .collect();
        self.code_sel = None;
        self.code_anchor = None;
        self.find_for = String::from("\u{0}");
    }
}
impl eframe::App for App {
    #[cfg(debug_assertions)]
    fn raw_input_hook(&mut self, _: &egui::Context, raw: &mut egui::RawInput) {
        // Development capture only: POST_RONDA_HOVER_FILE=1 pretends a file is dragged over.
        if std::env::var_os("POST_RONDA_HOVER_FILE").is_some() {
            raw.hovered_files.push(egui::HoveredFile::default());
        }
        // Development capture only: POST_RONDA_CLICK="x,y" hovers then clicks that point.
        static FRAME: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        // POST_RONDA_DRAG="x1,y1,x2,y2" presses at the first point, drags to the second and
        // releases there (POST_RONDA_DRAG_HOLD=1 keeps the button down for the screenshot).
        if let Some(spec) = std::env::var_os("POST_RONDA_DRAG") {
            let n: Vec<f32> = spec
                .to_string_lossy()
                .split(',')
                .filter_map(|v| v.parse().ok())
                .collect();
            if let [x1, y1, x2, y2] = n[..] {
                let (a, b) = (egui::pos2(x1, y1), egui::pos2(x2, y2));
                let button = |pos, pressed| egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                };
                let hold = std::env::var_os("POST_RONDA_DRAG_HOLD").is_some();
                match FRAME.fetch_add(1, Ordering::Relaxed) {
                    4 => raw.events.push(egui::Event::PointerMoved(a)),
                    6 => raw.events.push(button(a, true)),
                    7 => raw.events.push(egui::Event::PointerMoved(a.lerp(b, 0.33))),
                    8 => raw.events.push(egui::Event::PointerMoved(a.lerp(b, 0.66))),
                    9..=11 => raw.events.push(egui::Event::PointerMoved(b)),
                    12 if !hold => raw.events.push(button(b, false)),
                    _ => {}
                }
            }
            return;
        }
        let Some(spec) = std::env::var_os("POST_RONDA_CLICK") else {
            return;
        };
        let Some((x, y)) = spec
            .to_string_lossy()
            .split_once(',')
            .and_then(|(x, y)| Some((x.parse::<f32>().ok()?, y.parse::<f32>().ok()?)))
        else {
            return;
        };
        let pos = egui::pos2(x, y);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: if std::env::var_os("POST_RONDA_CLICK_RIGHT").is_some() {
                egui::PointerButton::Secondary
            } else {
                egui::PointerButton::Primary
            },
            pressed,
            modifiers: Default::default(),
        };
        let frame = FRAME.fetch_add(1, Ordering::Relaxed);
        match frame {
            4 => raw.events.push(egui::Event::PointerMoved(pos)),
            6 => raw.events.push(button(true)),
            7 => raw.events.push(button(false)),
            // POST_RONDA_TYPE="abc" types the text in two bursts, as a real keyboard would.
            9 | 11 => {
                if let Some(text) = std::env::var_os("POST_RONDA_TYPE") {
                    let text = text.to_string_lossy();
                    let split = text.chars().next().map_or(0, char::len_utf8);
                    let part = if frame == 9 {
                        &text[..split]
                    } else {
                        &text[split..]
                    };
                    raw.events.push(egui::Event::Text(part.to_owned()));
                }
            }
            _ => {}
        }
    }
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        // Once per run, look for a new release in the background (never while testing or capturing).
        if !self.update_checked {
            self.update_checked = true;
            if self.prefs.auto_update
                && !self.prefs.update_url.trim().is_empty()
                && std::env::var_os("POST_RONDA_STATE_DIR").is_none()
                && std::env::var_os("POST_RONDA_CAPTURE").is_none()
            {
                self.check_updates(ctx, false);
            }
        }
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Message::Response(origin, result) => {
                    self.running = false;
                    self.sent_at = None;
                    if self.collection.requests[self.selected] != *origin {
                        if let Ok(response) = result
                            && let Some(i) = self
                                .collection
                                .requests
                                .iter()
                                .position(|r| r == origin.as_ref())
                        {
                            self.response_cache.insert(i, response);
                        }
                        self.notify(Ok("Request completed in another tab".into()));
                        continue;
                    }
                    match result {
                        Ok(r) => {
                            self.response = Some(r);
                            self.response_body();
                            self.notify(Ok("Response received".into()));
                        }
                        Err(e) => {
                            self.response = None;
                            self.notify(Err(e));
                        }
                    }
                }
                Message::Update(manual, result) => {
                    self.update_state = match result {
                        Ok(release)
                            if post_ronda::update::is_newer(
                                &release.version,
                                env!("CARGO_PKG_VERSION"),
                            ) =>
                        {
                            if !manual {
                                self.notify(Ok(format!(
                                    "Post Ronda {} is available. See App settings > About.",
                                    release.version
                                )));
                            }
                            UpdateState::Available(release)
                        }
                        Ok(_) => UpdateState::UpToDate,
                        // A failed start-up check stays quiet; a failed manual one is shown.
                        Err(e) if manual => UpdateState::Failed(e),
                        Err(_) => UpdateState::Idle,
                    };
                }
                Message::Git(result) => {
                    self.git_running = false;
                    let reload = result.is_ok();
                    self.notify(result);
                    if reload {
                        match std::fs::read_to_string(self.project.join("collection.json"))
                            .map_err(|e| e.to_string())
                            .and_then(|s| collection::import(&s))
                        {
                            Ok(c) => {
                                self.collection = c;
                                self.preview = None;
                                self.tabs.retain(|&i| i < self.collection.requests.len());
                                if let Ok(e) = post_ronda::environment::Environments::load(
                                    &self.project.join("environments.json"),
                                ) {
                                    self.environments = e;
                                }
                                self.response_cache.clear();
                                self.selected = self
                                    .selected
                                    .min(self.collection.requests.len().saturating_sub(1));
                            }
                            Err(e) => self.notify(Err(format!(
                                "Git completed, but collection needs attention: {e}"
                            ))),
                        }
                    }
                }
            }
        }
        #[cfg(debug_assertions)]
        if std::env::var_os("POST_RONDA_CAPTURE").is_some() {
            return;
        }
        if self.dirty.is_none() && self.pending_structural() {
            self.dirty = Some(Instant::now());
        }
        // Folder, name, settings and environment changes are written automatically; edits to
        // request content wait for Save so the tab can show they are unsaved.
        if let Some(since) = self.dirty {
            if since.elapsed() >= Duration::from_millis(400) {
                match self.write(SaveScope::Structural) {
                    Ok(()) => {
                        self.notice = "Saved locally".into();
                        self.error = false;
                    }
                    Err(e) => self.notify(Err(e)),
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(400));
            }
        }
        if ctx.input(|i| i.viewport().close_requested())
            && !self.force_close
            && self.unsaved_count() > 0
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_prompt = true;
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // What each {{name}} stands for right now: the environment wins over the project.
        let mut known: std::collections::HashMap<String, (String, String)> = Default::default();
        let environment = self
            .environments
            .active
            .and_then(|i| self.environments.items.get(i));
        let layers = [
            (
                "Project variable".to_owned(),
                self.collection.variables.as_slice(),
            ),
            (
                environment
                    .map(|e| format!("Environment: {}", e.name))
                    .unwrap_or_default(),
                environment.map(|e| e.variables.as_slice()).unwrap_or(&[]),
            ),
        ];
        for (source, variables) in layers {
            for v in variables.iter().filter(|v| v.enabled && !v.name.is_empty()) {
                known.insert(v.name.clone(), (v.value.clone(), source.clone()));
            }
        }
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("mask-secrets"), !self.show_secret));
        ctx.data_mut(|d| {
            d.insert_temp(
                egui::Id::new(view::KNOWN_VARIABLES),
                std::sync::Arc::new(known),
            )
        });
        #[cfg(debug_assertions)]
        {
            let paste_path = std::env::var_os("POST_RONDA_PASTE_CURL");
            if ctx.cumulative_frame_nr() == 2
                && let Some(path) = &paste_path
                && let Ok(raw) = std::fs::read_to_string(path)
            {
                ctx.input_mut(|i| i.events.push(egui::Event::Paste(raw)));
            }
            if ctx.cumulative_frame_nr() == if paste_path.is_some() { 4 } else { 1 }
                && std::env::var_os("POST_RONDA_DEMO_SEND").is_some()
            {
                self.send(&ctx);
            }
            // Development capture only: POST_RONDA_FIND=text opens the find bar once there is a response.
            if let Some(query) = std::env::var_os("POST_RONDA_FIND")
                && self.response.is_some()
                && !self.find_open
            {
                self.find_open = true;
                self.find_query = query.to_string_lossy().into_owned();
            }
            capture::tick(&ctx, self.response.is_some());
        }
        if self.git_running {
            ui.disable();
        }
        // While the import dialog is open, Cmd/Ctrl+Enter confirms it instead of sending.
        if !self.git_running
            && !self.import_open
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter))
        {
            self.send(&ctx);
        }
        if !self.git_running
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
        {
            self.save_request();
        }
        if !self.git_running
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N))
        {
            self.new_request();
        }
        if !self.git_running
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::I))
        {
            self.import_open = true;
        }
        if !self.git_running
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::E))
        {
            self.settings = false;
            self.page = 2;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
            self.settings_chooser = true;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::K))
            || ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::P))
        {
            self.search_open = true;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
            let over_response = ctx
                .pointer_latest_pos()
                .is_some_and(|p| self.response_rect.contains(p));
            if over_response && self.response.is_some() {
                // With the pointer over the response, find in it.
                self.find_open = true;
                self.find_focus = true;
            } else {
                // Otherwise jump to the filter box above the request tree.
                self.sidebar_visible = true;
                self.focus_filter = true;
            }
        }
        if !self.git_running
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::W))
        {
            self.close_tab(self.selected);
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped.is_empty() {
            self.handle_dropped(dropped);
        }
        if !self.np_open && !self.import_queue.is_empty() {
            let (next, notes) = self.import_queue.remove(0);
            self.begin_new_project(Some(next));
            self.np_notes = notes;
        }
        self.draw_shell(ui);
        self.drop_overlay(&ctx);
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.cancel.store(true, Ordering::Relaxed);
        #[cfg(debug_assertions)]
        if std::env::var_os("POST_RONDA_CAPTURE").is_some() {
            return;
        }
        if (self.dirty.is_some() || self.pending_structural())
            && let Err(e) = self.write(SaveScope::Structural)
        {
            eprintln!("Could not save collection: {e}");
        }
    }
}
/// Where the sent-request history of a project is kept: with the app's own data, next to the
/// other projects' histories, so it never lands in the project's Git repository.
fn history_file(project: &std::path::Path) -> PathBuf {
    state_dir()
        .join("history")
        .join(format!("{}.json", project_key(project)))
}
/// Short stable name for a project folder, used for the app-side files kept per project.
fn project_key(project: &std::path::Path) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in project.to_string_lossy().bytes() {
        hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
/// The cookie jar of a project.
fn cookie_file(project: &std::path::Path) -> PathBuf {
    state_dir()
        .join("cookies")
        .join(format!("{}.txt", project_key(project)))
}
fn load_history(project: &std::path::Path) -> Vec<Request> {
    std::fs::read(history_file(project))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}
fn save_history(project: &std::path::Path, history: &[Request]) {
    let path = history_file(project);
    let Some(dir) = path.parent() else { return };
    let write = || -> Result<(), String> {
        use std::io::Write;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(history).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.persist(&path).map_err(|e| e.to_string())?;
        Ok(())
    };
    if let Err(e) = write() {
        eprintln!("Could not save history: {e}");
    }
}
/// The branch the project's repository is on (or the short commit when detached), if it has one.
fn git_branch(project: &std::path::Path) -> Option<String> {
    let head = std::fs::read_to_string(project.join(".git").join("HEAD")).ok()?;
    let head = head.trim();
    Some(match head.strip_prefix("ref: refs/heads/") {
        Some(branch) => branch.to_owned(),
        None => head.chars().take(7).collect(),
    })
}
/// The `origin` URL of the project's own repository, or empty when it has none.
fn origin_of(project: &std::path::Path) -> String {
    post_ronda::process::command("git")
        .current_dir(project)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default()
}
fn git_action(project: &std::path::Path, action: &str, remote: &str) -> Result<String, String> {
    let run = |args: &[&str]| -> Result<String, String> {
        let out = post_ronda::process::command("git")
            .current_dir(project)
            .env("GIT_TERMINAL_PROMPT", "0")
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
    if action != "init" && !project.join(".git").exists() {
        return Err("Initialize Git in this project first.".into());
    }
    match action {
        "init" => {
            if !project.join(".git").exists() {
                run(&["init", "-b", "main"])?;
            }
            Ok("Git initialized. Configure origin in Settings.".into())
        }
        "remote" => {
            if remote.trim().is_empty() || remote.starts_with('-') {
                return Err("Enter a Git remote URL".into());
            }
            if run(&["remote", "get-url", "origin"]).is_ok() {
                run(&["remote", "set-url", "origin", remote])?;
            } else {
                run(&["remote", "add", "origin", remote])?;
            }
            Ok("Git origin configured".into())
        }
        "pull" => {
            if !run(&["status", "--porcelain"])?.is_empty() {
                return Err("Save and commit local changes before Pull. Use Push to commit and upload, or commit in your terminal.".into());
            }
            run(&["pull", "--ff-only"])?;
            Ok("Pulled latest collection".into())
        }
        "push" => {
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
            if !diff.is_empty() {
                run(&["commit", "-m", "Update API collection"])?;
            }
            run(&["push", "--set-upstream", "origin", "HEAD"])?;
            Ok("Collection pushed".into())
        }
        _ => Err("Unknown Git action".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn git_roundtrip_and_dirty_pull_guard() {
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("origin.git");
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let output = std::process::Command::new("git")
            .args(["init", "--bare", "-b", "main"])
            .arg(&origin)
            .output()
            .unwrap();
        assert!(output.status.success());
        git_action(&project, "init", "").unwrap();
        for (key, val) in [
            ("user.name", "Post Ronda Test"),
            ("user.email", "test@postronda.invalid"),
        ] {
            assert!(
                std::process::Command::new("git")
                    .current_dir(&project)
                    .args(["config", key, val])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(project.join("collection.json"), "{}").unwrap();
        std::fs::write(
            project.join("environments.json"),
            "{\"active\":null,\"items\":[]}",
        )
        .unwrap();
        assert_eq!(origin_of(&project), "");
        git_action(&project, "remote", origin.to_str().unwrap()).unwrap();
        assert_eq!(origin_of(&project), origin.to_str().unwrap());
        git_action(&project, "push", "").unwrap();
        git_action(&project, "pull", "").unwrap();
        std::fs::write(project.join("collection.json"), "{\"changed\":true}").unwrap();
        assert!(
            git_action(&project, "pull", "")
                .unwrap_err()
                .contains("local changes")
        );
        git_action(&project, "push", "").unwrap();
        let verify = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(origin)
            .args(["show", "HEAD:collection.json"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8(verify.stdout).unwrap(),
            "{\"changed\":true}"
        );
    }
}
