use eframe::egui::{self, *};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub enum Appearance {
    Light,
    #[default]
    Dark,
    System,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub appearance: Appearance,
    pub font_size: f32,
    pub line_numbers: bool,
    pub autosave: bool,
    pub save_delay: u64,
    /// Index into `DARK_THEMES` / `LIGHT_THEMES`.
    pub dark_theme: usize,
    pub light_theme: usize,
    /// Request on top and response below, instead of side by side.
    pub stacked_layout: bool,
    /// Wrap long lines of a response to the pane instead of scrolling sideways.
    pub wrap_response: bool,
    /// Download and install new releases by itself. Stored now; it takes effect once the app
    /// has a release feed to read.
    pub auto_update: bool,
    /// Where to look for new releases: a GitHub repository or any JSON feed.
    pub update_url: String,
    /// Keep cookies the servers set and send them back (one jar per project).
    pub keep_cookies: bool,
    pub proxy: String,
    pub ca_cert: String,
    pub client_cert: String,
    pub client_key: String,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            appearance: Appearance::Dark,
            font_size: 13.,
            line_numbers: true,
            autosave: true,
            save_delay: 800,
            dark_theme: 0,
            light_theme: 0,
            stacked_layout: false,
            wrap_response: true,
            auto_update: true,
            update_url: post_ronda::update::DEFAULT_FEED.into(),
            keep_cookies: true,
            proxy: String::new(),
            ca_cert: String::new(),
            client_cert: String::new(),
            client_key: String::new(),
        }
    }
}
impl Preferences {
    /// Palette for the active appearance (`dark` is what the window is currently using).
    pub fn palette(&self, dark: bool) -> Palette {
        Palette::themed(
            dark,
            if dark {
                self.dark_theme
            } else {
                self.light_theme
            },
        )
    }
}
impl Preferences {
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|s| serde_json::from_slice(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        use std::io::Write;
        let parent = path.parent().ok_or("Invalid preferences path")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut f = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        f.write_all(&serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        f.persist(path).map_err(|e| e.to_string())?;
        Ok(())
    }
}
#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub sidebar: Color32,
    pub field: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub gold: Color32,
    pub green: Color32,
    pub blue: Color32,
    pub red: Color32,
}
/// Colours of one theme: bg, sidebar, field, border, text, muted, hover, selected, accent,
/// green, blue, red.
type ThemeColors = [[u8; 3]; 12];
pub const DARK_THEMES: [(&str, ThemeColors); 8] = [
    (
        "Dark",
        [
            [26, 26, 26],
            [26, 26, 26],
            [34, 34, 34],
            [49, 49, 49],
            [204, 204, 204],
            [151, 151, 151],
            [35, 35, 35],
            [39, 39, 39],
            [230, 177, 69],
            [100, 211, 146],
            [117, 186, 235],
            [229, 116, 94],
        ],
    ),
    (
        "Dark Monochrome",
        [
            [24, 24, 24],
            [24, 24, 24],
            [32, 32, 32],
            [54, 54, 54],
            [214, 214, 214],
            [140, 140, 140],
            [34, 34, 34],
            [44, 44, 44],
            [214, 214, 214],
            [176, 176, 176],
            [196, 196, 196],
            [156, 156, 156],
        ],
    ),
    (
        "Dark Pastel",
        [
            [28, 26, 33],
            [28, 26, 33],
            [37, 34, 43],
            [58, 54, 68],
            [218, 210, 226],
            [152, 145, 166],
            [38, 35, 45],
            [46, 43, 55],
            [240, 152, 190],
            [150, 222, 182],
            [150, 192, 242],
            [242, 142, 142],
        ],
    ),
    (
        "Catppuccin Frappé",
        [
            [48, 52, 70],
            [41, 44, 60],
            [65, 69, 89],
            [81, 87, 109],
            [198, 208, 245],
            [165, 173, 206],
            [56, 60, 79],
            [65, 69, 89],
            [202, 158, 230],
            [166, 209, 137],
            [140, 170, 238],
            [231, 130, 132],
        ],
    ),
    (
        "Catppuccin Macchiato",
        [
            [36, 39, 58],
            [30, 32, 48],
            [54, 58, 79],
            [73, 77, 100],
            [202, 211, 245],
            [165, 173, 203],
            [44, 47, 67],
            [54, 58, 79],
            [198, 160, 246],
            [166, 218, 149],
            [138, 173, 244],
            [237, 135, 150],
        ],
    ),
    (
        "Catppuccin Mocha",
        [
            [30, 30, 46],
            [24, 24, 37],
            [49, 50, 68],
            [69, 71, 90],
            [205, 214, 244],
            [166, 173, 200],
            [38, 38, 56],
            [49, 50, 68],
            [203, 166, 247],
            [166, 227, 161],
            [137, 180, 250],
            [243, 139, 168],
        ],
    ),
    (
        "Nord",
        [
            [46, 52, 64],
            [43, 48, 59],
            [59, 66, 82],
            [67, 76, 94],
            [216, 222, 233],
            [154, 163, 181],
            [52, 59, 73],
            [59, 66, 82],
            [235, 203, 139],
            [163, 190, 140],
            [129, 161, 193],
            [191, 97, 106],
        ],
    ),
    (
        "VS Code Dark",
        [
            [30, 30, 30],
            [37, 37, 38],
            [45, 45, 48],
            [60, 60, 60],
            [204, 204, 204],
            [157, 157, 157],
            [42, 45, 46],
            [55, 55, 61],
            [55, 148, 255],
            [115, 201, 145],
            [86, 156, 214],
            [244, 135, 113],
        ],
    ),
];
pub const LIGHT_THEMES: [(&str, ThemeColors); 6] = [
    (
        "Light",
        [
            [255, 255, 255],
            [248, 248, 248],
            [244, 244, 244],
            [228, 228, 228],
            [52, 52, 52],
            [128, 128, 128],
            [241, 241, 241],
            [234, 234, 234],
            [192, 127, 30],
            [38, 139, 79],
            [43, 126, 185],
            [199, 74, 58],
        ],
    ),
    (
        "Light Monochrome",
        [
            [255, 255, 255],
            [247, 247, 247],
            [243, 243, 243],
            [224, 224, 224],
            [40, 40, 40],
            [118, 118, 118],
            [240, 240, 240],
            [232, 232, 232],
            [60, 60, 60],
            [90, 90, 90],
            [70, 70, 70],
            [110, 110, 110],
        ],
    ),
    (
        "Catppuccin Latte",
        [
            [239, 241, 245],
            [230, 233, 239],
            [220, 224, 232],
            [204, 208, 218],
            [76, 79, 105],
            [108, 111, 133],
            [230, 233, 239],
            [220, 224, 232],
            [136, 57, 239],
            [64, 160, 43],
            [30, 102, 245],
            [210, 15, 57],
        ],
    ),
    (
        "Solarized Light",
        [
            [253, 246, 227],
            [238, 232, 213],
            [245, 238, 219],
            [221, 214, 193],
            [88, 110, 117],
            [131, 148, 150],
            [243, 236, 215],
            [238, 232, 213],
            [181, 137, 0],
            [133, 153, 0],
            [38, 139, 210],
            [220, 50, 47],
        ],
    ),
    (
        "GitHub Light",
        [
            [255, 255, 255],
            [246, 248, 250],
            [246, 248, 250],
            [208, 215, 222],
            [36, 41, 47],
            [87, 96, 106],
            [243, 244, 246],
            [234, 238, 242],
            [9, 105, 218],
            [26, 127, 55],
            [9, 105, 218],
            [207, 34, 46],
        ],
    ),
    (
        "Nord Light",
        [
            [236, 239, 244],
            [229, 233, 240],
            [229, 233, 240],
            [216, 222, 233],
            [46, 52, 64],
            [95, 105, 125],
            [229, 233, 240],
            [216, 222, 233],
            [183, 110, 84],
            [100, 145, 85],
            [94, 129, 172],
            [191, 97, 106],
        ],
    ),
];
impl Palette {
    pub fn themed(dark: bool, index: usize) -> Self {
        let (_, c) = if dark {
            DARK_THEMES[index.min(DARK_THEMES.len() - 1)]
        } else {
            LIGHT_THEMES[index.min(LIGHT_THEMES.len() - 1)]
        };
        let rgb = |i: usize| Color32::from_rgb(c[i][0], c[i][1], c[i][2]);
        Self {
            dark,
            bg: rgb(0),
            sidebar: rgb(1),
            field: rgb(2),
            border: rgb(3),
            text: rgb(4),
            muted: rgb(5),
            hover: rgb(6),
            selected: rgb(7),
            gold: rgb(8),
            green: rgb(9),
            blue: rgb(10),
            red: rgb(11),
        }
    }
}
pub fn setup(ctx: &Context, prefs: &Preferences) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        FontData::from_static(include_bytes!("../assets/InterVariable.ttf")).into(),
    );
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "Inter".into());
    ctx.set_fonts(fonts);
    apply(ctx, prefs);
}
pub fn apply(ctx: &Context, prefs: &Preferences) {
    ctx.set_theme(match prefs.appearance {
        Appearance::Light => ThemePreference::Light,
        Appearance::Dark => ThemePreference::Dark,
        Appearance::System => ThemePreference::System,
    });
    for theme in [Theme::Dark, Theme::Light] {
        let p = prefs.palette(theme == Theme::Dark);
        let mut v = if p.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        v.panel_fill = p.bg;
        v.window_fill = p.bg;
        v.extreme_bg_color = p.bg;
        v.faint_bg_color = p.hover;
        v.override_text_color = Some(p.text);
        v.selection.bg_fill = if p.dark {
            Color32::from_rgb(80, 65, 31)
        } else {
            Color32::from_rgb(249, 230, 192)
        };
        v.selection.stroke = Stroke::new(1., p.gold);
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive] {
            w.bg_fill = p.bg;
            w.weak_bg_fill = p.bg;
            w.bg_stroke = Stroke::new(1., p.border);
            w.fg_stroke = Stroke::new(1., p.muted);
            w.corner_radius = CornerRadius::same(5);
        }
        v.widgets.hovered.bg_fill = p.hover;
        v.widgets.hovered.weak_bg_fill = p.hover;
        v.widgets.hovered.bg_stroke = Stroke::new(1., p.border);
        v.widgets.hovered.fg_stroke = Stroke::new(1., p.text);
        v.widgets.active.bg_fill = p.selected;
        v.widgets.active.weak_bg_fill = p.selected;
        v.widgets.active.bg_stroke = Stroke::new(1., p.gold);
        v.widgets.active.fg_stroke = Stroke::new(1., p.text);
        ctx.set_visuals_of(theme, v);
        ctx.style_mut_of(theme, |s| {
            s.text_styles
                .insert(TextStyle::Body, FontId::proportional(prefs.font_size));
            s.text_styles
                .insert(TextStyle::Button, FontId::proportional(prefs.font_size));
            s.text_styles
                .insert(TextStyle::Small, FontId::proportional(11.));
            s.text_styles
                .insert(TextStyle::Monospace, FontId::monospace(prefs.font_size));
            s.spacing.item_spacing = vec2(8., 6.);
            s.spacing.button_padding = vec2(8., 5.);
            s.spacing.interact_size = vec2(28., 28.);
            s.spacing.icon_width = 14.;
            s.spacing.icon_width_inner = 10.;
            s.animation_time = 0.08;
        });
    }
}
pub fn region(
    ui: &mut Ui,
    rect: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    add: impl FnOnce(&mut Ui),
) {
    ui.scope_builder(
        UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            add(ui);
        },
    );
}
pub fn text(ui: &Ui, pos: Pos2, s: &str, size: f32, color: Color32) {
    ui.painter().text(
        pos,
        Align2::LEFT_CENTER,
        s,
        FontId::proportional(size),
        color,
    );
}
pub fn line(ui: &Ui, a: Pos2, b: Pos2, p: Palette) {
    ui.painter().line_segment([a, b], Stroke::new(1., p.border));
}
#[derive(Clone, Copy)]
#[repr(usize)]
#[allow(dead_code)]
pub enum Icon {
    Home,
    Cube,
    Search,
    Plus,
    More,
    Git,
    Pull,
    Push,
    Code,
    Save,
    Shield,
    Eye,
    Trash,
    Chevron,
    Settings,
    Book,
    History,
    Palette,
    Sidebar,
    Split,
    Close,
    Folder,
    Globe,
    Copy,
    Send,
    Pencil,
    Info,
    Dots,
    Pin,
    Sun,
    Moon,
    Desktop,
    Rows,
}
pub fn icon(ui: &Ui, rect: Rect, kind: Icon, color: Color32) {
    icon_sized(ui, rect, kind, color, 16.);
}
pub fn icon_sized(ui: &Ui, rect: Rect, kind: Icon, color: Color32, size: f32) {
    let id = Id::new("tabler-icon-atlas");
    let texture = ui
        .ctx()
        .data(|d| d.get_temp::<TextureHandle>(id))
        .unwrap_or_else(|| {
            let img = image::load_from_memory(include_bytes!("../assets/tabler-atlas.png"))
                .expect("bundled icon atlas")
                .into_rgba8();
            let texture = ui.ctx().load_texture(
                "Tabler icons",
                ColorImage::from_rgba_unmultiplied(
                    [img.width() as usize, img.height() as usize],
                    img.as_raw(),
                ),
                TextureOptions::LINEAR,
            );
            ui.ctx().data_mut(|d| d.insert_temp(id, texture.clone()));
            texture
        });
    let i = kind as usize;
    let x = (i % 8) as f32 * 64. + 8.;
    let y = (i / 8) as f32 * 64. + 8.;
    let uv = Rect::from_min_max(
        pos2(x / 512., y / 320.),
        pos2((x + 48.) / 512., (y + 48.) / 320.),
    );
    ui.painter().image(
        texture.id(),
        Rect::from_center_size(rect.center(), vec2(size, size)),
        uv,
        color,
    );
}
/// Large decorative send icon, drawn from its own 420px bitmap so it stays sharp at 140pt.
pub fn empty_state_icon(ui: &Ui, rect: Rect, color: Color32) {
    let id = Id::new("send-large");
    let texture = ui
        .ctx()
        .data(|d| d.get_temp::<TextureHandle>(id))
        .unwrap_or_else(|| {
            let img = image::load_from_memory(include_bytes!("../assets/send-large.png"))
                .expect("bundled send icon")
                .into_rgba8();
            let texture = ui.ctx().load_texture(
                "Send icon",
                ColorImage::from_rgba_unmultiplied(
                    [img.width() as usize, img.height() as usize],
                    img.as_raw(),
                ),
                TextureOptions::LINEAR,
            );
            ui.ctx().data_mut(|d| d.insert_temp(id, texture.clone()));
            texture
        });
    ui.painter().image(
        texture.id(),
        rect,
        Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)),
        color,
    );
}
pub fn icon_button(ui: &mut Ui, rect: Rect, kind: Icon, tip: &str, p: Palette) -> Response {
    let response = ui
        .interact(rect, ui.id().with(tip), Sense::click())
        .on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
        .on_hover_text(tip);
    if response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    icon(
        ui,
        rect,
        kind,
        if response.hovered() { p.text } else { p.muted },
    );
    response
}
pub fn outlined(ui: &mut Ui, rect: Rect, label: &str, kind: Option<Icon>, p: Palette) -> Response {
    let response = ui
        .interact(rect, ui.id().with(label), Sense::click())
        .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
    ui.painter().rect(
        rect,
        5,
        if response.hovered() { p.hover } else { p.bg },
        Stroke::new(1., p.border),
        StrokeKind::Inside,
    );
    let x = if let Some(kind) = kind {
        icon(
            ui,
            Rect::from_min_size(rect.min + vec2(7., 0.), vec2(20., rect.height())),
            kind,
            p.gold,
        );
        rect.left() + 31.
    } else {
        rect.left() + 10.
    };
    text(ui, pos2(x, rect.center().y), label, 13., p.text);
    response
}
pub fn tab(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    count: usize,
    active: bool,
    p: Palette,
) -> Response {
    let res = ui
        .interact(rect, ui.id().with(label), Sense::click())
        .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
    let color = if active || res.hovered() {
        p.text
    } else {
        p.muted
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.into(), FontId::proportional(13.), color);
    ui.painter().galley(
        pos2(rect.left(), rect.center().y - 8.),
        galley.clone(),
        color,
    );
    if count > 0 {
        text(
            ui,
            pos2(rect.left() + galley.size().x + 3., rect.center().y - 3.),
            &count.to_string(),
            9.,
            color,
        );
    }
    if active {
        ui.painter().line_segment(
            [
                pos2(rect.left(), rect.bottom() - 2.),
                pos2(rect.right() - 10., rect.bottom() - 2.),
            ],
            Stroke::new(2., p.gold),
        );
    }
    res
}
pub fn method_color(method: &str, p: Palette) -> Color32 {
    match method {
        "GET" => p.green,
        "POST" => p.blue,
        "DELETE" => p.red,
        _ => p.gold,
    }
}
/// Syntax colours for generated code; chosen to stay readable on both themes.
struct CodeColors {
    keyword: Color32,
    string: Color32,
    number: Color32,
    function: Color32,
    comment: Color32,
    plain: Color32,
}
impl CodeColors {
    fn new(p: Palette) -> Self {
        if p.dark {
            Self {
                keyword: Color32::from_rgb(224, 108, 170),
                string: Color32::from_rgb(230, 190, 100),
                number: Color32::from_rgb(209, 154, 102),
                function: Color32::from_rgb(117, 186, 235),
                comment: p.muted,
                plain: p.text,
            }
        } else {
            Self {
                keyword: Color32::from_rgb(176, 40, 110),
                string: Color32::from_rgb(150, 98, 0),
                number: Color32::from_rgb(176, 88, 0),
                function: Color32::from_rgb(20, 100, 170),
                comment: p.muted,
                plain: p.text,
            }
        }
    }
}
fn code_keywords(language: &str) -> &'static [&'static str] {
    match language {
        "Shell" => &["curl", "http", "wget"],
        "JavaScript" | "Node.js" => &[
            "const",
            "let",
            "var",
            "await",
            "async",
            "function",
            "return",
            "new",
            "import",
            "from",
            "require",
            "if",
            "else",
            "true",
            "false",
            "null",
            "undefined",
        ],
        "Python" => &[
            "import", "from", "as", "def", "return", "if", "else", "with", "None", "True", "False",
        ],
        "PHP" => &["echo", "true", "false", "null", "new", "function", "return"],
        "Go" => &[
            "package", "import", "func", "var", "const", "return", "if", "else", "nil", "true",
            "false", "defer", "go",
        ],
        "Rust" => &["let", "use", "fn", "async", "await", "mut", "true", "false"],
        "Ruby" => &[
            "require", "def", "end", "if", "else", "nil", "true", "false", "puts",
        ],
        "Swift" => &[
            "import", "var", "let", "try", "await", "true", "false", "nil",
        ],
        "Java" => &[
            "import", "class", "new", "public", "static", "void", "final", "true", "false", "null",
        ],
        "C#" => &["using", "var", "new", "await", "true", "false", "null"],
        "JSON" => &["true", "false", "null"],
        _ => &[],
    }
}
/// Appends `text` to the current line, starting new lines at every `\n`.
fn emit(
    jobs: &mut Vec<egui::text::LayoutJob>,
    job: &mut egui::text::LayoutJob,
    text: &str,
    color: Color32,
) {
    let font = FontId::monospace(12.5);
    for (i, part) in text.split('\n').enumerate() {
        if i > 0 {
            jobs.push(std::mem::take(job));
        }
        if !part.is_empty() {
            job.append(
                part,
                0.,
                egui::TextFormat {
                    font_id: font.clone(),
                    color,
                    ..Default::default()
                },
            );
        }
    }
}
/// One layout job per line of generated code, coloured for the given language.
pub fn highlight_code(code: &str, language: &str, p: Palette) -> Vec<egui::text::LayoutJob> {
    let colors = CodeColors::new(p);
    let mut jobs = Vec::new();
    let mut job = egui::text::LayoutJob::default();
    if language == "HTTP" {
        let mut in_body = false;
        let mut body = String::new();
        for (n, line) in code.split('\n').enumerate() {
            if in_body {
                body.push_str(line);
                body.push('\n');
                continue;
            }
            if n == 0 {
                let (method, rest) = line.split_once(' ').unwrap_or((line, ""));
                emit(&mut jobs, &mut job, method, colors.keyword);
                emit(&mut jobs, &mut job, &format!(" {rest}"), colors.string);
            } else if line.is_empty() {
                in_body = true;
            } else if let Some((name, value)) = line.split_once(':') {
                emit(&mut jobs, &mut job, name, colors.function);
                emit(&mut jobs, &mut job, &format!(":{value}"), colors.plain);
            } else {
                emit(&mut jobs, &mut job, line, colors.plain);
            }
            jobs.push(std::mem::take(&mut job));
        }
        if in_body {
            jobs.extend(highlight_code(body.trim_end_matches('\n'), "JSON", p));
        }
        return jobs;
    }
    let hash_comments = matches!(language, "Shell" | "Python" | "Ruby" | "PowerShell" | "PHP");
    let slash_comments = !matches!(
        language,
        "Shell" | "Python" | "Ruby" | "PowerShell" | "JSON"
    );
    let keywords = code_keywords(language);
    let chars: Vec<char> = code.chars().collect();
    let take = |from: usize, to: usize| chars[from..to].iter().collect::<String>();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev_space = i == 0 || chars[i - 1].is_whitespace();
        let next = chars.get(i + 1).copied().unwrap_or(' ');
        if c == '"' || c == '\'' {
            let raw_quote = language == "Shell" && c == '\'';
            let mut j = i + 1;
            while j < chars.len() {
                if chars[j] == '\\' && !raw_quote {
                    j += 2;
                    continue;
                }
                if chars[j] == c {
                    j += 1;
                    break;
                }
                j += 1;
            }
            let j = j.min(chars.len());
            emit(&mut jobs, &mut job, &take(i, j), colors.string);
            i = j;
        } else if (hash_comments && c == '#' && prev_space)
            || (slash_comments && c == '/' && next == '/' && (i == 0 || chars[i - 1] != ':'))
        {
            let end = chars[i..]
                .iter()
                .position(|&ch| ch == '\n')
                .map_or(chars.len(), |n| i + n);
            emit(&mut jobs, &mut job, &take(i, end), colors.comment);
            i = end;
        } else if language == "Shell"
            && c == '-'
            && prev_space
            && (next == '-' || next.is_alphabetic())
        {
            let end = chars[i..]
                .iter()
                .position(|ch| ch.is_whitespace())
                .map_or(chars.len(), |n| i + n);
            emit(&mut jobs, &mut job, &take(i, end), colors.keyword);
            i = end;
        } else if c == '$' && (next.is_alphabetic() || next == '_') && language != "Shell" {
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            emit(&mut jobs, &mut job, &take(i, j), colors.function);
            i = j;
        } else if c.is_ascii_digit()
            && (i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_'))
        {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                j += 1;
            }
            emit(&mut jobs, &mut job, &take(i, j), colors.number);
            i = j;
        } else if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
                // PowerShell cmdlets such as Invoke-RestMethod contain hyphens.
                if language == "PowerShell"
                    && chars.get(j) == Some(&'-')
                    && chars.get(j + 1).is_some_and(|n| n.is_alphabetic())
                    && chars[i].is_uppercase()
                {
                    j += 1;
                }
            }
            let word = take(i, j);
            let after = chars[j..]
                .iter()
                .find(|ch| !ch.is_whitespace() || **ch == '\n');
            let color = if keywords.contains(&word.as_str()) {
                colors.keyword
            } else if after == Some(&'(') || (language == "PowerShell" && word.contains('-')) {
                colors.function
            } else {
                colors.plain
            };
            emit(&mut jobs, &mut job, &word, color);
            i = j;
        } else {
            emit(&mut jobs, &mut job, &c.to_string(), colors.plain);
            i += 1;
        }
    }
    jobs.push(job);
    jobs
}
pub fn highlight(s: &str, p: Palette) -> Vec<egui::text::LayoutJob> {
    let mut result = Vec::new();
    // Bound rendering work for large bodies; Save/Copy retain the complete response.
    let mut end = s.len().min(1024 * 1024);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    for line in s[..end].lines() {
        let mut job = egui::text::LayoutJob::default();
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let start = i;
            let mut color = p.text;
            if bytes[i] == b'"' {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i = (i + 2).min(bytes.len());
                    } else if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                color = if line[i..].trim_start().starts_with(':') {
                    p.blue
                } else {
                    p.gold
                };
            } else if bytes[i].is_ascii_digit() || bytes[i] == b'-' {
                i += 1;
                while i < bytes.len()
                    && matches!(bytes[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'-' | b'+')
                {
                    i += 1;
                }
                color = p.gold;
            } else if bytes[i].is_ascii_alphabetic() {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                if matches!(&line[start..i], "false" | "true" | "null") {
                    color = p.red;
                }
            } else {
                let ch = line[i..].chars().next().unwrap();
                i += ch.len_utf8();
            }
            job.append(
                &line[start..i],
                0.,
                TextFormat {
                    font_id: FontId::monospace(13.),
                    color,
                    ..Default::default()
                },
            );
        }
        if job.text.is_empty() {
            job.append(" ", 0., TextFormat::default());
        }
        result.push(job);
    }
    if end < s.len() {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            "… Preview limited to 1 MiB. Use Copy or Save for the full response.",
            0.,
            TextFormat {
                color: p.muted,
                ..Default::default()
            },
        );
        result.push(job);
    }
    result
}
pub fn chevron(ui: &Ui, rect: Rect, openness: f32, color: Color32) {
    let id = Id::new("tabler-icon-atlas");
    // Ensure the shared texture exists before retrieving it.
    icon(
        ui,
        Rect::from_min_size(pos2(-1000., -1000.), vec2(0., 0.)),
        Icon::Chevron,
        Color32::TRANSPARENT,
    );
    let Some(texture) = ui.ctx().data(|d| d.get_temp::<TextureHandle>(id)) else {
        return;
    };
    let i = Icon::Chevron as usize;
    let x = (i % 8) as f32 * 64. + 8.;
    let y = (i / 8) as f32 * 64. + 8.;
    let uv = Rect::from_min_max(
        pos2(x / 512., y / 320.),
        pos2((x + 48.) / 512., (y + 48.) / 320.),
    );
    let mut mesh = egui::epaint::Mesh::with_texture(texture.id());
    mesh.add_rect_with_uv(
        Rect::from_center_size(rect.center(), vec2(14., 14.)),
        uv,
        color,
    );
    mesh.rotate(
        egui::emath::Rot2::from_angle(openness * std::f32::consts::FRAC_PI_2),
        rect.center(),
    );
    ui.painter().add(mesh);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("preferences.json");
        let prefs = Preferences {
            appearance: Appearance::Light,
            ..Default::default()
        };
        prefs.save(&path).unwrap();
        assert_eq!(Preferences::load(&path).appearance, Appearance::Light);
    }
    #[test]
    fn code_highlight_colours_tokens_per_language() {
        let p = Palette::themed(true, 0);
        let colors = CodeColors::new(p);
        let color_of = |jobs: &[egui::text::LayoutJob], line: usize, text: &str| {
            jobs[line]
                .sections
                .iter()
                .find(|s| &jobs[line].text[s.byte_range.start.0..s.byte_range.end.0] == text)
                .map(|s| s.format.color)
        };
        let shell = highlight_code(
            "curl --url 'http://a.test' \\\n  -H 'x: 1' # note",
            "Shell",
            p,
        );
        assert_eq!(shell.len(), 2);
        assert_eq!(color_of(&shell, 0, "curl"), Some(colors.keyword));
        assert_eq!(color_of(&shell, 0, "--url"), Some(colors.keyword));
        assert_eq!(color_of(&shell, 0, "'http://a.test'"), Some(colors.string));
        assert_eq!(color_of(&shell, 1, "# note"), Some(colors.comment));
        let py = highlight_code("response = requests.request(\"GET\", 30)", "Python", p);
        assert_eq!(color_of(&py, 0, "request"), Some(colors.function));
        assert_eq!(color_of(&py, 0, "\"GET\""), Some(colors.string));
        assert_eq!(color_of(&py, 0, "30"), Some(colors.number));
        let ps = highlight_code("$h = @{}\nInvoke-RestMethod -Uri 'u'", "PowerShell", p);
        assert_eq!(color_of(&ps, 0, "$h"), Some(colors.function));
        assert_eq!(color_of(&ps, 1, "Invoke-RestMethod"), Some(colors.function));
        let http = highlight_code("POST /a HTTP/1.1\nHost: a.test\n\n{\"k\":true}", "HTTP", p);
        assert_eq!(color_of(&http, 0, "POST"), Some(colors.keyword));
        assert_eq!(color_of(&http, 1, "Host"), Some(colors.function));
        assert_eq!(color_of(&http, 3, "true"), Some(colors.keyword));
        // A string spanning several lines keeps its colour on every line.
        let multi = highlight_code("x = 'a\nb'", "Ruby", p);
        assert_eq!(multi.len(), 2);
        assert_eq!(color_of(&multi, 1, "b'"), Some(colors.string));
    }
    #[test]
    fn unicode_json_highlight() {
        let jobs = highlight(
            "{\"é\":\"ronda 👋\",\"ok\":false}",
            Palette::themed(true, 0),
        );
        assert_eq!(jobs[0].text, "{\"é\":\"ronda 👋\",\"ok\":false}");
    }
}
