use crate::style::{self, Appearance, Icon, Palette, icon_button, line, outlined, region, text};
use crate::*;
use eframe::egui::{
    Align, Align2, FontId, Layout, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};
use post_ronda::environment::{Environment, Variable};
use post_ronda::{BodyMode, RawKind};

impl App {
    pub(super) fn palette(&self) -> Palette {
        self.prefs.palette(self.palette_dark)
    }
    /// Edits are noticed by comparing with the last saved state, so there is nothing to schedule:
    /// request content waits for Save, everything else is picked up by `pending_structural`.
    fn changed(&mut self) {}
    fn store_preferences(&mut self, ctx: &egui::Context) {
        style::apply(ctx, &self.prefs);
        // Response colours are baked into the highlighted lines.
        self.response_body();
        if let Err(e) = self.prefs.save(&state_dir().join("preferences.json")) {
            self.notify(Err(e));
        }
    }
    pub(super) fn activate(&mut self, index: usize) {
        if index >= self.collection.requests.len() {
            return;
        }
        if self.selected != index {
            if let Some(r) = self.response.take() {
                self.response_cache.insert(self.selected, r);
            }
            while self.response_cache.len() > 5 {
                self.response_cache.pop_first();
            }
            self.selected = index;
            self.response = self.response_cache.remove(&index);
            self.response_body();
        }
        if !self.tabs.contains(&index) {
            self.tabs.push(index);
        }
        self.settings = false;
        self.page = 0;
    }
    /// A single click in the sidebar only looks at a request: it takes the preview tab (replacing
    /// the previous preview) instead of piling up tabs. Editing, sending, saving or a double click
    /// keeps the tab.
    pub(super) fn open_preview(&mut self, index: usize) {
        if index >= self.collection.requests.len() {
            return;
        }
        if !self.tabs.contains(&index) {
            let old = self
                .preview
                .filter(|&p| p != index && !self.unsaved_changes(p))
                .and_then(|p| self.tabs.iter().position(|&t| t == p));
            match old {
                Some(position) => self.tabs[position] = index,
                None => self.tabs.push(index),
            }
            self.preview = Some(index);
        }
        self.activate(index);
    }
    pub(super) fn keep_tab(&mut self, index: usize) {
        if self.preview == Some(index) {
            self.preview = None;
        }
    }
    pub(super) fn close_tab(&mut self, index: usize) {
        let was_selected = self.selected == index;
        if self.drafts.contains(&index) {
            // A request that was never saved disappears with its tab.
            self.remove_request(index);
        } else {
            self.tabs.retain(|&i| i != index);
        }
        if was_selected {
            if let Some(&next) = self.tabs.last() {
                self.activate(next);
            } else {
                self.page = 1;
            }
        }
    }
    pub fn draw_shell(&mut self, ui: &mut Ui) {
        let dark = ui.ctx().theme() == egui::Theme::Dark;
        if dark != self.palette_dark {
            self.palette_dark = dark;
            self.response_body();
        }
        let p = self.palette();
        let root = ui.max_rect();
        ui.painter().rect_filled(root, 0, p.bg);
        let top = Rect::from_min_size(root.min, vec2(root.width(), 36.));
        let status = Rect::from_min_max(pos2(root.left(), root.bottom() - 24.), root.max);
        let sidebar = Rect::from_min_max(
            pos2(root.left(), top.bottom()),
            pos2(
                root.left()
                    + if self.sidebar_visible {
                        self.sidebar_width
                    } else {
                        0.
                    },
                status.top(),
            ),
        );
        let main = Rect::from_min_max(
            pos2(sidebar.right(), top.bottom()),
            pos2(root.right(), status.top()),
        );
        region(ui, top, "title-bar", |ui| self.title_bar(ui, top));
        region(ui, status, "status-bar", |ui| self.status_bar(ui, status));
        if self.sidebar_visible {
            region(ui, sidebar, "sidebar", |ui| self.sidebar(ui, sidebar));
            line(ui, sidebar.right_top(), sidebar.right_bottom(), p);
            let drag = ui
                .interact(
                    Rect::from_center_size(sidebar.right_center(), vec2(6., sidebar.height())),
                    ui.id().with("sidebar-resize"),
                    Sense::drag(),
                )
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            if drag.dragged()
                && let Some(pos) = drag.interact_pointer_pos()
            {
                self.sidebar_width = (pos.x - root.left()).clamp(190., 360.);
            }
        }
        region(ui, main, "main", |ui| {
            self.project_bar(ui, Rect::from_min_size(main.min, vec2(main.width(), 44.)));
            let tabs = Rect::from_min_size(main.min + vec2(0., 44.), vec2(main.width(), 36.));
            self.request_tabs(ui, tabs);
            let body = Rect::from_min_max(pos2(main.left(), tabs.bottom()), main.max);
            match self.page {
                1 => region(ui, body, "overview", |ui| self.overview(ui, body)),
                2 => region(ui, body, "environments", |ui| {
                    self.environments_view(ui, body)
                }),
                _ => region(ui, body, "request", |ui| self.request_view(ui, body)),
            }
        });
        let resize = ui
            .interact(
                Rect::from_min_size(root.max - vec2(12., 12.), vec2(12., 12.)),
                ui.id().with("window-resize"),
                Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::ResizeNwSe);
        if resize.drag_started() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::BeginResize(
                    egui::ResizeDirection::SouthEast,
                ));
        }
        self.extra_windows(ui.ctx());
    }
    fn title_bar(&mut self, ui: &mut Ui, r: Rect) {
        let p = self.palette();
        ui.painter().rect_filled(r, 0, p.sidebar);
        line(ui, r.left_bottom(), r.right_bottom(), p);
        let drag = ui.interact(
            r.shrink2(vec2(80., 0.)),
            ui.id().with("window-drag"),
            Sense::click_and_drag(),
        );
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        for (i, color) in [
            Color32::from_rgb(255, 95, 87),
            Color32::from_rgb(254, 188, 46),
            Color32::from_rgb(40, 200, 64),
        ]
        .into_iter()
        .enumerate()
        {
            let c = pos2(r.left() + 15. + 20. * i as f32, r.center().y);
            let button = ui.interact(
                Rect::from_center_size(c, vec2(16., 18.)),
                ui.id().with(("window", i)),
                Sense::click(),
            );
            ui.painter().circle_filled(c, 5.5, color);
            if button.clicked() {
                match i {
                    0 => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                    1 => ui
                        .ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                    _ => {
                        let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                    }
                }
            }
        }
        if icon_button(
            ui,
            Rect::from_min_size(pos2(r.left() + 80., r.top() + 4.), vec2(26., 28.)),
            Icon::Home,
            "Workspace overview",
            p,
        )
        .clicked()
        {
            self.page = 1;
            self.settings = false;
        }
        let switcher_width = 230.;
        region(
            ui,
            Rect::from_min_size(
                pos2(r.left() + 112., r.top() + 4.),
                vec2(switcher_width, 28.),
            ),
            "workspace-menu",
            |ui| {
                let rect = ui.max_rect();
                // The open project's name, with room left for the chevron.
                let label = ui.painter().layout(
                    self.collection.name.clone(),
                    FontId::proportional(13.),
                    p.text,
                    switcher_width - 36.,
                );
                let shown = label.size().x.min(switcher_width - 36.);
                ui.painter().with_clip_rect(rect).galley(
                    pos2(rect.left(), rect.center().y - label.size().y.min(16.) / 2.),
                    label,
                    p.text,
                );
                style::chevron(
                    ui,
                    Rect::from_center_size(
                        pos2(rect.left() + shown + 14., rect.center().y),
                        vec2(14., 14.),
                    ),
                    1.,
                    p.muted,
                );
                let response = ui
                    .interact(
                        Rect::from_min_size(rect.min, vec2(shown + 28., 28.)),
                        ui.id().with("workspace-dropdown"),
                        Sense::click(),
                    )
                    .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
                egui::Popup::menu(&response).show(|ui| {
                    ui.set_width(330.);
                    ui.spacing_mut().item_spacing.y = 1.;
                    ui.add_space(4.);
                    ui.label(RichText::new("PROJECTS").size(11.).color(p.muted));
                    ui.add_space(4.);
                    let search = ui.add(
                        TextEdit::singleline(&mut self.project_filter)
                            .hint_text("Search projects…")
                            .desired_width(f32::INFINITY)
                            .margin(egui::Margin::symmetric(8, 6)),
                    );
                    if ui.memory(|m| m.focused()).is_none() {
                        search.request_focus();
                    }
                    ui.add_space(4.);
                    let filter = self.project_filter.trim().to_lowercase();
                    let mut switch_to = None;
                    let mut remove = None;
                    let shown: Vec<ProjectEntry> = self
                        .projects
                        .iter()
                        .filter(|e| {
                            filter.is_empty()
                                || e.name.to_lowercase().contains(&filter)
                                || e.path
                                    .display()
                                    .to_string()
                                    .to_lowercase()
                                    .contains(&filter)
                        })
                        .cloned()
                        .collect();
                    if shown.is_empty() {
                        ui.add_space(6.);
                        ui.label(RichText::new("No project matches").size(12.).color(p.muted));
                        ui.add_space(6.);
                    }
                    // Five projects fit; the rest scroll.
                    egui::ScrollArea::vertical()
                        .max_height(45. * 5.)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for entry in shown {
                                let active = entry.path == self.project;
                                let (rect, res) = ui.allocate_exact_size(
                                    vec2(ui.available_width(), 44.),
                                    Sense::click(),
                                );
                                let res =
                                    res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
                                let exists = entry.path.join("collection.json").exists();
                                if res.hovered() || active {
                                    ui.painter().rect_filled(
                                        rect,
                                        4,
                                        if active { p.selected } else { p.hover },
                                    );
                                }
                                style::icon(
                                    ui,
                                    Rect::from_center_size(
                                        pos2(rect.left() + 18., rect.center().y),
                                        vec2(16., 16.),
                                    ),
                                    Icon::Folder,
                                    if active { p.gold } else { p.muted },
                                );
                                let clip = ui.painter().with_clip_rect(Rect::from_min_max(
                                    pos2(rect.left() + 36., rect.top()),
                                    pos2(rect.right() - 34., rect.bottom()),
                                ));
                                clip.text(
                                    pos2(rect.left() + 36., rect.center().y - 8.),
                                    Align2::LEFT_CENTER,
                                    &entry.name,
                                    FontId::proportional(13.),
                                    if exists { p.text } else { p.muted },
                                );
                                let home = std::env::var("HOME").unwrap_or_default();
                                let pretty = entry.path.display().to_string();
                                let pretty = match pretty.strip_prefix(&home) {
                                    Some(rest) if !home.is_empty() => format!("~{rest}"),
                                    _ => pretty,
                                };
                                let shown_path = if exists {
                                    pretty
                                } else {
                                    format!("{pretty} (missing)")
                                };
                                clip.text(
                                    pos2(rect.left() + 36., rect.center().y + 9.),
                                    Align2::LEFT_CENTER,
                                    shown_path,
                                    FontId::proportional(11.),
                                    p.muted,
                                );
                                if active {
                                    text(
                                        ui,
                                        pos2(rect.right() - 24., rect.center().y),
                                        "✓",
                                        13.,
                                        p.gold,
                                    );
                                } else if res.hovered() {
                                    let x = Rect::from_center_size(
                                        pos2(rect.right() - 20., rect.center().y),
                                        vec2(22., 22.),
                                    );
                                    if icon_button(
                                        ui,
                                        x,
                                        Icon::Close,
                                        "Remove from the list (files stay on disk)",
                                        p,
                                    )
                                    .clicked()
                                    {
                                        remove = Some(entry.path.clone());
                                    }
                                }
                                if res.clicked() && remove.is_none() {
                                    switch_to = Some(entry.path.clone());
                                }
                            }
                        });
                    menu_gap(ui);
                    let mut chosen = None;
                    for (id, icon, label) in [
                        (0, Icon::Plus, "New project…"),
                        (1, Icon::Folder, "Open project folder…"),
                        (2, Icon::Pull, "Import Postman or OpenAPI…"),
                    ] {
                        if menu_item(ui, icon, label, false, p) {
                            chosen = Some(id);
                        }
                    }
                    menu_gap(ui);
                    if menu_item(ui, Icon::Push, "Export as Postman collection…", false, p) {
                        chosen = Some(4);
                    }
                    if let Some(path) = remove {
                        self.remove_project_entry(&path);
                    }
                    if let Some(path) = switch_to {
                        self.switch_project(path);
                        ui.close();
                    }
                    match chosen {
                        Some(0) => {
                            self.begin_new_project(None);
                            ui.close();
                        }
                        Some(1) => {
                            self.open_project();
                            ui.close();
                        }
                        Some(2) => {
                            self.import_collection_file(false);
                            ui.close();
                        }
                        Some(4) => {
                            self.export_postman();
                            ui.close();
                        }
                        _ => {}
                    }
                });
                if !egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response)) {
                    self.project_filter.clear();
                }
            },
        );
        let name =
            ui.painter()
                .layout_no_wrap("Post Ronda".into(), FontId::proportional(13.), p.text);
        let x = r.center().x - name.size().x / 2.;
        style::icon(
            ui,
            Rect::from_center_size(pos2(x - 16., r.center().y), vec2(18., 18.)),
            Icon::Cube,
            p.gold,
        );
        ui.painter()
            .galley(pos2(x, r.center().y - 8.), name, p.text);
        if icon_button(
            ui,
            Rect::from_min_size(pos2(r.right() - 65., r.top() + 4.), vec2(26., 28.)),
            Icon::Sidebar,
            "Toggle sidebar",
            p,
        )
        .clicked()
        {
            self.sidebar_visible = !self.sidebar_visible;
        }
        if icon_button(
            ui,
            Rect::from_min_size(pos2(r.right() - 35., r.top() + 4.), vec2(26., 28.)),
            if self.prefs.stacked_layout {
                Icon::Rows
            } else {
                Icon::Split
            },
            if self.prefs.stacked_layout {
                "Layout: stacked. Click for side by side"
            } else {
                "Layout: side by side. Click for stacked"
            },
            p,
        )
        .clicked()
        {
            self.prefs.stacked_layout = !self.prefs.stacked_layout;
            self.store_preferences(ui.ctx());
        }
    }
    fn status_bar(&mut self, ui: &mut Ui, r: Rect) {
        let p = self.palette();
        line(ui, r.left_top(), r.right_top(), p);
        let gear = icon_button(
            ui,
            Rect::from_min_size(r.min + vec2(10., 0.), vec2(24., 24.)),
            Icon::Settings,
            "Settings (⌘/Ctrl+,)",
            p,
        );
        // ⌘/Ctrl+, opens the same menu.
        if std::mem::take(&mut self.settings_chooser) {
            egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&gear));
        }
        egui::Popup::menu(&gear).show(|ui| {
            ui.set_width(210.);
            ui.spacing_mut().item_spacing.y = 1.;
            if menu_item(ui, Icon::Settings, "App settings", false, p) {
                self.settings = true;
                ui.close();
            }
            if menu_item(ui, Icon::Folder, "Project settings", false, p) {
                self.open_project_settings(self.collection_tab.min(5));
                ui.close();
            }
        });
        if icon_button(
            ui,
            Rect::from_min_size(r.min + vec2(39., 0.), vec2(24., 24.)),
            Icon::Git,
            "Git (project settings)",
            p,
        )
        .clicked()
        {
            self.open_project_settings(5);
        }
        let theme_button = icon_button(
            ui,
            Rect::from_min_size(r.min + vec2(68., 0.), vec2(24., 24.)),
            Icon::Palette,
            "Theme",
            p,
        );
        // Quick access to the appearance and the themes without opening Settings.
        // The menu stays open while trying themes out; it closes on a click elsewhere.
        egui::Popup::menu(&theme_button)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                let both = self.prefs.appearance == Appearance::System;
                ui.set_width(if both { 430. } else { 210. });
                ui.spacing_mut().item_spacing.y = 1.;
                ui.label(RichText::new("Appearance").size(12.).color(p.muted));
                ui.add_space(6.);
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.;
                    for (mode, icon, tip) in [
                        (Appearance::Light, Icon::Sun, "Light"),
                        (Appearance::Dark, Icon::Moon, "Dark"),
                        (Appearance::System, Icon::Desktop, "System"),
                    ] {
                        let (rect, res) = ui.allocate_exact_size(vec2(32., 30.), Sense::click());
                        let on = self.prefs.appearance == mode;
                        let fill = if on || res.hovered() {
                            p.selected
                        } else {
                            p.field
                        };
                        ui.painter().rect(
                            rect,
                            6,
                            fill,
                            Stroke::new(1., if on { p.gold } else { p.border }),
                            egui::StrokeKind::Inside,
                        );
                        style::icon(
                            ui,
                            Rect::from_center_size(rect.center(), vec2(18., 18.)),
                            icon,
                            if on { p.gold } else { p.muted },
                        );
                        if res
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(tip)
                            .clicked()
                        {
                            self.prefs.appearance = mode;
                            changed = true;
                        }
                    }
                });
                ui.add_space(8.);
                line(
                    ui,
                    ui.cursor().min,
                    ui.cursor().min + vec2(ui.available_width(), 0.),
                    p,
                );
                ui.add_space(8.);
                // Light and Dark show their own list; System needs a choice for each.
                let modes: &[bool] = match self.prefs.appearance {
                    Appearance::Light => &[false],
                    Appearance::Dark => &[true],
                    Appearance::System => &[false, true],
                };
                ui.columns(modes.len(), |columns| {
                    for (column, &dark) in columns.iter_mut().zip(modes) {
                        column.spacing_mut().item_spacing.y = 1.;
                        column.horizontal(|ui| {
                            ui.label(
                                RichText::new(if dark { "Dark theme" } else { "Light theme" })
                                    .size(12.)
                                    .color(p.muted),
                            );
                            if both && p.dark == dark {
                                ui.label(RichText::new("Active").size(11.).color(p.gold));
                            }
                        });
                        column.add_space(4.);
                        let (names, current): (Vec<&str>, usize) = if dark {
                            (
                                style::DARK_THEMES.iter().map(|t| t.0).collect(),
                                self.prefs.dark_theme,
                            )
                        } else {
                            (
                                style::LIGHT_THEMES.iter().map(|t| t.0).collect(),
                                self.prefs.light_theme,
                            )
                        };
                        for (i, name) in names.iter().enumerate() {
                            if menu_choice(column, name, current == i, None, p) {
                                if dark {
                                    self.prefs.dark_theme = i;
                                } else {
                                    self.prefs.light_theme = i;
                                }
                                changed = true;
                            }
                        }
                    }
                });
                if changed {
                    self.store_preferences(ui.ctx());
                }
            });
        if icon_button(
            ui,
            Rect::from_min_size(r.min + vec2(97., 0.), vec2(24., 24.)),
            Icon::History,
            "Request history",
            p,
        )
        .clicked()
        {
            self.history_view = !self.history_view;
        }
        let notice = if self.git_running {
            "Git operation in progress…"
        } else {
            &self.notice
        };
        let notice_rect =
            Rect::from_min_max(r.min + vec2(138., 0.), pos2(r.right() - 190., r.bottom()));
        let color = if self.error { p.red } else { p.muted };
        ui.painter().with_clip_rect(notice_rect).text(
            pos2(notice_rect.left(), r.center().y),
            Align2::LEFT_CENTER,
            notice,
            FontId::proportional(11.),
            color,
        );
        if icon_button(
            ui,
            Rect::from_min_size(pos2(r.right() - 171., r.top()), vec2(24., 24.)),
            Icon::Search,
            "Search requests (⌘/Ctrl+K)",
            p,
        )
        .clicked()
        {
            self.search_open = true;
        }
        let search = ui.interact(
            Rect::from_min_size(pos2(r.right() - 144., r.top()), vec2(45., 24.)),
            ui.id().with("search-label"),
            Sense::click(),
        );
        text(
            ui,
            pos2(r.right() - 144., r.center().y),
            "Search",
            11.,
            p.muted,
        );
        if search.clicked() {
            self.search_open = true;
        }
        text(
            ui,
            pos2(r.right() - 78., r.center().y),
            "v0.2.0",
            11.,
            p.muted,
        );
    }
    fn project_bar(&mut self, ui: &mut Ui, r: Rect) {
        let p = self.palette();
        style::icon(
            ui,
            Rect::from_min_size(r.min + vec2(16., 10.), vec2(24., 24.)),
            Icon::Cube,
            p.text,
        );
        text(
            ui,
            pos2(r.left() + 52., r.center().y),
            &self.collection.name,
            14.,
            p.text,
        );
        let ctx = ui.ctx().clone();
        let env_label = self
            .environments
            .active
            .and_then(|i| self.environments.items.get(i))
            .map(|e| e.name.as_str())
            .unwrap_or("No Environment")
            .to_owned();
        let env_rect = Rect::from_min_size(pos2(r.right() - 190., r.top() + 8.), vec2(174., 27.));
        let env_response = outlined(ui, env_rect, &env_label, Some(Icon::Globe), p);
        style::chevron(
            ui,
            Rect::from_center_size(
                pos2(env_rect.right() - 11., env_rect.center().y),
                vec2(14., 14.),
            ),
            1.,
            p.muted,
        );
        egui::Popup::menu(&env_response).show(|ui| {
            ui.set_width(210.);
            ui.spacing_mut().item_spacing.y = 1.;
            if menu_choice(
                ui,
                "No Environment",
                self.environments.active.is_none(),
                None,
                p,
            ) {
                self.environments.active = None;
                self.changed();
                ui.close();
            }
            for i in 0..self.environments.items.len() {
                let name = self.environments.items[i].name.clone();
                if menu_choice(ui, &name, self.environments.active == Some(i), None, p) {
                    self.environments.active = Some(i);
                    self.changed();
                    ui.close();
                }
            }
            menu_gap(ui);
            if menu_item(ui, Icon::Settings, "Configure environments…", false, p) {
                self.page = 2;
                self.settings = false;
                ui.close();
            }
        });
        // Before a repository exists the button creates one; afterwards it shows the branch.
        let branch = crate::git_branch(&self.project);
        if !self.git_running
            && outlined(
                ui,
                Rect::from_min_size(pos2(r.right() - 402., r.top() + 8.), vec2(112., 27.)),
                &match &branch {
                    Some(b) if b.chars().count() > 12 => {
                        format!("{}…", b.chars().take(11).collect::<String>())
                    }
                    Some(b) => b.clone(),
                    None => "Initialize Git".to_owned(),
                },
                Some(Icon::Git),
                p,
            )
            .clicked()
        {
            if branch.is_some() {
                self.open_project_settings(5);
            } else {
                self.git("init", &ctx);
            }
        }
        if icon_button(
            ui,
            Rect::from_min_size(pos2(r.right() - 283., r.top() + 8.), vec2(25., 27.)),
            Icon::Pull,
            "Pull collection",
            p,
        )
        .clicked()
        {
            self.git("pull", &ctx);
        }
        if icon_button(
            ui,
            Rect::from_min_size(pos2(r.right() - 255., r.top() + 8.), vec2(25., 27.)),
            Icon::Push,
            "Commit and push collection",
            p,
        )
        .clicked()
        {
            self.git("push", &ctx);
        }
        if icon_button(
            ui,
            Rect::from_min_size(pos2(r.right() - 225., r.top() + 8.), vec2(25., 27.)),
            Icon::Shield,
            "Collection settings",
            p,
        )
        .clicked()
        {
            self.open_project_settings(0);
        }
    }
    fn request_tabs(&mut self, ui: &mut Ui, r: Rect) {
        let p = self.palette();
        line(ui, r.left_bottom(), r.right_bottom(), p);
        if let Some(p) = self.preview
            && (!self.tabs.contains(&p)
                || p >= self.collection.requests.len()
                || self.unsaved_changes(p))
        {
            self.preview = None;
        }
        // The active tab is brought into view when it changes or tabs are opened or closed.
        let seen = (self.selected, self.tabs.len());
        let bring_into_view = self.tab_scroll_seen != seen;
        self.tab_scroll_seen = seen;
        let strip = Rect::from_min_max(
            pos2(r.left() + 12., r.top()),
            pos2(
                r.right() - if self.tabs.len() > 1 { 38. } else { 12. },
                r.bottom(),
            ),
        );
        let mut strip_scroll = None;
        region(ui, strip, "tab-strip", |ui| {
            let hovering = ui.rect_contains_pointer(strip);
            strip_scroll = Some(
                egui::ScrollArea::horizontal()
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.;
                            let mut close = None;
                            let mut activate = None;
                            let mut keep = None;
                            let mut tab_action = None;
                            let tab_count = self.tabs.len();
                            for (position, &index) in self.tabs.iter().enumerate() {
                                if index >= self.collection.requests.len() {
                                    continue;
                                }
                                let request = &self.collection.requests[index];
                                let name =
                                    request.name.rsplit(" / ").next().unwrap_or(&request.name);
                                let label_width = ui
                                    .painter()
                                    .layout_no_wrap(name.into(), FontId::proportional(13.), p.text)
                                    .size()
                                    .x;
                                // The method keeps its own width (DELETE, OPTIONS…) so it never runs into the name.
                                let method = if request.method == "DELETE" {
                                    "DEL"
                                } else {
                                    request.method.as_str()
                                };
                                let method_width = ui
                                    .painter()
                                    .layout_no_wrap(
                                        method.into(),
                                        FontId::proportional(11.),
                                        p.text,
                                    )
                                    .size()
                                    .x;
                                let name_left = 9. + method_width + 10.;
                                let width = (label_width + name_left + 34.).clamp(120., 240.);
                                let (rect, res) =
                                    ui.allocate_exact_size(vec2(width, 35.), Sense::click());
                                let res =
                                    res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
                                let active = self.page == 0 && self.selected == index;
                                // `res.hovered()` turns false once the close button sits on top of the
                                // tab, so use the raw pointer position to keep the button visible.
                                let over_tab = ui.rect_contains_pointer(rect);
                                tab_background(ui, rect, active, over_tab, p);
                                if active && bring_into_view {
                                    ui.scroll_to_rect(rect.expand2(vec2(40., 0.)), None);
                                }
                                text(
                                    ui,
                                    pos2(rect.left() + 9., rect.center().y),
                                    method,
                                    11.,
                                    style::method_color(&request.method, p),
                                );
                                let label_rect = Rect::from_min_max(
                                    pos2(rect.left() + name_left, rect.top()),
                                    pos2(rect.right() - 26., rect.bottom()),
                                );
                                let painter = ui.painter().with_clip_rect(label_rect);
                                // A preview tab is shown in italics, like in VS Code.
                                let title = egui::text::LayoutJob::single_section(
                                    name.to_owned(),
                                    egui::TextFormat {
                                        font_id: FontId::proportional(13.),
                                        color: if active { p.text } else { p.muted },
                                        italics: self.preview == Some(index),
                                        ..Default::default()
                                    },
                                );
                                let title = ui.fonts_mut(|f| f.layout_job(title));
                                painter.galley(
                                    pos2(label_rect.left(), rect.center().y - title.size().y / 2.),
                                    title,
                                    p.text,
                                );
                                if res.double_clicked() {
                                    keep = Some(index);
                                }
                                if res.clicked() {
                                    activate = Some(index);
                                }
                                // An unsaved request shows a dot in place of the close button.
                                let unsaved = self.unsaved_changes(index);
                                if unsaved && !over_tab {
                                    ui.painter().circle_filled(
                                        pos2(rect.right() - 14., rect.center().y),
                                        3.5,
                                        Color32::from_rgb(235, 140, 40),
                                    );
                                }
                                if over_tab || (active && !unsaved) {
                                    let x = icon_button(
                                        ui,
                                        Rect::from_min_size(
                                            pos2(rect.right() - 23., rect.top() + 6.),
                                            vec2(20., 23.),
                                        ),
                                        Icon::Close,
                                        &format!("Close tab {index}"),
                                        p,
                                    );
                                    if x.clicked() {
                                        close = Some(index);
                                    }
                                }
                                let enabled = [
                                    true,
                                    true,
                                    self.request_unsaved(index),
                                    true,
                                    tab_count > 1,
                                    position > 0,
                                    position + 1 < tab_count,
                                    true,
                                    true,
                                ];
                                res.context_menu(|ui| {
                                    if let Some(action) = tab_menu_items(ui, enabled, p) {
                                        tab_action = Some((index, position, action));
                                    }
                                });
                            }
                            if let Some(i) = activate {
                                self.activate(i);
                            }
                            if let Some(i) = keep {
                                self.keep_tab(i);
                            }
                            if let Some((index, position, action)) = tab_action {
                                let tabs = self.tabs.clone();
                                match action {
                                    TabMenu::New => self.new_request(),
                                    TabMenu::Clone => self.clone_request(index),
                                    TabMenu::Revert => {
                                        if let Some(mut saved) = self.baseline.get(&index).cloned()
                                        {
                                            saved.name =
                                                self.collection.requests[index].name.clone();
                                            self.collection.requests[index] = saved;
                                            self.dirty = Some(Instant::now());
                                            self.notify(Ok("Changes reverted".into()));
                                        }
                                    }
                                    TabMenu::Close => self.close_tab(index),
                                    TabMenu::CloseOthers => {
                                        let others: Vec<_> =
                                            tabs.iter().copied().filter(|&i| i != index).collect();
                                        self.close_tabs(&others, Some(index));
                                    }
                                    TabMenu::CloseLeft => {
                                        self.close_tabs(&tabs[..position], Some(index))
                                    }
                                    TabMenu::CloseRight => {
                                        self.close_tabs(&tabs[position + 1..], Some(index))
                                    }
                                    TabMenu::CloseSaved => {
                                        let saved: Vec<_> = tabs
                                            .iter()
                                            .copied()
                                            .filter(|&i| !self.unsaved_changes(i))
                                            .collect();
                                        self.close_tabs(&saved, None);
                                    }
                                    TabMenu::CloseAll => self.close_tabs(&tabs, None),
                                }
                            }
                            if let Some(i) = close {
                                self.close_tab(i);
                            }
                            let (rect, _) = ui.allocate_exact_size(vec2(36., 35.), Sense::hover());
                            if icon_button(ui, rect, Icon::Plus, "New request (⌘/Ctrl+N)", p)
                                .clicked()
                            {
                                self.new_request();
                            }
                        });
                    }),
            );
            // A mouse wheel scrolls the strip sideways, like a trackpad swipe does.
            if hovering && let Some(scroll) = strip_scroll.take() {
                let wheel = ui.input(|i| i.smooth_scroll_delta);
                if wheel.y != 0. && wheel.x == 0. {
                    let mut scroll = scroll;
                    let limit = (scroll.content_size.x - scroll.inner_rect.width()).max(0.);
                    scroll.state.offset.x = (scroll.state.offset.x - wheel.y).clamp(0., limit);
                    scroll.state.store(ui.ctx(), scroll.id);
                    ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
                }
            }
        });
        // All open tabs in one list: handy when they no longer fit, and to see what is open.
        if self.tabs.len() > 1 {
            let button = Rect::from_min_size(pos2(r.right() - 32., r.top() + 6.), vec2(24., 24.));
            let res = ui
                .interact(button, ui.id().with("tab-list"), Sense::click())
                .on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
                .on_hover_text("All open tabs");
            if res.hovered() {
                ui.painter().rect_filled(button, 4, p.hover);
            }
            style::chevron(ui, button, 1., if res.hovered() { p.text } else { p.muted });
            let mut chosen = None;
            egui::Popup::menu(&res).show(|ui| {
                ui.set_width(280.);
                ui.spacing_mut().item_spacing.y = 1.;
                egui::ScrollArea::vertical()
                    .max_height(420.)
                    .show(ui, |ui| {
                        for &index in &self.tabs {
                            let Some(request) = self.collection.requests.get(index) else {
                                continue;
                            };
                            let name = request.name.rsplit(" / ").next().unwrap_or(&request.name);
                            let dot = if self.unsaved_changes(index) {
                                "  •"
                            } else {
                                ""
                            };
                            let label = format!("{}  {}{}", request.method, name, dot);
                            let active = self.page == 0 && self.selected == index;
                            if menu_choice(ui, &label, active, None, p) {
                                chosen = Some(index);
                            }
                        }
                    });
                if chosen.is_some() {
                    ui.close();
                }
            });
            if let Some(index) = chosen {
                self.activate(index);
            }
        }
    }
    fn sidebar(&mut self, ui: &mut Ui, r: Rect) {
        let p = self.palette();
        ui.painter().rect_filled(r, 0, p.sidebar);
        let head = Rect::from_min_size(r.min, vec2(r.width(), 29.));
        line(ui, head.left_bottom(), head.right_bottom(), p);
        style::icon(
            ui,
            Rect::from_min_size(head.min + vec2(10., 6.), vec2(18., 18.)),
            Icon::Cube,
            p.muted,
        );
        text(
            ui,
            pos2(r.left() + 36., head.center().y),
            if self.history_view {
                "History"
            } else {
                "Collections"
            },
            12.,
            p.text,
        );
        if icon_button(
            ui,
            Rect::from_min_size(pos2(r.right() - 77., r.top() + 1.), vec2(24., 26.)),
            Icon::Search,
            "Find request",
            p,
        )
        .clicked()
        {
            self.search_open = true;
        }
        if icon_button(
            ui,
            Rect::from_min_size(pos2(r.right() - 51., r.top() + 1.), vec2(24., 26.)),
            Icon::Plus,
            "New request",
            p,
        )
        .clicked()
        {
            self.new_request();
        }
        region(
            ui,
            Rect::from_min_size(pos2(r.right() - 27., r.top() + 2.), vec2(25., 25.)),
            "collection-menu",
            |ui| {
                let response = icon_button(ui, ui.max_rect(), Icon::More, "Collection actions", p);
                egui::Popup::menu(&response).show(|ui| {
                    ui.set_width(220.);
                    ui.spacing_mut().item_spacing.y = 1.;
                    if menu_item(ui, Icon::Plus, "New request", false, p) {
                        self.new_request();
                        ui.close();
                    }
                    if menu_item(ui, Icon::Folder, "New folder", false, p) {
                        self.folder_menu_action(ui.ctx(), String::new(), FolderMenu::NewFolder);
                        ui.close();
                    }
                    if menu_item(ui, Icon::Folder, "Open project…", false, p) {
                        self.open_project();
                        ui.close();
                    }
                    if menu_item(ui, Icon::Code, "Import cURL…", false, p) {
                        self.import_open = true;
                        ui.close();
                    }
                    if menu_item(ui, Icon::Pull, "Import into this project…", false, p) {
                        self.import_collection_file(true);
                        ui.close();
                    }
                    menu_gap(ui);
                    if menu_item(ui, Icon::Pencil, "Rename project…", false, p) {
                        self.folder_menu_action(ui.ctx(), String::new(), FolderMenu::Rename);
                        ui.close();
                    }
                    if menu_item(ui, Icon::Chevron, "Collapse all folders", false, p) {
                        self.folder_menu_action(ui.ctx(), String::new(), FolderMenu::Collapse);
                        ui.close();
                    }
                    if menu_item(ui, Icon::Chevron, "Expand all folders", false, p) {
                        self.folder_menu_action(ui.ctx(), String::new(), FolderMenu::Expand);
                        ui.close();
                    }
                    if menu_item(ui, Icon::Settings, "Project settings", false, p) {
                        self.open_project_settings(0);
                        ui.close();
                    }
                    if menu_item(ui, Icon::Push, "Export collection…", false, p) {
                        self.export_file(false);
                        ui.close();
                    }
                    if menu_item(ui, Icon::Book, "Generate documentation…", false, p) {
                        self.export_file(true);
                        ui.close();
                    }
                });
            },
        );
        let search = Rect::from_min_size(r.min + vec2(10., 37.), vec2(r.width() - 20., 32.));
        ui.painter().rect_filled(search, 5, p.field);
        style::icon(
            ui,
            Rect::from_min_size(search.min + vec2(7., 8.), vec2(17., 17.)),
            Icon::Search,
            p.muted,
        );
        let filter = ui.put(
            Rect::from_min_max(search.min + vec2(30., 6.), search.max - vec2(6., 4.)),
            TextEdit::singleline(&mut self.search)
                .frame(egui::Frame::NONE)
                .hint_text("Search requests…")
                .font(FontId::proportional(12.)),
        );
        if self.focus_filter {
            filter.request_focus();
            self.focus_filter = false;
        }
        let footer = Rect::from_min_max(pos2(r.left(), r.bottom() - 56.), r.max);
        line(ui, footer.left_top(), footer.right_top(), p);
        for (i, label, kind) in [
            (0, "Documentation", Icon::Book),
            (1, "Request History", Icon::History),
        ] {
            let rect =
                Rect::from_min_size(footer.min + vec2(0., i as f32 * 28.), vec2(r.width(), 28.));
            let res = ui
                .interact(rect, ui.id().with(label), Sense::click())
                .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
            if res.hovered() {
                ui.painter().rect_filled(rect, 0, p.hover);
            }
            style::icon(
                ui,
                Rect::from_min_size(rect.min + vec2(10., 6.), vec2(17., 17.)),
                kind,
                p.muted,
            );
            text(
                ui,
                pos2(rect.left() + 36., rect.center().y),
                label,
                12.,
                p.text,
            );
            if res.clicked() {
                if i == 0 {
                    self.open_project_settings(1);
                } else {
                    self.history_view = !self.history_view;
                }
            }
        }
        let tree_rect = Rect::from_min_max(
            r.min + vec2(8., 80.),
            pos2(r.right() - 7., footer.top() - 6.),
        );
        region(ui, tree_rect, "tree", |ui| {
            ui.spacing_mut().item_spacing.y = 0.;
            ui.spacing_mut().indent = 18.;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.history_view {
                        let mut chosen = None;
                        for (i, h) in self.history.iter().enumerate() {
                            let (rect, res) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 26.),
                                Sense::click(),
                            );
                            let method = if h.method == "DELETE" {
                                "DEL"
                            } else {
                                h.method.as_str()
                            };
                            let method_width = ui
                                .painter()
                                .layout_no_wrap(method.into(), FontId::proportional(10.), p.text)
                                .size()
                                .x;
                            text(
                                ui,
                                pos2(rect.left() + 6., rect.center().y),
                                method,
                                10.,
                                style::method_color(&h.method, p),
                            );
                            text(
                                ui,
                                pos2(
                                    rect.left() + (6. + method_width + 8.).max(47.),
                                    rect.center().y,
                                ),
                                h.name.rsplit(" / ").next().unwrap_or(&h.name),
                                12.,
                                p.text,
                            );
                            if res.clicked() {
                                chosen = Some(i);
                            }
                        }
                        if let Some(i) = chosen {
                            self.collection.requests.push(self.history[i].clone());
                            self.activate(self.collection.requests.len() - 1);
                            self.changed();
                        }
                    } else {
                        let query = self.search.to_lowercase();
                        let entries = self.tree_entries(&query);
                        // The project itself is not a folder: its folders and requests sit at the top
                        // level, and empty space below them (or right-click) means "the root".
                        let mut chosen = tree_rows(
                            ui,
                            &entries,
                            0,
                            &self.collection.requests,
                            self.selected,
                            &self.collection.pinned,
                            p,
                        );
                        let (rest, rest_res) = ui.allocate_exact_size(
                            vec2(ui.available_width(), ui.available_height().max(80.)),
                            Sense::click(),
                        );
                        if entries.iter().all(|e| e.0.is_none())
                            && self.collection.folders.is_empty()
                        {
                            ui.painter().text(
                                rest.left_top() + vec2(10., 14.),
                                Align2::LEFT_CENTER,
                                "No requests yet. Right-click for options, or press ⌘N.",
                                FontId::proportional(12.),
                                p.muted,
                            );
                        }
                        if rest_res.dnd_hover_payload::<Dragged>().is_some() {
                            ui.painter().line_segment(
                                [
                                    rest.left_top() + vec2(4., 1.),
                                    rest.right_top() + vec2(-4., 1.),
                                ],
                                Stroke::new(2., p.gold),
                            );
                        }
                        if let Some(payload) = rest_res.dnd_release_payload::<Dragged>() {
                            chosen = Some(RowAction::Move((*payload).clone(), DropTarget::Root));
                        }
                        rest_res.context_menu(|ui| {
                            if let Some(kind) = folder_menu_items(ui, true, false, p) {
                                chosen = Some(RowAction::Folder(String::new(), kind));
                            }
                        });
                        match chosen {
                            Some(RowAction::Open(i)) => self.open_preview(i),
                            Some(RowAction::OpenKept(i)) => {
                                self.activate(i);
                                self.keep_tab(i);
                            }
                            Some(RowAction::Menu(i, kind)) => {
                                self.row_menu_action(ui.ctx(), i, kind)
                            }
                            Some(RowAction::Folder(path, kind)) => {
                                self.folder_menu_action(ui.ctx(), path, kind)
                            }
                            Some(RowAction::Move(dragged, target)) => {
                                self.apply_move(dragged, target)
                            }
                            None => {}
                        }
                    }
                });
        });
    }
    /// Sidebar entries: requests in order, with empty folders slotted in at their saved positions.
    /// A trailing empty path segment keeps an empty folder in the tree. With a search query only
    /// matching requests are listed.
    fn tree_entries(&self, query: &str) -> Vec<(Option<usize>, Vec<&str>)> {
        let mut marks: Vec<&post_ronda::collection::FolderMark> = if query.is_empty() {
            self.collection.folders.iter().collect()
        } else {
            vec![]
        };
        marks.sort_by_key(|m| m.before); // stable: ties keep their saved order
        let mut next = 0;
        let mut entries = vec![];
        for (i, r) in self.collection.requests.iter().enumerate() {
            while next < marks.len() && marks[next].before <= i {
                entries.push(mark_entry(marks[next]));
                next += 1;
            }
            if !self.drafts.contains(&i)
                && format!("{} {}", r.name, r.url)
                    .to_lowercase()
                    .contains(query)
            {
                entries.push((Some(i), r.name.split(" / ").collect()));
            }
        }
        entries.extend(marks[next..].iter().map(|m| mark_entry(m)));
        entries
    }
    fn row_menu_action(&mut self, ctx: &egui::Context, i: usize, kind: RowMenu) {
        match kind {
            RowMenu::Pin => {
                let name = self.collection.requests[i].name.clone();
                self.toggle_pin(&name);
            }
            RowMenu::Clone => self.clone_request(i),
            RowMenu::Copy => {
                ctx.copy_text(curl::export(&self.collection.requests[i]));
                self.notify(Ok("cURL copied to clipboard".into()));
            }
            RowMenu::Rename => {
                let name = &self.collection.requests[i].name;
                self.rename_name = name.rsplit(" / ").next().unwrap_or(name).to_owned();
                self.rename_target = NameTarget::Request(i);
                self.rename_open = true;
            }
            RowMenu::Code => {
                self.activate(i);
                self.codegen_open = true;
            }
            RowMenu::Reveal => self.reveal_project(),
            RowMenu::Info => {
                self.info_index = i;
                self.info_open = true;
            }
            RowMenu::Delete => {
                self.activate(i);
                self.delete_folder = None;
                self.confirm_delete = true;
            }
        }
    }
    fn folder_menu_action(&mut self, ctx: &egui::Context, folder: String, kind: FolderMenu) {
        match kind {
            FolderMenu::Pin => self.toggle_pin(&folder),
            FolderMenu::NewRequest => self.new_request_in(Some(&folder)),
            FolderMenu::NewFolder => {
                self.rename_target = NameTarget::NewFolder(folder);
                self.rename_name.clear();
                self.rename_open = true;
            }
            FolderMenu::Clone => self.clone_folder(&folder),
            FolderMenu::Rename => {
                if folder.is_empty() {
                    self.rename_name = self.collection.name.clone();
                    self.rename_target = NameTarget::Collection;
                } else {
                    self.rename_name = folder.rsplit(" / ").next().unwrap_or(&folder).to_owned();
                    self.rename_target = NameTarget::Folder(folder);
                }
                self.rename_open = true;
            }
            FolderMenu::Collapse => self.set_folders_open(ctx, &folder, false),
            FolderMenu::Expand => self.set_folders_open(ctx, &folder, true),
            FolderMenu::Export => self.export_folder(&folder),
            FolderMenu::Reveal => self.reveal_project(),
            FolderMenu::Settings => self.open_project_settings(0),
            FolderMenu::Delete => {
                self.delete_count = self.folder_members(&folder).0.len();
                self.delete_folder = Some(folder);
                self.confirm_delete = true;
            }
        }
    }
    /// Opens or closes `root` and every folder inside it (all folders when `root` is empty).
    pub(super) fn set_folders_open(&mut self, ctx: &egui::Context, root: &str, open: bool) {
        let mut paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let names = self
            .collection
            .requests
            .iter()
            .map(|r| r.name.as_str())
            .chain(self.collection.folders.iter().map(|m| m.path.as_str()));
        for name in names {
            let parts: Vec<&str> = name.split(" / ").collect();
            // Every prefix is a folder, except the full name of a request.
            let own = self.collection.folders.iter().any(|m| m.path == name);
            let depth = if own { parts.len() } else { parts.len() - 1 };
            for n in 1..=depth {
                paths.insert(parts[..n].join(" / "));
            }
        }
        ctx.data_mut(|d| {
            for path in paths {
                if root.is_empty() || path == root || path.starts_with(&format!("{root} / ")) {
                    d.insert_persisted(egui::Id::new(("folder-open", &path)), open);
                }
            }
        });
        ctx.request_repaint();
    }
    /// One folder, with everything in it, as a collection of its own.
    fn folder_as_collection(&self, folder: &str) -> Collection {
        let (indexes, _) = self.folder_members(folder);
        let prefix = format!("{folder} / ");
        let strip = |name: &str| name.strip_prefix(&prefix).unwrap_or(name).to_owned();
        let leaf = folder.rsplit(" / ").next().unwrap_or(folder).to_owned();
        let requests: Vec<Request> = indexes
            .iter()
            .map(|&i| {
                let mut r = self.collection.requests[i].clone();
                r.name = strip(&r.name);
                r
            })
            .collect();
        let folders = self
            .collection
            .folders
            .iter()
            .filter(|m| m.path.starts_with(&prefix))
            .map(|m| {
                let before = indexes.iter().filter(|&&i| i < m.before).count();
                post_ronda::collection::FolderMark::new(strip(&m.path), before)
            })
            .collect();
        Collection {
            name: leaf,
            requests,
            folders,
            pinned: vec![],
            description: String::new(),
            variables: self.collection.variables.clone(),
            headers: self.collection.headers.clone(),
            default_timeout: self.collection.default_timeout,
            follow_redirects: self.collection.follow_redirects,
            insecure: self.collection.insecure,
        }
    }
    /// Saves one folder as a Postman collection of its own.
    fn export_folder(&mut self, folder: &str) {
        let exported = self.folder_as_collection(folder);
        let leaf = exported.name.clone();
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{leaf}.postman_collection.json"))
            .add_filter("Postman Collection", &["json"])
            .save_file()
        else {
            return;
        };
        let result = serde_json::to_vec_pretty(&post_ronda::collection::export_postman(&exported))
            .map_err(|e| e.to_string())
            .and_then(|bytes| std::fs::write(path, bytes).map_err(|e| e.to_string()));
        self.notify(result.map(|_| format!("Exported “{leaf}” as a Postman collection")));
    }
    /// A small label that follows the pointer while something is dragged in the sidebar.
    fn drag_ghost(&self, ctx: &egui::Context) {
        let Some(payload) = egui::DragAndDrop::payload::<Dragged>(ctx) else {
            return;
        };
        let Some(pos) = ctx.pointer_latest_pos() else {
            return;
        };
        let p = self.palette();
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        let label = match payload.as_ref() {
            Dragged::Request(i) => self.collection.requests.get(*i).map(|r| {
                format!(
                    "{}  {}",
                    r.method,
                    r.name.rsplit(" / ").next().unwrap_or(&r.name)
                )
            }),
            Dragged::Folder(path) => Some(path.rsplit(" / ").next().unwrap_or(path).to_owned()),
        };
        let Some(label) = label else {
            return;
        };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("dnd-ghost"),
        ));
        let galley = painter.layout_no_wrap(label, FontId::proportional(13.), p.text);
        let rect = Rect::from_min_size(pos + vec2(14., 12.), galley.size() + vec2(20., 12.));
        painter.rect(
            rect,
            5,
            p.selected,
            Stroke::new(1., p.gold),
            StrokeKind::Inside,
        );
        painter.galley(rect.min + vec2(10., 6.), galley, p.text);
    }
    fn toast_view(&mut self, ctx: &egui::Context) {
        let p = self.palette();
        let open = self
            .toast
            .as_ref()
            .is_some_and(|t| t.at.elapsed().as_secs_f32() < 2.4);
        let fade = ctx.animate_bool_with_time_and_easing(
            egui::Id::new("toast-fade"),
            open,
            0.18,
            egui::emath::easing::cubic_out,
        );
        if !open && fade <= 0. {
            self.toast = None;
            return;
        }
        let Some(toast) = &self.toast else {
            return;
        };
        if open {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let screen = ctx.content_rect();
        let galley = ctx.fonts_mut(|f| {
            f.layout(
                toast.text.clone(),
                FontId::proportional(13.),
                p.text,
                (screen.width() - 120.).clamp(160., 440.),
            )
        });
        let size = vec2(galley.size().x + 62., (galley.size().y + 22.).max(40.));
        let pos = pos2(
            screen.center().x - size.x / 2.,
            screen.top() + 44. - 10. * (1. - fade),
        );
        egui::Area::new(egui::Id::new("toast"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pos)
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_opacity(fade);
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let painter = ui.painter();
                painter.add(
                    egui::Shadow {
                        offset: [0, 6],
                        blur: 20,
                        spread: 0,
                        color: Color32::from_black_alpha(110),
                    }
                    .as_shape(rect, 8),
                );
                painter.rect(
                    rect,
                    8,
                    if p.dark {
                        Color32::from_gray(46)
                    } else {
                        Color32::WHITE
                    },
                    Stroke::new(1., p.border),
                    StrokeKind::Inside,
                );
                let c = pos2(rect.left() + 26., rect.center().y);
                let color = if toast.error { p.red } else { p.green };
                painter.circle_filled(c, 10., color);
                let mark = Stroke::new(2., Color32::from_gray(24));
                if toast.error {
                    painter.line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], mark);
                    painter.line_segment([c + vec2(3.5, -3.5), c + vec2(-3.5, 3.5)], mark);
                } else {
                    painter.line_segment([c + vec2(-4., 0.5), c + vec2(-1.2, 3.4)], mark);
                    painter.line_segment([c + vec2(-1.2, 3.4), c + vec2(4.2, -3.2)], mark);
                }
                painter.galley(
                    pos2(rect.left() + 46., rect.center().y - galley.size().y / 2.),
                    galley,
                    p.text,
                );
            });
    }
    fn close_dialog(&mut self, ctx: &egui::Context) {
        let p = self.palette();
        let open = self.close_prompt;
        let count = self.unsaved_count().max(1);
        let (mut save, mut discard, mut cancel) = (false, false, false);
        let close = modal(ctx, "unsaved", open, 480., "Unsaved Changes", p, |ui| {
            ui.label(format!(
                "You have unsaved changes in {} request{}. Do you want to save them before closing?",
                count,
                if count == 1 { "" } else { "s" }
            ));
            ui.add_space(14.);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Don't Save").color(p.red))
                            .frame(false)
                            .min_size(vec2(90., 30.)),
                    )
                    .clicked()
                {
                    discard = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Save").color(Color32::BLACK))
                                .fill(p.gold)
                                .min_size(vec2(78., 30.)),
                        )
                        .clicked()
                        || (open && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        save = true;
                    }
                    if ui
                        .add(
                            egui::Button::new("Cancel")
                                .frame(false)
                                .min_size(vec2(72., 30.)),
                        )
                        .clicked()
                    {
                        cancel = true;
                    }
                });
            });
        });
        if !open {
            return;
        }
        if save {
            // Drafts with content are kept under their Untitled names rather than lost.
            let kept: Vec<usize> = self
                .drafts
                .iter()
                .copied()
                .filter(|&i| self.draft_has_content(i))
                .collect();
            for i in kept {
                self.drafts.remove(&i);
            }
            match self.save() {
                Ok(()) => {
                    self.force_close = true;
                    self.close_prompt = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(e) => {
                    self.close_prompt = false;
                    self.notify(Err(e));
                }
            }
        } else if discard {
            self.force_close = true;
            self.close_prompt = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if close || cancel {
            self.close_prompt = false;
        }
    }
    /// New project, or the destination of an imported Postman collection.
    fn project_dialog(&mut self, ctx: &egui::Context) {
        let p = self.palette();
        let open = self.np_open;
        let importing = self.np_import.is_some();
        let mut merge = self.np_merge && importing;
        let current = self.collection.name.clone();
        let mut name = self.np_name.clone();
        let mut parent = self.np_parent.clone();
        let error = self.np_error.clone();
        let (mut confirm, mut cancel) = (false, false);
        let title = if importing {
            "Import Collection"
        } else {
            "New Project"
        };
        let close = modal(ctx, "new-project", open, 560., title, p, |ui| {
            if let Some(c) = &self.np_import {
                ui.label(
                    RichText::new(format!(
                        "“{}” has {} request{}. What should happen to it?",
                        c.name,
                        c.requests.len(),
                        if c.requests.len() == 1 { "" } else { "s" }
                    ))
                    .size(12.5)
                    .color(p.muted),
                );
                if !self.np_notes.is_empty() {
                    ui.add_space(4.);
                    ui.label(
                        RichText::new(format!("Not carried over: {}.", self.np_notes.join("; ")))
                            .size(12.)
                            .color(p.gold),
                    );
                }
                ui.add_space(6.);
                if option_card(
                    ui,
                    !merge,
                    "Import as a new project",
                    "Keeps everything together in a project of its own, in its own folder.",
                    p,
                ) {
                    merge = false;
                }
                if option_card(
                    ui,
                    merge,
                    &format!("Add to “{current}”"),
                    "Only the requests, folders and variables go into the project you have open.",
                    p,
                ) {
                    merge = true;
                }
                ui.add_space(8.);
            }
            if merge {
                // Nothing else to decide: the open project keeps its name and location.
                if open && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    confirm = true;
                }
            } else {
                ui.label(RichText::new("Project name").strong());
                let field = ui.add(
                    TextEdit::singleline(&mut name)
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 7)),
                );
                if open && ui.memory(|m| m.focused()).is_none() {
                    field.request_focus();
                }
                if open && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    confirm = true;
                }
                ui.add_space(8.);
                ui.label(RichText::new("Location").strong());
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new("Browse…").min_size(vec2(80., 28.)))
                        .on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
                        .clicked()
                        && let Some(dir) = rfd::FileDialog::new()
                            .set_title("Where should the project folder go?")
                            .pick_folder()
                    {
                        parent = dir;
                    }
                    let folder = if name.trim().is_empty() {
                        "…".to_owned()
                    } else {
                        name.trim().to_owned()
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{}", parent.join(folder).display()))
                                .color(p.muted),
                        )
                        .truncate(),
                    );
                });
                ui.label(
                RichText::new("Each project lives in its own folder with collection.json and environments.json.")
                    .size(12.)
                    .color(p.muted),
            );
            }
            if !error.is_empty() {
                ui.add_space(4.);
                ui.label(RichText::new(&error).size(12.5).color(p.red));
            }
            ui.add_space(8.);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(if importing { "Import" } else { "Create" })
                                .color(Color32::BLACK),
                        )
                        .fill(p.gold)
                        .min_size(vec2(84., 30.)),
                    )
                    .on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    confirm = true;
                }
                if ui
                    .add(
                        egui::Button::new("Cancel")
                            .frame(false)
                            .min_size(vec2(72., 30.)),
                    )
                    .on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
        self.np_name = name;
        self.np_parent = parent;
        self.np_merge = merge;
        if confirm && open && merge {
            match self.np_import.take() {
                Some(imported) => match self.merge_collection(imported.clone()) {
                    Ok(count) => {
                        self.np_open = false;
                        self.np_error.clear();
                        let into = self.collection.name.clone();
                        self.notify(Ok(format!(
                            "Added {count} request{} to “{into}”",
                            if count == 1 { "" } else { "s" }
                        )));
                    }
                    Err(e) => {
                        self.np_import = Some(imported);
                        self.np_error = e;
                    }
                },
                None => self.np_open = false,
            }
        } else if confirm && open {
            match self.create_project() {
                Ok(()) => {
                    self.np_open = false;
                    self.np_error.clear();
                    self.np_import = None;
                    let created = self.collection.name.clone();
                    self.notify(Ok(format!("Project \"{created}\" is ready")));
                }
                Err(e) => self.np_error = e,
            }
        }
        if (close || cancel) && open {
            self.np_open = false;
            self.np_error.clear();
            self.np_import = None;
        }
    }
    /// Paste a cURL command to turn it into a request (same dialog style as the others).
    fn import_dialog(&mut self, ctx: &egui::Context) {
        let p = self.palette();
        let open = self.import_open;
        let mut text = self.curl_text.clone();
        let error = self.import_error.clone();
        let (mut confirm, mut cancel) = (false, false);
        let close = modal(ctx, "import-curl", open, 680., "Import cURL", p, |ui| {
            ui.label(
                RichText::new(
                    "Paste a cURL command, for example from the browser: Network → Copy as cURL.",
                )
                .size(12.5)
                .color(p.muted),
            );
            ui.add_space(6.);
            egui::Frame::new()
                .fill(p.field)
                .stroke(Stroke::new(1., p.border))
                .corner_radius(6)
                .inner_margin(10.)
                .show(ui, |ui| {
                    let edit = ui.add(
                        TextEdit::multiline(&mut text)
                            .code_editor()
                            .frame(egui::Frame::NONE)
                            .desired_width(f32::INFINITY)
                            .desired_rows(12)
                            .hint_text("curl 'https://api.example.com/users' -H 'Accept: application/json'"),
                    );
                    if open && ui.memory(|m| m.focused()).is_none() {
                        edit.request_focus();
                    }
                });
            if !error.is_empty() {
                ui.add_space(4.);
                ui.label(RichText::new(&error).size(12.5).color(p.red));
            }
            ui.add_space(8.);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Import").color(Color32::BLACK))
                            .fill(p.gold)
                            .min_size(vec2(80., 30.)),
                    )
                    .on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
                    .clicked()
                    || (open
                        && ui.input_mut(|i| {
                            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
                        }))
                {
                    confirm = true;
                }
                if ui
                    .add(
                        egui::Button::new("Cancel")
                            .frame(false)
                            .min_size(vec2(72., 30.)),
                    )
                    .on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
        self.curl_text = text;
        if confirm && open {
            match curl::parse(&self.curl_text) {
                Ok(mut request) => {
                    Self::prettify_body(&mut request);
                    self.tab = if request.body_mode != BodyMode::None {
                        1
                    } else {
                        0
                    };
                    self.collection.requests.push(request);
                    self.selected = self.collection.requests.len() - 1;
                    self.response = None;
                    self.dirty = Some(Instant::now());
                    self.import_open = false;
                    self.import_error.clear();
                    self.curl_text.clear();
                    self.notify(Ok("cURL imported".into()));
                }
                Err(e) => self.import_error = e,
            }
        }
        if (close || cancel) && open {
            self.import_open = false;
            self.import_error.clear();
        }
    }
    fn delete_dialog(&mut self, ctx: &egui::Context) {
        let p = self.palette();
        let open = self.confirm_delete;
        let is_folder = self.delete_folder.is_some();
        if open {
            let name = match &self.delete_folder {
                Some(folder) => folder.clone(),
                None => self.collection.requests[self.selected].name.clone(),
            };
            self.delete_label = name.rsplit(" / ").next().unwrap_or(&name).to_owned();
        }
        let label = self.delete_label.clone();
        let count = self.delete_count;
        let (mut confirm, mut cancel) = (false, false);
        let title = if is_folder {
            "Delete Folder"
        } else {
            "Delete Request"
        };
        let close = modal(ctx, "delete", open, 460., title, p, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.;
                ui.label("Are you sure you want to delete ");
                ui.label(RichText::new(&label).strong());
                if is_folder && count > 0 {
                    ui.label(format!(
                        " and its {count} request{}",
                        if count == 1 { "" } else { "s" }
                    ));
                }
                ui.label("?");
            });
            ui.add_space(14.);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Delete").color(Color32::WHITE))
                            .fill(Color32::from_rgb(196, 70, 46))
                            .min_size(vec2(78., 30.)),
                    )
                    .clicked()
                    || (open && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    confirm = true;
                }
                if ui
                    .add(
                        egui::Button::new("Cancel")
                            .frame(false)
                            .min_size(vec2(72., 30.)),
                    )
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
        if confirm && open {
            match self.delete_folder.clone() {
                Some(folder) => self.delete_folder_now(&folder),
                None => {
                    self.remove_request(self.selected);
                    self.response = None;
                }
            }
            self.confirm_delete = false;
        } else if (close || cancel) && open {
            self.confirm_delete = false;
        }
    }
    fn rename_dialog(&mut self, ctx: &egui::Context) {
        let p = self.palette();
        let open = self.rename_open;
        let target = self.rename_target.clone();
        let (title, label, action) = match &target {
            NameTarget::Request(_) => ("Rename Request", "Request name", "Rename"),
            NameTarget::Folder(_) => ("Rename Folder", "Folder name", "Rename"),
            NameTarget::Collection => ("Rename Collection", "Collection name", "Rename"),
            NameTarget::NewFolder(_) => ("New Folder", "Folder name", "Create"),
        };
        let mut name = self.rename_name.clone();
        let (mut confirm, mut cancel) = (false, false);
        let close = modal(ctx, "rename", open, 440., title, p, |ui| {
            ui.label(RichText::new(label).strong());
            let field = ui.add(
                TextEdit::singleline(&mut name)
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(8, 7)),
            );
            // `lost_focus` reads live focus state, so Enter is checked before focus is re-requested.
            if open && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                confirm = true;
            }
            if open {
                field.request_focus();
            }
            ui.add_space(6.);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new(action).color(Color32::BLACK))
                            .fill(p.gold)
                            .min_size(vec2(80., 30.)),
                    )
                    .clicked()
                {
                    confirm = true;
                }
                if ui
                    .add(egui::Button::new("Cancel").min_size(vec2(80., 30.)))
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
        self.rename_name = name;
        if confirm && open {
            let clean = self.rename_name.trim().replace(" / ", " - ");
            if clean.is_empty() {
                self.notify(Err("A name is required".into()));
            } else {
                match target {
                    NameTarget::Request(index) if index < self.collection.requests.len() => {
                        let old = &self.collection.requests[index].name;
                        let renamed = match old.rsplit_once(" / ") {
                            Some((folder, _)) => format!("{folder} / {clean}"),
                            None => clean,
                        };
                        let old_name = self.collection.requests[index].name.clone();
                        self.retarget_pins(&old_name, &renamed);
                        self.collection.requests[index].name = renamed;
                        self.dirty = Some(Instant::now());
                    }
                    NameTarget::Request(_) => {}
                    NameTarget::Folder(path) => self.rename_folder(&path, &clean),
                    NameTarget::Collection => {
                        self.collection.name = clean;
                        self.dirty = Some(Instant::now());
                    }
                    NameTarget::NewFolder(parent) => {
                        let full = if parent.is_empty() {
                            clean
                        } else {
                            format!("{parent} / {clean}")
                        };
                        self.add_folder(full);
                    }
                }
                self.rename_open = false;
            }
        }
        if (close || cancel) && open {
            self.rename_open = false;
        }
    }
    fn info_dialog(&mut self, ctx: &egui::Context) {
        let p = self.palette();
        let open = self.info_open;
        let Some(request) = self.collection.requests.get(self.info_index).cloned() else {
            return;
        };
        let (folder, name) = match request.name.rsplit_once(" / ") {
            Some((f, n)) => (f.to_owned(), n.to_owned()),
            None => (self.collection.name.clone(), request.name.clone()),
        };
        let rows = [
            ("Name", name),
            ("Method", request.method.clone()),
            ("URL", request.url.clone()),
            ("Folder", folder),
            (
                "Headers",
                request
                    .headers
                    .iter()
                    .filter(|h| h.enabled)
                    .count()
                    .to_string(),
            ),
            ("Timeout", format!("{} s", request.timeout)),
            (
                "Follow redirects",
                if request.follow_redirects {
                    "Yes"
                } else {
                    "No"
                }
                .into(),
            ),
            ("Location", self.project.display().to_string()),
        ];
        let close = modal(ctx, "info", open, 520., "Request Info", p, |ui| {
            egui::Grid::new("request-info")
                .num_columns(2)
                .spacing([20., 10.])
                .show(ui, |ui| {
                    for (label, value) in &rows {
                        ui.label(RichText::new(*label).color(p.muted));
                        ui.add(egui::Label::new(value).wrap().selectable(true));
                        ui.end_row();
                    }
                });
        });
        if close {
            self.info_open = false;
        }
    }
    fn request_view(&mut self, ui: &mut Ui, r: Rect) {
        let p = self.palette();
        if !self.tabs.contains(&self.selected) {
            self.tabs.push(self.selected);
        }
        let url = Rect::from_min_max(r.min + vec2(16., 12.), pos2(r.right() - 90., r.top() + 46.));
        ui.painter()
            .rect(url, 5, p.bg, Stroke::new(1., p.border), StrokeKind::Inside);
        let method_rect = Rect::from_min_size(url.min + vec2(8., 3.), vec2(63., 28.));
        let method = self.collection.requests[self.selected].method.clone();
        text(
            ui,
            pos2(method_rect.left() + 3., method_rect.center().y),
            &method,
            12.,
            style::method_color(&method, p),
        );
        let method_response = ui
            .interact(method_rect, ui.id().with("method-picker"), Sense::click())
            .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
        egui::Popup::menu(&method_response).show(|ui| {
            ui.set_width(130.);
            ui.spacing_mut().item_spacing.y = 1.;
            for m in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
                let current = self.collection.requests[self.selected].method == m;
                if menu_choice(ui, m, current, Some(style::method_color(m, p)), p) {
                    self.collection.requests[self.selected].method = m.into();
                    self.changed();
                    ui.close();
                }
            }
        });
        line(
            ui,
            pos2(url.left() + 74., url.top() + 8.),
            pos2(url.left() + 74., url.bottom() - 8.),
            p,
        );
        let url_id = ui.id().with("url-input");
        let focused = ui.ctx().memory(|m| m.focused());
        let url_focused = focused == Some(url_id);
        let incoming = if url_focused || focused.is_none() {
            ui.ctx().input(|i| {
                i.events.iter().find_map(|event| {
                    if let egui::Event::Paste(raw) = event
                        && (raw.trim_start().starts_with("curl ")
                            || raw.trim_start().starts_with("curl.exe "))
                    {
                        Some(raw.clone())
                    } else {
                        None
                    }
                })
            })
        } else {
            None
        };
        if let Some(raw) = incoming {
            ui.ctx()
                .input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Paste(_))));
            if !url_focused && !self.collection.requests[self.selected].url.is_empty() {
                self.new_request();
            }
            match self.paste_curl(&raw) {
                Ok(()) => self.url_caret_to_end(ui.ctx(), url_id),
                Err(e) => self.notify(Err(e)),
            }
        }
        let url_rect = Rect::from_min_max(url.min + vec2(81., 8.), url.max - vec2(66., 5.));
        // Typing `{{` offers the variables that exist; the arrow keys, Enter and Tab pick one.
        let mut accepted = None;
        if let Some(completion) = self.variable_completion(ui.ctx(), url_id) {
            let count = completion.names.len();
            self.var_sel = self.var_sel.min(count - 1);
            ui.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    self.var_sel = (self.var_sel + 1) % count;
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    self.var_sel = (self.var_sel + count - 1) % count;
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
                {
                    accepted = Some(completion.names[self.var_sel].clone());
                }
            });
            if let Some(name) = accepted.take() {
                self.accept_variable(ui.ctx(), url_id, &completion, &name);
            }
        } else {
            self.var_sel = 0;
        }
        let mut url_layouter = variable_layouter(p, FontId::proportional(13.));
        let url_output = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(url_rect)
                    .layout(Layout::centered_and_justified(egui::Direction::TopDown)),
                |ui| {
                    TextEdit::singleline(&mut self.collection.requests[self.selected].url)
                        .id(url_id)
                        .frame(egui::Frame::NONE)
                        .hint_text("Enter URL or paste a cURL request")
                        .layouter(&mut url_layouter)
                        .font(FontId::proportional(13.))
                        .show(ui)
                },
            )
            .inner;
        let url_response = url_output.response.clone();
        variable_tooltip(ui, &url_output, p);
        if let Some(completion) = self.variable_completion(ui.ctx(), url_id) {
            let known: Variables = ui
                .ctx()
                .data(|d| d.get_temp(egui::Id::new(KNOWN_VARIABLES)))
                .unwrap_or_default();
            let mut picked = None;
            egui::Area::new(egui::Id::new("variable-suggestions"))
                .order(egui::Order::Foreground)
                .fixed_pos(url_rect.left_bottom() + vec2(-6., 10.))
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(p.bg)
                        .stroke(Stroke::new(1., p.border))
                        .corner_radius(6)
                        .inner_margin(4.)
                        .shadow(egui::Shadow {
                            offset: [0, 6],
                            blur: 18,
                            spread: 0,
                            color: Color32::from_black_alpha(120),
                        })
                        .show(ui, |ui| {
                            ui.set_width(320.);
                            ui.spacing_mut().item_spacing.y = 1.;
                            for (i, name) in completion.names.iter().enumerate().take(8) {
                                let (rect, res) = ui.allocate_exact_size(
                                    vec2(ui.available_width(), 26.),
                                    Sense::click(),
                                );
                                if i == self.var_sel || res.hovered() {
                                    ui.painter().rect_filled(rect, 4, p.selected);
                                }
                                text(
                                    ui,
                                    pos2(rect.left() + 10., rect.center().y),
                                    name,
                                    13.,
                                    p.blue,
                                );
                                if let Some((value, _)) = known.get(name) {
                                    let shown = if looks_secret(name) {
                                        "••••••••".to_owned()
                                    } else {
                                        value.clone()
                                    };
                                    let clip = ui.painter().with_clip_rect(Rect::from_min_max(
                                        pos2(rect.center().x - 20., rect.top()),
                                        rect.max,
                                    ));
                                    clip.text(
                                        pos2(rect.right() - 8., rect.center().y),
                                        Align2::RIGHT_CENTER,
                                        shown,
                                        FontId::proportional(11.5),
                                        p.muted,
                                    );
                                }
                                if res.clicked() {
                                    picked = Some(name.clone());
                                }
                            }
                        });
                });
            if let Some(name) = picked {
                self.accept_variable(ui.ctx(), url_id, &completion, &name);
            }
        }
        if self.focus_url {
            self.url_caret_to_end(ui.ctx(), url_id);
            url_response.request_focus();
            self.focus_url = false;
        }
        if url_response.changed() {
            let raw = self.collection.requests[self.selected].url.clone();
            if raw.trim_start().starts_with("curl ") || raw.trim_start().starts_with("curl.exe ") {
                match self.paste_curl(&raw) {
                    Ok(()) => self.url_caret_to_end(ui.ctx(), url_id),
                    Err(e) => self.notify(Err(e)),
                }
            }
            self.changed();
        }
        if icon_button(
            ui,
            Rect::from_min_size(pos2(url.right() - 61., url.top() + 3.), vec2(27., 28.)),
            Icon::Code,
            "Generate Code",
            p,
        )
        .clicked()
        {
            self.codegen_open = true;
        }
        if icon_button(
            ui,
            Rect::from_min_size(pos2(url.right() - 30., url.top() + 3.), vec2(27., 28.)),
            Icon::Save,
            "Save request (⌘/Ctrl+S)",
            p,
        )
        .clicked()
        {
            self.save_request();
        }
        if self.unsaved_changes(self.selected) {
            style::icon(
                ui,
                Rect::from_min_size(pos2(url.right() - 30., url.top() + 3.), vec2(27., 28.)),
                Icon::Save,
                Color32::from_rgb(235, 140, 40),
            );
        }
        let send = Rect::from_min_size(pos2(r.right() - 82., r.top() + 12.), vec2(66., 34.));
        let res = ui
            .interact(send, ui.id().with("send"), Sense::click())
            .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
        if self.running {
            ui.painter().rect(
                send,
                5,
                if res.hovered() { p.hover } else { p.bg },
                Stroke::new(1.5, p.gold),
                StrokeKind::Inside,
            );
        } else {
            ui.painter().rect_filled(
                send,
                5,
                if res.hovered() {
                    Color32::from_rgb(239, 189, 85)
                } else {
                    Color32::from_rgb(226, 170, 54)
                },
            );
        }
        ui.painter().text(
            send.center(),
            Align2::CENTER_CENTER,
            if self.running { "Cancel" } else { "Send" },
            FontId::proportional(12.),
            if self.running { p.gold } else { Color32::BLACK },
        );
        if res.clicked() {
            if self.running {
                self.cancel.store(true, Ordering::Relaxed);
            } else {
                self.send(ui.ctx());
            }
        }
        let content = Rect::from_min_max(r.min + vec2(16., 62.), r.max - vec2(16., 16.));
        let (left, right) = if self.prefs.stacked_layout {
            let split = content.top() + content.height() * self.split_ratio_v;
            line(
                ui,
                pos2(content.left(), split),
                pos2(content.right(), split),
                p,
            );
            let drag = ui
                .interact(
                    Rect::from_center_size(
                        pos2(content.center().x, split),
                        vec2(content.width(), 8.),
                    ),
                    ui.id().with("editor-divider-stacked"),
                    Sense::click_and_drag(),
                )
                .on_hover_cursor(egui::CursorIcon::ResizeVertical);
            if drag.dragged()
                && let Some(pos) = drag.interact_pointer_pos()
            {
                self.split_ratio_v = ((pos.y - content.top()) / content.height()).clamp(0.2, 0.8);
            }
            if drag.double_clicked() {
                self.split_ratio_v = 0.5;
            }
            (
                Rect::from_min_max(content.min, pos2(content.right(), split - 8.)),
                Rect::from_min_max(pos2(content.left(), split + 12.), content.max),
            )
        } else {
            let split = content.left() + content.width() * self.split_ratio;
            line(
                ui,
                pos2(split, content.top()),
                pos2(split, content.bottom()),
                p,
            );
            let drag = ui
                .interact(
                    Rect::from_center_size(
                        pos2(split, content.center().y),
                        vec2(8., content.height()),
                    ),
                    ui.id().with("editor-divider"),
                    Sense::click_and_drag(),
                )
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            if drag.dragged()
                && let Some(pos) = drag.interact_pointer_pos()
            {
                self.split_ratio = ((pos.x - content.left()) / content.width()).clamp(0.32, 0.68);
            }
            if drag.double_clicked() {
                self.split_ratio = 0.48;
            }
            (
                Rect::from_min_max(content.min, pos2(split - 16., content.bottom())),
                Rect::from_min_max(pos2(split + 20., content.top()), content.max),
            )
        };
        region(ui, left, "request-editor", |ui| {
            self.request_editor(ui, left)
        });
        region(ui, right, "response-editor", |ui| {
            self.response_view(ui, right)
        });
    }

    /// Shown while files are dragged over the window: says what dropping them will do.
    pub(super) fn drop_overlay(&self, ctx: &egui::Context) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let p = self.palette();
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("drop-overlay"),
        ));
        painter.rect_filled(screen, 0, Color32::from_rgba_unmultiplied(0, 0, 0, 150));
        let card = Rect::from_center_size(screen.center(), vec2(420., 150.));
        painter.rect(
            card,
            12,
            p.sidebar,
            Stroke::new(1.5, p.gold),
            egui::StrokeKind::Inside,
        );
        painter.text(
            card.center() - vec2(0., 20.),
            Align2::CENTER_CENTER,
            "Drop to import",
            FontId::proportional(18.),
            p.text,
        );
        painter.text(
            card.center() + vec2(0., 14.),
            Align2::CENTER_CENTER,
            "Postman collection, OpenAPI file, environment, or a project folder",
            FontId::proportional(12.),
            p.muted,
        );
    }
    /// Applies this frame's pointer activity to the response selection.
    fn update_code_selection(&mut self, pointer: &CodePointer) {
        if pointer.pressed.is_some() {
            self.code_sel = None;
            self.code_anchor = None;
            self.code_selecting = true;
        }
        if !self.code_selecting {
            return;
        }
        if let Some(target) = pointer.target() {
            match pointer.pressed {
                Some(kind @ (2 | 3)) => {
                    let text = &self.code_lines[target.0].text;
                    let (start, end) = if kind == 2 {
                        word_range(text, target.1)
                    } else {
                        (0, self.code_line_len[target.0])
                    };
                    self.code_sel = Some(((target.0, start), (target.0, end)));
                    self.code_selecting = false;
                }
                _ => {
                    let anchor = *self.code_anchor.get_or_insert(target);
                    self.code_sel = (anchor != target).then_some((anchor, target));
                }
            }
        }
        if !pointer.down {
            self.code_selecting = false;
        }
    }
    /// The selected response text, if any.
    fn selected_code(&self) -> Option<String> {
        let (a, b) = self.code_sel?;
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let mut out = String::new();
        for line in start.0..=end.0.min(self.code_lines.len().saturating_sub(1)) {
            let text = &self.code_lines[line].text;
            let from = if line == start.0 { start.1 } else { 0 };
            let to = if line == end.0 { end.1 } else { usize::MAX };
            if line > start.0 {
                out.push('\n');
            }
            out.extend(text.chars().skip(from).take(to.saturating_sub(from)));
        }
        (!out.is_empty()).then_some(out)
    }
    /// Response body with long lines wrapped to the pane, so everything stays visible and
    /// selectable without scrolling sideways. Only the lines in view are laid out.
    fn wrapped_body(&mut self, ui: &mut Ui, p: Palette) {
        let font = FontId::monospace(13.);
        let (char_width, row_height) =
            ui.fonts_mut(|f| (f.glyph_width(&font, '0').max(1.), f.row_height(&font)));
        let gutter = if self.prefs.line_numbers { 35. } else { 0. };
        // Single-line rows keep the 20px rhythm of the unwrapped view.
        let pad = (20. - row_height).max(2.);
        let text_width = (ui.available_width() - gutter - 16.).max(char_width * 8.);
        let per_line = ((text_width / char_width).floor() as usize).max(1);
        let mut tops = Vec::with_capacity(self.code_line_len.len() + 1);
        let mut y = 0.;
        tops.push(0.);
        for &length in &self.code_line_len {
            y += length.div_ceil(per_line).max(1) as f32 * row_height + pad;
            tops.push(y);
        }
        let total = y;
        let lines = self.code_lines.len().min(self.code_line_len.len());
        let selection = self.code_sel;
        let mut pointer = None;
        let find_current = self.find_matches.get(self.find_index).copied();
        let find_matches = std::mem::take(&mut self.find_matches);
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if std::mem::take(&mut self.find_jump)
            && let Some((line, _, _)) = find_current
        {
            area = area.vertical_scroll_offset((tops[line.min(tops.len() - 1)] - 80.).max(0.));
        }
        let scrolled = area.show_viewport(ui, |ui, viewport| {
            let mut state = CodePointer::new(ui);
            let origin = ui.cursor().min;
            ui.allocate_exact_size(vec2(ui.available_width(), total), Sense::hover());
            let first = tops
                .partition_point(|&t| t <= viewport.top())
                .saturating_sub(1);
            for i in first..lines {
                if tops[i] > viewport.bottom() {
                    break;
                }
                let top = origin.y + tops[i];
                if self.prefs.line_numbers {
                    ui.painter().text(
                        pos2(origin.x + 10., top + pad / 2. + row_height / 2.),
                        Align2::CENTER_CENTER,
                        (i + 1).to_string(),
                        FontId::monospace(13.),
                        p.muted,
                    );
                }
                let galley = code_galley(ui, self.code_lines[i].clone(), Some(text_width));
                let at = pos2(origin.x + gutter, top + pad / 2.);
                let length = self.code_line_len[i];
                let line_finds = finds_on(&find_matches, i);
                paint_code_line(
                    ui,
                    at,
                    &galley,
                    i,
                    length,
                    selection,
                    line_finds,
                    find_current,
                    p,
                );
                state.visit(i, length, &galley, at, top..origin.y + tops[i + 1]);
            }
            pointer = Some(state);
        });
        self.find_matches = find_matches;
        if let Some(pointer) = pointer {
            self.update_code_selection(&pointer);
        }
        drag_autoscroll(ui, scrolled);
    }
    /// Preferences > Themes: appearance mode plus one theme per mode.
    fn themes_tab(&mut self, ui: &mut Ui, p: Palette) -> bool {
        let mut changed = false;
        ui.label(RichText::new("APPEARANCE").size(12.).color(p.muted));
        ui.add_space(16.);
        ui.horizontal(|ui| {
            for (mode, label) in [
                (Appearance::Light, "Light"),
                (Appearance::Dark, "Dark"),
                (Appearance::System, "System"),
            ] {
                let dark = mode == Appearance::Dark;
                let preview = self.prefs.palette(dark);
                if theme_card(ui, preview, label, self.prefs.appearance == mode, p) {
                    self.prefs.appearance = mode;
                    changed = true;
                }
            }
        });
        ui.add_space(14.);
        ui.label(
            RichText::new(
                "The appearance is saved on this computer. System follows your operating system.",
            )
            .size(12.)
            .color(p.muted),
        );
        for (title, dark) in [("LIGHT THEME", false), ("DARK THEME", true)] {
            ui.add_space(26.);
            ui.label(RichText::new(title).size(12.).color(p.muted));
            ui.add_space(12.);
            let (names, current): (Vec<&str>, usize) = if dark {
                (
                    style::DARK_THEMES.iter().map(|t| t.0).collect(),
                    self.prefs.dark_theme,
                )
            } else {
                (
                    style::LIGHT_THEMES.iter().map(|t| t.0).collect(),
                    self.prefs.light_theme,
                )
            };
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(12., 14.);
                for (i, name) in names.iter().enumerate() {
                    let preview = Palette::themed(dark, i);
                    if theme_card(ui, preview, name, current == i, p) {
                        if dark {
                            self.prefs.dark_theme = i;
                            if self.prefs.appearance == Appearance::Light {
                                self.prefs.appearance = Appearance::Dark;
                            }
                        } else {
                            self.prefs.light_theme = i;
                            if self.prefs.appearance == Appearance::Dark {
                                self.prefs.appearance = Appearance::Light;
                            }
                        }
                        changed = true;
                    }
                }
            });
        }
        changed
    }
    /// Preferences > Display: text size, response options and the editor layout.
    fn display_tab(&mut self, ui: &mut Ui, p: Palette) -> bool {
        let mut changed = false;
        ui.label(RichText::new("DISPLAY").size(12.).color(p.muted));
        ui.add_space(24.);
        ui.label("Interface font size");
        ui.add_space(6.);
        ui.horizontal(|ui| {
            let size = self.prefs.font_size;
            let step = |ui: &mut Ui, label: &str, enabled: bool| {
                ui.add_enabled(
                    enabled,
                    egui::Button::new(RichText::new(label).size(15.)).min_size(vec2(32., 28.)),
                )
                .on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
                .clicked()
            };
            if step(ui, "−", size > 11.) {
                self.prefs.font_size = (size - 1.).max(11.);
                changed = true;
            }
            ui.add_sized(
                vec2(56., 28.),
                egui::Label::new(RichText::new(format!("{} px", size as u32)).strong()),
            );
            if step(ui, "+", size < 17.) {
                self.prefs.font_size = (size + 1.).min(17.);
                changed = true;
            }
            if size != 13. && ui.link("Reset").clicked() {
                self.prefs.font_size = 13.;
                changed = true;
            }
        });
        ui.add_space(18.);
        changed |= ui
            .checkbox(
                &mut self.prefs.line_numbers,
                "Show line numbers in responses",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.prefs.wrap_response,
                "Wrap long lines in responses",
            )
            .changed();
        ui.add_space(26.);
        ui.label("Request and response layout");
        ui.add_space(12.);
        ui.horizontal(|ui| {
            for (stacked, label) in [(false, "Side by side"), (true, "Stacked")] {
                let (rect, res) = ui.allocate_exact_size(vec2(146., 112.), Sense::click());
                let res = res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
                let selected = self.prefs.stacked_layout == stacked;
                let frame = Rect::from_min_size(rect.min, vec2(146., 83.));
                ui.painter().rect(
                    frame,
                    7,
                    p.bg,
                    Stroke::new(
                        if selected { 2. } else { 1. },
                        if selected { p.gold } else { p.border },
                    ),
                    StrokeKind::Inside,
                );
                let inner = frame.shrink(10.);
                let (a, b) = if stacked {
                    let h = (inner.height() - 6.) / 2.;
                    (
                        Rect::from_min_size(inner.min, vec2(inner.width(), h)),
                        Rect::from_min_size(inner.min + vec2(0., h + 6.), vec2(inner.width(), h)),
                    )
                } else {
                    let w = (inner.width() - 6.) / 2.;
                    (
                        Rect::from_min_size(inner.min, vec2(w, inner.height())),
                        Rect::from_min_size(inner.min + vec2(w + 6., 0.), vec2(w, inner.height())),
                    )
                };
                ui.painter().rect_filled(a, 3, p.field);
                ui.painter().rect_filled(b, 3, p.selected);
                text(
                    ui,
                    pos2(rect.left(), rect.bottom() - 12.),
                    label,
                    13.,
                    p.text,
                );
                if res.clicked() {
                    self.prefs.stacked_layout = stacked;
                    changed = true;
                }
            }
        });
        ui.add_space(20.);
        if ui.button("Reset layout").clicked() {
            self.sidebar_width = 244.;
            self.split_ratio = 0.48;
            self.split_ratio_v = 0.5;
            self.sidebar_visible = true;
        }
        changed
    }
    /// Writes the response bytes to a file the user picks (spreadsheets, PDFs, images, text…).
    fn save_response_file(&mut self) {
        let Some(response) = self.response.take() else {
            return;
        };
        let name = suggested_file_name(&response, &self.collection.requests[self.selected].url);
        let outcome = rfd::FileDialog::new()
            .set_file_name(&name)
            .save_file()
            .map(|path| {
                std::fs::write(&path, &response.raw)
                    .map(|_| {
                        format!(
                            "Saved {}",
                            path.file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        )
                    })
                    .map_err(|e| e.to_string())
            });
        self.response = Some(response);
        if let Some(result) = outcome {
            self.notify(result);
        }
    }
    /// Puts the caret at the end of the URL with nothing selected. The text field keeps its old
    /// selection range when the URL is replaced from outside (a pasted cURL), which would
    /// otherwise show part of the new URL highlighted.
    /// The `{{partial` right before the caret in the URL field, with the variables that fit it.
    fn variable_completion(&self, ctx: &egui::Context, id: egui::Id) -> Option<Completion> {
        if !ctx.memory(|m| m.has_focus(id)) {
            return None;
        }
        let url = &self.collection.requests[self.selected].url;
        let caret = egui::TextEdit::load_state(ctx, id)?
            .cursor
            .char_range()?
            .primary
            .index
            .0;
        let caret_byte = url.char_indices().nth(caret).map_or(url.len(), |(b, _)| b);
        let before = &url[..caret_byte];
        let open = before.rfind("{{")?;
        let partial = &before[open + 2..];
        if partial.contains(['}', ' ', '/', '?', '&', '=']) {
            return None;
        }
        let known: Variables = ctx
            .data(|d| d.get_temp(egui::Id::new(KNOWN_VARIABLES)))
            .unwrap_or_default();
        let partial_lower = partial.to_lowercase();
        let mut names: Vec<String> = known
            .keys()
            .filter(|n| n.to_lowercase().starts_with(&partial_lower))
            .cloned()
            .collect();
        names.sort();
        (!names.is_empty() && !(names.len() == 1 && names[0] == partial)).then_some(Completion {
            open,
            caret: caret_byte,
            names,
        })
    }
    /// Replaces the half-typed `{{par` with `{{name}}` and puts the caret after it.
    fn accept_variable(
        &mut self,
        ctx: &egui::Context,
        id: egui::Id,
        completion: &Completion,
        name: &str,
    ) {
        let url = &mut self.collection.requests[self.selected].url;
        let after = url[completion.caret..]
            .strip_prefix("}}")
            .unwrap_or(&url[completion.caret..])
            .to_owned();
        let head = url[..completion.open].to_owned();
        let inserted = format!("{{{{{name}}}}}");
        let caret_chars = head.chars().count() + inserted.chars().count();
        *url = format!("{head}{inserted}{after}");
        let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(caret_chars),
            )));
        state.store(ctx, id);
        self.var_sel = 0;
    }
    fn url_caret_to_end(&self, ctx: &egui::Context, id: egui::Id) {
        let end = self.collection.requests[self.selected].url.chars().count();
        let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(end),
            )));
        state.store(ctx, id);
    }
    fn param_rows(&self) -> Vec<Header> {
        let request = &self.collection.requests[self.selected];
        let query = request
            .url
            .split_once('?')
            .map(|(_, q)| q.split('#').next().unwrap_or(q))
            .unwrap_or("");
        url::form_urlencoded::parse(query.as_bytes())
            .map(|(k, v)| {
                let mut row = Header::new(&k, &v);
                row.description = request
                    .query_notes
                    .get(k.as_ref())
                    .cloned()
                    .unwrap_or_default();
                row
            })
            .collect()
    }
    fn set_param_rows(&mut self, rows: Vec<Header>) {
        let request = &mut self.collection.requests[self.selected];
        let prefix = request.url.split('?').next().unwrap_or("").to_owned();
        request.query_notes = rows
            .iter()
            .filter(|h| !h.description.is_empty())
            .map(|h| (h.name.clone(), h.description.clone()))
            .collect();
        let mut encoded = url::form_urlencoded::Serializer::new(String::new());
        for h in rows.iter().filter(|h| h.enabled) {
            encoded.append_pair(&h.name, &h.value);
        }
        let query = encoded.finish();
        request.url = if query.is_empty() {
            prefix
        } else {
            format!("{prefix}?{query}")
        };
    }
    fn bulk_rows(&self, target: &str) -> Vec<Header> {
        let request = &self.collection.requests[self.selected];
        match target {
            "headers" => request.headers.clone(),
            "urlencoded" => request.urlencoded.clone(),
            "form" => request.form.clone(),
            _ => self.param_rows(),
        }
    }
    fn set_bulk_rows(&mut self, target: &str, rows: Vec<Header>) {
        let request = &mut self.collection.requests[self.selected];
        match target {
            "headers" => request.headers = rows,
            "urlencoded" => request.urlencoded = rows,
            "form" => request.form = rows,
            _ => self.set_param_rows(rows),
        }
    }
    fn begin_bulk(&mut self, target: &'static str) {
        self.bulk_buf = rows_to_bulk(&self.bulk_rows(target));
        self.bulk_target = Some((self.selected, target));
    }
    /// Postman-style "Bulk Edit": the table as `key:value` lines, applied as you type.
    fn bulk_view(&mut self, ui: &mut Ui, p: Palette, target: &'static str) {
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let res = ui
                    .add(
                        egui::Label::new(RichText::new("Key-Value Edit").size(12.5).color(p.muted))
                            .sense(Sense::click()),
                    )
                    .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
                if res.clicked() {
                    self.bulk_target = None;
                }
            });
        });
        ui.add_space(6.);
        let rows = ((ui.available_height() / 17.) as usize).max(10);
        egui::Frame::new()
            .fill(p.field)
            .corner_radius(6)
            .inner_margin(10.)
            .show(ui, |ui| {
                let edit = ui.add(
                    TextEdit::multiline(&mut self.bulk_buf)
                        .code_editor()
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY)
                        .desired_rows(rows)
                        .hint_text("key:value\n//disabled:value"),
                );
                if edit.changed() {
                    let previous = self.bulk_rows(target);
                    let parsed = bulk_to_rows(&self.bulk_buf, &previous);
                    self.set_bulk_rows(target, parsed);
                    self.changed();
                }
            });
    }
    fn params_tab(&mut self, ui: &mut Ui, p: Palette) {
        ui.label(RichText::new("Query").size(12.).color(p.muted));
        ui.add_space(6.);
        if self
            .bulk_target
            .is_some_and(|(i, t)| i == self.selected && t == "params")
        {
            self.bulk_view(ui, p, "params");
            return;
        }
        let mut rows = self.param_rows();
        let mut bulk = false;
        if header_table(
            ui,
            &mut rows,
            &mut self.draft_header,
            p,
            false,
            Some(&mut bulk),
            false,
        ) {
            self.set_param_rows(rows);
            self.changed();
        }
        if bulk {
            self.begin_bulk("params");
        }
        ui.add_space(20.);
        ui.label(RichText::new("Path").size(12.).color(p.muted));
    }
    fn headers_tab(&mut self, ui: &mut Ui, p: Palette) {
        if self
            .bulk_target
            .is_some_and(|(i, t)| i == self.selected && t == "headers")
        {
            self.bulk_view(ui, p, "headers");
            return;
        }
        let mut bulk = false;
        if header_table(
            ui,
            &mut self.collection.requests[self.selected].headers,
            &mut self.draft_header,
            p,
            true,
            Some(&mut bulk),
            false,
        ) {
            self.changed();
        }
        if bulk {
            self.begin_bulk("headers");
        }
        ui.add_space(12.);
        ui.horizontal(|ui| {
            style::icon(
                ui,
                Rect::from_min_size(ui.cursor().min, vec2(16., 18.)),
                Icon::Eye,
                p.muted,
            );
            ui.add_space(22.);
            ui.label(
                RichText::new(format!(
                    "Collection headers ({})",
                    self.collection.headers.iter().filter(|h| h.enabled).count()
                ))
                .size(12.)
                .color(p.muted),
            );
        });
    }
    /// Body tab with the same choices as Postman: none, form-data, x-www-form-urlencoded,
    /// raw (JSON, Text, ...) and binary.
    fn body_tab(&mut self, ui: &mut Ui, p: Palette) {
        let sel = self.selected;
        let mode = self.collection.requests[sel].body_mode;
        let kind = self.collection.requests[sel].raw_kind;
        let (mut new_mode, mut new_kind, mut beautify) = (mode, None, false);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(16., 4.);
            for (m, label) in [
                (BodyMode::None, "none"),
                (BodyMode::FormData, "form-data"),
                (BodyMode::UrlEncoded, "x-www-form-urlencoded"),
                (BodyMode::Raw, "raw"),
                (BodyMode::Binary, "binary"),
            ] {
                let width = ui
                    .painter()
                    .layout_no_wrap(label.into(), FontId::proportional(13.), p.text)
                    .size()
                    .x;
                let (rect, res) = ui.allocate_exact_size(vec2(width + 24., 24.), Sense::click());
                let res = res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
                let centre = pos2(rect.left() + 8., rect.center().y);
                let selected = mode == m;
                ui.painter().circle_stroke(
                    centre,
                    7.,
                    Stroke::new(1.5, if selected { p.gold } else { p.muted }),
                );
                if selected {
                    ui.painter().circle_filled(centre, 3.5, p.gold);
                }
                text(
                    ui,
                    pos2(rect.left() + 22., rect.center().y),
                    label,
                    13.,
                    p.text,
                );
                if res.clicked() {
                    new_mode = m;
                }
            }
        });
        if mode == BodyMode::Raw {
            ui.add_space(6.);
            ui.horizontal(|ui| {
                let label = kind.label();
                let width = ui
                    .painter()
                    .layout_no_wrap(label.into(), FontId::proportional(13.), p.blue)
                    .size()
                    .x;
                let (rect, res) = ui.allocate_exact_size(vec2(width + 22., 24.), Sense::click());
                let res = res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
                text(ui, pos2(rect.left(), rect.center().y), label, 13., p.blue);
                style::chevron(
                    ui,
                    Rect::from_center_size(
                        pos2(rect.right() - 8., rect.center().y),
                        vec2(12., 12.),
                    ),
                    1.,
                    p.blue,
                );
                if let Some(k) = egui::Popup::menu(&res)
                    .show(|ui| {
                        ui.set_width(120.);
                        let mut chosen = None;
                        for k in RawKind::ALL {
                            if menu_item_plain(ui, k.label(), p) {
                                chosen = Some(k);
                            }
                        }
                        if chosen.is_some() {
                            ui.close();
                        }
                        chosen
                    })
                    .and_then(|r| r.inner)
                {
                    new_kind = Some(k);
                }
                if kind == RawKind::Json {
                    // Right next to the language picker, as in Postman.
                    ui.add_space(14.);
                    let res = ui
                        .add(
                            egui::Label::new(RichText::new("Beautify").size(13.).color(p.blue))
                                .sense(Sense::click()),
                        )
                        .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
                    beautify = res.clicked();
                }
            });
        }
        if new_mode != mode {
            self.collection.requests[sel].body_mode = new_mode;
            self.bulk_target = None;
            self.changed();
        }
        if let Some(k) = new_kind {
            self.collection.requests[sel].raw_kind = k;
            self.changed();
        }
        if beautify {
            let body = &mut self.collection.requests[sel].body;
            match serde_json::from_str::<serde_json::Value>(body) {
                Ok(v) => {
                    *body = serde_json::to_string_pretty(&v).unwrap_or_default();
                    self.changed();
                }
                Err(e) => self.notify(Err(format!("Not valid JSON: {e}"))),
            }
        }
        ui.add_space(10.);
        match self.collection.requests[sel].body_mode {
            BodyMode::None => {
                ui.add_space(40.);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("This request does not have a body").color(p.muted));
                });
            }
            BodyMode::Raw => self.body_editor(ui, p),
            BodyMode::FormData | BodyMode::UrlEncoded => {
                let form = self.collection.requests[sel].body_mode == BodyMode::FormData;
                let target = if form { "form" } else { "urlencoded" };
                if self
                    .bulk_target
                    .is_some_and(|(i, t)| i == sel && t == target)
                {
                    self.bulk_view(ui, p, target);
                } else {
                    let mut bulk = false;
                    let request = &mut self.collection.requests[sel];
                    let rows = if form {
                        &mut request.form
                    } else {
                        &mut request.urlencoded
                    };
                    if header_table(
                        ui,
                        rows,
                        &mut self.draft_header,
                        p,
                        true,
                        Some(&mut bulk),
                        form,
                    ) {
                        self.changed();
                    }
                    if bulk {
                        self.begin_bulk(target);
                    }
                }
            }
            BodyMode::Binary => {
                let path = self.collection.requests[sel].binary_path.clone();
                ui.horizontal(|ui| {
                    if ui.button("Select File").clicked()
                        && let Some(file) = rfd::FileDialog::new().pick_file()
                    {
                        self.collection.requests[sel].binary_path =
                            file.to_string_lossy().into_owned();
                        self.changed();
                    }
                    if path.is_empty() {
                        ui.label(RichText::new("No file selected").color(p.muted));
                    } else {
                        ui.label(RichText::new(&path).color(p.blue));
                        if ui.link("Clear").clicked() {
                            self.collection.requests[sel].binary_path.clear();
                            self.changed();
                        }
                    }
                });
                ui.add_space(8.);
                ui.label(
                    RichText::new("The file is sent as the raw request body (application/octet-stream unless you set a Content-Type header).")
                        .size(12.)
                        .color(p.muted),
                );
            }
        }
    }
    /// Raw body editor: no line wrapping (long values scroll sideways so indentation stays
    /// intact) and JSON bodies are coloured like responses.
    fn body_editor(&mut self, ui: &mut Ui, p: Palette) {
        let width = ui.available_width();
        let rows = ((ui.available_height() / 17.) as usize).max(12);
        let json = self.collection.requests[self.selected].raw_kind == RawKind::Json;
        let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, _wrap: f32| {
            let text = text.as_str();
            let font = FontId::monospace(13.);
            let plain = |text: &str| {
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    text,
                    0.,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color: p.text,
                        line_height: Some(20.),
                        ..Default::default()
                    },
                );
                job
            };
            let trimmed = text.trim_start();
            let mut job = if json
                && (trimmed.starts_with('{') || trimmed.starts_with('['))
                && text.len() < 200_000
            {
                let mut job = egui::text::LayoutJob::default();
                for (i, line) in style::highlight(text, p).into_iter().enumerate() {
                    if i > 0 {
                        job.append(
                            "\n",
                            0.,
                            egui::TextFormat {
                                font_id: font.clone(),
                                line_height: Some(20.),
                                ..Default::default()
                            },
                        );
                    }
                    for section in &line.sections {
                        job.append(
                            &line.text[section.byte_range.start.0..section.byte_range.end.0],
                            0.,
                            egui::TextFormat {
                                font_id: font.clone(),
                                line_height: Some(20.),
                                ..section.format.clone()
                            },
                        );
                    }
                }
                // The editor needs a galley made of exactly the text it holds.
                if job.text == text { job } else { plain(text) }
            } else {
                plain(text)
            };
            job.wrap.max_width = f32::INFINITY;
            let job = tint_variables(job, ui, p);
            ui.fonts_mut(|f| f.layout_job(job))
        };
        // Line numbers stay put while the text scrolls sideways: the gutter is drawn outside the
        // scroll area, using the screen position of each line.
        let area = ui.available_rect_before_wrap();
        let gutter = if self.prefs.line_numbers { 38. } else { 0. };
        let gutter_rect = Rect::from_min_size(area.min, vec2(gutter, area.height()));
        let editor_rect = Rect::from_min_max(pos2(area.left() + gutter, area.top()), area.max);
        let gutter_painter = ui.painter().with_clip_rect(gutter_rect);
        // Same 20px rhythm as the response viewer.
        let row_height = 20.;
        let muted = p.muted;
        ui.allocate_rect(area, Sense::hover());
        ui.scope_builder(egui::UiBuilder::new().max_rect(editor_rect), |ui| {
            egui::ScrollArea::both()
                .id_salt("body-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let body = &mut self.collection.requests[self.selected].body;
                    let output = TextEdit::multiline(body)
                        .frame(egui::Frame::NONE)
                        .margin(egui::Margin::ZERO)
                        .layouter(&mut layouter)
                        .desired_width((width - gutter).max(80.))
                        .desired_rows(rows)
                        .hint_text("Enter request body…")
                        .show(ui);
                    if gutter > 0. {
                        let lines = body.split('\n').count().max(1);
                        let top = output.galley_pos.y;
                        let first =
                            (((gutter_rect.top() - top) / row_height).floor().max(0.)) as usize;
                        for i in first..lines {
                            // The editor draws text a little above the centre of a taller row.
                            let y = top + (i as f32 + 0.5) * row_height - 3.;
                            if y > gutter_rect.bottom() + row_height {
                                break;
                            }
                            gutter_painter.text(
                                pos2(gutter_rect.right() - 10., y),
                                Align2::RIGHT_CENTER,
                                (i + 1).to_string(),
                                FontId::monospace(13.),
                                muted,
                            );
                        }
                    }
                    if output.response.changed() {
                        self.changed();
                    }
                });
        });
    }
    /// The Auth tab: how this request identifies itself (none, API key, bearer token or basic).
    /// The type is picked on the left, its fields sit in rows on the right.
    fn auth_tab(&mut self, ui: &mut Ui, p: Palette) {
        use post_ronda::AuthKind;
        let index = self.selected;
        let kinds = [
            (AuthKind::None, "No Auth"),
            (AuthKind::ApiKey, "API Key"),
            (AuthKind::Bearer, "Bearer Token"),
            (AuthKind::Basic, "Basic Auth"),
        ];
        let current = self.collection.requests[index].auth.kind;
        let current_label = kinds
            .iter()
            .find(|k| k.0 == current)
            .map(|k| k.1)
            .unwrap_or("No Auth");
        let total = ui.available_width();
        let side = 190.;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(side, 10.), Layout::top_down(Align::Min), |ui| {
                ui.label(RichText::new("Auth type").size(12.).color(p.muted));
                ui.add_space(4.);
                if let Some(choice) = dropdown_field(ui, "auth-type", current_label, side - 12., &kinds.map(|k| k.1), p) {
                    self.collection.requests[index].auth.kind = kinds[choice].0;
                }
                ui.add_space(10.);
                let hint = match current {
                    AuthKind::None => "No credentials are added to this request.",
                    _ => "The credentials are added to the request when it is sent, as a header or a query parameter. Values can use {{variables}}.",
                };
                ui.add(egui::Label::new(RichText::new(hint).size(12.).color(p.muted)).wrap());
            });
            let (divider, _) = ui.allocate_exact_size(vec2(1., 1.), Sense::hover());
            let _ = divider;
            ui.add_space(14.);
            ui.vertical(|ui| {
                let width = (total - side - 40.).max(240.);
                ui.set_width(width);
                let show_secret = self.show_secret;
                let auth = &mut self.collection.requests[index].auth;
                ui.label(RichText::new(current_label).size(15.));
                ui.add_space(12.);
                // One row per setting: the name on the left, the field filling the rest.
                let row = |ui: &mut Ui, label: &str, value: &mut String, hint: &str, secret: bool| {
                    ui.horizontal(|ui| {
                        let (cell, _) = ui.allocate_exact_size(vec2(84., 30.), Sense::hover());
                        text(ui, pos2(cell.left(), cell.center().y), label, 13., p.text);
                        let mut layouter = variable_layouter(p, FontId::proportional(13.));
                        let edit = TextEdit::singleline(value)
                            .hint_text(hint)
                            .desired_width((ui.available_width() - 4.).clamp(160., 460.))
                            .margin(egui::Margin::symmetric(8, 7));
                        ui.add(if secret { edit.password(true) } else { edit.layouter(&mut layouter) });
                    });
                    ui.add_space(4.);
                };
                match auth.kind {
                    AuthKind::None => {}
                    AuthKind::ApiKey => {
                        row(ui, "Key", &mut auth.key, "X-API-Key", false);
                        row(ui, "Value", &mut auth.value, "{{api_key}}", !show_secret);
                        ui.horizontal(|ui| {
                            let (cell, _) = ui.allocate_exact_size(vec2(84., 32.), Sense::hover());
                            text(ui, pos2(cell.left(), cell.center().y), "Add to", 13., p.text);
                            let places = ["Header", "Query params"];
                            let width = (ui.available_width() - 4.).clamp(110., 200.);
                            if let Some(choice) = dropdown_field(ui, "auth-place", places[auth.in_query as usize], width, &places, p) {
                                auth.in_query = choice == 1;
                            }
                        });
                    }
                    AuthKind::Bearer => row(ui, "Token", &mut auth.token, "{{token}}", !show_secret),
                    AuthKind::Basic => {
                        row(ui, "Username", &mut auth.username, "Username", false);
                        row(ui, "Password", &mut auth.password, "Password", !show_secret);
                    }
                }
                if auth.kind != AuthKind::None {
                    ui.add_space(8.);
                    ui.horizontal(|ui| {
                        ui.add_space(88.);
                        ui.checkbox(&mut self.show_secret, "Show secret values");
                    });
                }
            });
        });
    }
    fn request_editor(&mut self, ui: &mut Ui, r: Rect) {
        let p = self.palette();
        let request = &self.collection.requests[self.selected];
        let params = self.param_rows().len();
        let header_count = request.headers.len();
        let labels = [
            ("Params", params, 62.),
            ("Body", 0, 48.),
            ("Headers", header_count, 76.),
            ("Auth", 0, 48.),
            ("Docs", 0, 48.),
        ];
        let auth_set = request.auth.is_set();
        let mut x = r.left();
        for (i, (label, count, width)) in labels.into_iter().enumerate() {
            let rect = Rect::from_min_size(pos2(x, r.top()), vec2(width, 35.));
            if style::tab(ui, rect, label, count, self.tab == i, p).clicked() {
                self.tab = i;
            }
            if label == "Auth" && auth_set {
                ui.painter()
                    .circle_filled(pos2(rect.right() - 6., rect.top() + 12.), 3., p.green);
            }
            x += width;
        }
        let body = Rect::from_min_max(r.min + vec2(0., 49.), r.max);
        region(ui, body, "request-tab-body", |ui| {
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| match self.tab {
                    0 => {
                        self.params_tab(ui, p);
                    }
                    1 => {
                        self.body_tab(ui, p);
                    }
                    2 => {
                        self.headers_tab(ui, p);
                    }
                    3 => {
                        self.auth_tab(ui, p);
                    }
                    _ => {
                        ui.label("Documentation");
                        ui.add_space(10.);
                        if ui
                            .add_sized(
                                vec2(ui.available_width(), ui.available_height().max(300.)),
                                TextEdit::multiline(
                                    &mut self.collection.requests[self.selected].description,
                                )
                                .frame(egui::Frame::NONE)
                                .hint_text("Document this request using Markdown…"),
                            )
                            .changed()
                        {
                            self.changed();
                        }
                    }
                });
        });
    }
    fn response_view(&mut self, ui: &mut Ui, r: Rect) {
        if !self.running {
            return self.response_content(ui, r);
        }
        // While a request is in flight the previous response stays visible but dimmed.
        let p = self.palette();
        ui.ctx().request_repaint_after(Duration::from_millis(100));
        if self.response.is_some() {
            ui.set_opacity(0.25);
            self.response_content(ui, r);
            ui.set_opacity(1.);
        }
        let elapsed = self
            .sent_at
            .map(|t| t.elapsed().as_secs_f32())
            .unwrap_or(0.);
        let c = pos2(r.center().x, r.top() + r.height() * 0.38);
        ui.painter().text(
            c,
            Align2::CENTER_CENTER,
            format!("{elapsed:.1}s"),
            FontId::proportional(20.),
            p.text,
        );
        ui.put(
            Rect::from_center_size(c + vec2(0., 38.), vec2(26., 26.)),
            egui::Spinner::new().size(24.).color(p.muted),
        );
        let button = Rect::from_center_size(c + vec2(0., 88.), vec2(116., 30.));
        if outlined(ui, button, "Cancel Request", None, p).clicked() {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }
    fn response_content(&mut self, ui: &mut Ui, r: Rect) {
        let p = self.palette();
        if self.response.is_none() {
            self.empty_response(ui, r);
            return;
        }
        let header_count = self
            .response
            .as_ref()
            .unwrap()
            .headers
            .lines()
            .filter(|s| s.contains(':'))
            .count();
        // Tabs are as wide as their text so spacing stays even for any count.
        let tab_width = |ui: &Ui, label: &str, count: usize| {
            let text = ui
                .painter()
                .layout_no_wrap(label.into(), FontId::proportional(13.), p.text)
                .size()
                .x;
            text + if count > 0 {
                4. + 6. * count.to_string().len() as f32
            } else {
                0.
            } + 10.
        };
        let response_width = tab_width(ui, "Response", 0);
        let headers_width = tab_width(ui, "Headers", header_count);
        let headers_left = r.left() + response_width + 14.;
        if style::tab(
            ui,
            Rect::from_min_size(r.min, vec2(response_width, 35.)),
            "Response",
            0,
            self.response_tab == 0,
            p,
        )
        .clicked()
        {
            self.response_tab = 0;
        }
        if style::tab(
            ui,
            Rect::from_min_size(pos2(headers_left, r.top()), vec2(headers_width, 35.)),
            "Headers",
            header_count,
            self.response_tab == 1,
            p,
        )
        .clicked()
        {
            self.response_tab = 1;
        }
        let tabs_right = headers_left + headers_width;
        let response = self.response.as_ref().unwrap();
        let status = response
            .status
            .split_once(' ')
            .map(|(_, s)| s)
            .unwrap_or(&response.status)
            .to_owned();
        let binary = response.binary;
        let binary_info = format!(
            "{} · {}",
            format_size(response.bytes),
            if response.content_type.is_empty() {
                "unknown type"
            } else {
                response.content_type.as_str()
            }
        );
        let metrics = format!(
            "{}ms  {}",
            response.elapsed.as_millis(),
            format_size(response.bytes)
        );
        let metrics_width = ui
            .painter()
            .layout_no_wrap(metrics.clone(), FontId::proportional(12.), p.text)
            .size()
            .x;
        let status_width = ui
            .painter()
            .layout_no_wrap(status.clone(), FontId::proportional(13.), p.text)
            .size()
            .x;
        let status_x = (r.right() - 134. - metrics_width - status_width - 9.).max(tabs_right + 12.);
        text(
            ui,
            pos2(status_x, r.top() + 17.),
            &status,
            13.,
            if status.starts_with('2') {
                p.green
            } else {
                p.gold
            },
        );
        text(
            ui,
            pos2(status_x + status_width + 9., r.top() + 17.),
            &metrics,
            12.,
            p.text,
        );
        let format_label = if self.pretty { "JSON" } else { "Raw" };
        let format_width = ui
            .painter()
            .layout_no_wrap(format_label.into(), FontId::proportional(13.), p.text)
            .size()
            .x
            + 31.
            + 26.;
        let format_rect = Rect::from_min_size(
            pos2(status_x - 14. - format_width, r.top() + 5.),
            vec2(format_width, 25.),
        );
        if !binary && format_rect.left() > tabs_right + 14. {
            let rect = format_rect;
            let response = outlined(ui, rect, format_label, Some(Icon::Code), p);
            style::chevron(
                ui,
                Rect::from_center_size(pos2(rect.right() - 14., rect.center().y), vec2(12., 12.)),
                1.,
                p.muted,
            );
            egui::Popup::menu(&response).show(|ui| {
                ui.set_width(150.);
                ui.spacing_mut().item_spacing.y = 1.;
                for (value, label) in [(true, "Pretty JSON"), (false, "Raw text")] {
                    if menu_choice(ui, label, self.pretty == value, None, p) {
                        self.pretty = value;
                        self.response_body();
                        ui.close();
                    }
                }
            });
        }
        // Actions sit in the toolbar instead of behind a menu: copy, save to a file, layout.
        let mut action = None;
        for (i, (icon, tip)) in [
            (Icon::Search, "Find in response (⌘/Ctrl+F)"),
            (Icon::Copy, "Copy response"),
            (Icon::Pull, "Save response to a file…"),
            (
                if self.prefs.stacked_layout {
                    Icon::Rows
                } else {
                    Icon::Split
                },
                if self.prefs.stacked_layout {
                    "Layout: stacked. Click for side by side"
                } else {
                    "Layout: side by side. Click for stacked"
                },
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let rect = Rect::from_min_size(
                pos2(r.right() - 120. + i as f32 * 30., r.top() + 3.),
                vec2(28., 28.),
            );
            if icon_button(ui, rect, icon, tip, p).clicked() {
                action = Some(i);
            }
        }
        match action {
            Some(1) if binary => {
                self.notify(Err(
                    "This response is a file, so there is no text to copy. Save it instead.".into(),
                ));
            }
            Some(1) => {
                ui.ctx().copy_text(
                    if let (0, Some(selected)) = (self.response_tab, self.selected_code()) {
                        selected
                    } else if self.response_tab == 0 {
                        self.response_text.clone()
                    } else {
                        self.response
                            .as_ref()
                            .map(|r| r.headers.clone())
                            .unwrap_or_default()
                    },
                );
                self.notify(Ok("Copied to clipboard!".into()));
            }
            Some(2) => self.save_response_file(),
            Some(3) => {
                self.prefs.stacked_layout = !self.prefs.stacked_layout;
                self.store_preferences(ui.ctx());
            }
            Some(0) => {
                self.find_open = !self.find_open;
                self.find_focus = self.find_open;
            }
            _ => {}
        }
        self.response_rect = r;
        let body = Rect::from_min_max(r.min + vec2(0., 50.), r.max);
        region(ui, body, "response-content", |ui| {
            if binary && self.response_tab == 0 {
                let c = ui.max_rect().center() - vec2(0., 24.);
                style::icon_sized(
                    ui,
                    Rect::from_center_size(c - vec2(0., 46.), vec2(48., 48.)),
                    Icon::Pull,
                    p.muted,
                    44.,
                );
                ui.painter().text(
                    c + vec2(0., 2.),
                    Align2::CENTER_CENTER,
                    "Binary response",
                    FontId::proportional(15.),
                    p.text,
                );
                ui.painter().text(
                    c + vec2(0., 26.),
                    Align2::CENTER_CENTER,
                    &binary_info,
                    FontId::proportional(12.),
                    p.muted,
                );
                if outlined(
                    ui,
                    Rect::from_center_size(c + vec2(0., 64.), vec2(130., 30.)),
                    "Save to file…",
                    Some(Icon::Pull),
                    p,
                )
                .clicked()
                {
                    self.save_response_file();
                }
                return;
            }
            if self.response_tab == 1 {
                let mut headers = self.response.as_ref().unwrap().headers.as_str();
                ui.add(
                    TextEdit::multiline(&mut headers)
                        .code_editor()
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY),
                );
                return;
            }
            if self.prefs.wrap_response {
                self.wrapped_body(ui, p);
                return;
            }
            let selection = self.code_sel;
            let mut pointer = None;
            let find_current = self.find_matches.get(self.find_index).copied();
            let find_matches = std::mem::take(&mut self.find_matches);
            let mut area = egui::ScrollArea::both().auto_shrink([false, false]);
            if std::mem::take(&mut self.find_jump)
                && let Some((line, _, _)) = find_current
            {
                area = area.vertical_scroll_offset((line as f32 * 20. - 80.).max(0.));
            }
            let scrolled = area.show_rows(ui, 20., self.code_lines.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.;
                let mut state = CodePointer::new(ui);
                let offset = if self.prefs.line_numbers { 35. } else { 0. };
                for i in range {
                    let galley = code_galley(ui, self.code_lines[i].clone(), None);
                    // Wide enough for the longest line so the pane scrolls sideways.
                    let width = ui.available_width().max(offset + galley.size().x + 16.);
                    let (rect, _) = ui.allocate_exact_size(vec2(width, 20.), Sense::hover());
                    if self.prefs.line_numbers {
                        ui.painter().text(
                            pos2(rect.left() + 10., rect.center().y),
                            Align2::CENTER_CENTER,
                            (i + 1).to_string(),
                            FontId::monospace(13.),
                            p.muted,
                        );
                    }
                    let at = rect.min + vec2(offset, 1.);
                    let length = self.code_line_len[i];
                    let line_finds = finds_on(&find_matches, i);
                    paint_code_line(
                        ui,
                        at,
                        &galley,
                        i,
                        length,
                        selection,
                        line_finds,
                        find_current,
                        p,
                    );
                    state.visit(i, length, &galley, at, rect.top()..rect.bottom());
                }
                pointer = Some(state);
            });
            self.find_matches = find_matches;
            if let Some(pointer) = pointer {
                self.update_code_selection(&pointer);
            }
            drag_autoscroll(ui, scrolled);
        });
        self.find_bar(ui, body, p);
    }
    /// Recomputes the matches of the find bar's query in the response text.
    fn find_in_response(&mut self) {
        self.find_for = self.find_query.clone();
        self.find_matches.clear();
        self.find_index = 0;
        self.find_jump = true;
        let needle: Vec<char> = self
            .find_query
            .chars()
            .filter_map(|c| c.to_lowercase().next())
            .collect();
        if needle.is_empty() {
            return;
        }
        'lines: for (line, job) in self.code_lines.iter().enumerate() {
            let hay: Vec<char> = job
                .text
                .chars()
                .map(|c| c.to_lowercase().next().unwrap_or(c))
                .collect();
            let mut at = 0;
            while at + needle.len() <= hay.len() {
                if hay[at..at + needle.len()] == needle[..] {
                    self.find_matches.push((line, at, at + needle.len()));
                    if self.find_matches.len() >= 10_000 {
                        break 'lines;
                    }
                    at += needle.len();
                } else {
                    at += 1;
                }
            }
        }
    }
    /// The find bar floating at the top right of the response body.
    fn find_bar(&mut self, ui: &mut Ui, body: Rect, p: Palette) {
        if !self.find_open || self.response_tab != 0 || self.response.is_none() {
            return;
        }
        if self.find_for != self.find_query {
            self.find_in_response();
        }
        let bar = Rect::from_min_size(pos2(body.right() - 372., body.top() + 6.), vec2(358., 34.));
        ui.painter().rect(
            bar,
            7,
            p.sidebar,
            Stroke::new(1., p.border),
            StrokeKind::Inside,
        );
        let edit_rect = Rect::from_min_size(bar.min + vec2(8., 4.), vec2(190., 26.));
        let edit = ui.put(
            edit_rect,
            TextEdit::singleline(&mut self.find_query)
                .hint_text("Find in response")
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::symmetric(4, 5))
                .font(FontId::proportional(13.)),
        );
        if std::mem::take(&mut self.find_focus) {
            edit.request_focus();
        }
        let total = self.find_matches.len();
        let status = if self.find_query.is_empty() {
            String::new()
        } else if total == 0 {
            "No results".to_owned()
        } else {
            format!(
                "{} of {}{}",
                self.find_index + 1,
                total,
                if total >= 10_000 { "+" } else { "" }
            )
        };
        text(
            ui,
            pos2(bar.left() + 206., bar.center().y),
            &status,
            12.,
            if total == 0 && !self.find_query.is_empty() {
                p.red
            } else {
                p.muted
            },
        );
        let mut step = None;
        if edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            step = Some(if ui.input(|i| i.modifiers.shift) {
                -1
            } else {
                1
            });
            edit.request_focus();
        }
        for (n, (glyph, tip, delta)) in [("↑", "Previous match", -1), ("↓", "Next match", 1)]
            .into_iter()
            .enumerate()
        {
            let rect = Rect::from_min_size(
                pos2(bar.right() - 92. + 26. * n as f32, bar.top() + 5.),
                vec2(24., 24.),
            );
            let res = ui
                .interact(rect, ui.id().with(("find-step", n)), Sense::click())
                .on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
                .on_hover_text(tip);
            if res.hovered() {
                ui.painter().rect_filled(rect, 4, p.hover);
            }
            text(ui, rect.center() - vec2(4., 0.), glyph, 14., p.text);
            if res.clicked() {
                step = Some(delta);
            }
        }
        if let Some(delta) = step
            && total > 0
        {
            self.find_index = (self.find_index as i64 + delta).rem_euclid(total as i64) as usize;
            self.find_jump = true;
        }
        if icon_button(
            ui,
            Rect::from_min_size(pos2(bar.right() - 38., bar.top() + 5.), vec2(24., 24.)),
            Icon::Close,
            "Close (Esc)",
            p,
        )
        .clicked()
            || (edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)))
        {
            self.find_open = false;
        }
    }
    fn empty_response(&mut self, ui: &mut Ui, r: Rect) {
        let p = self.palette();
        let c = pos2(r.center().x, r.top() + r.height() * 0.31);
        let color = if p.dark {
            Color32::from_gray(74)
        } else {
            Color32::from_gray(196)
        };
        style::empty_state_icon(ui, Rect::from_center_size(c, vec2(140., 140.)), color);
        let y = c.y + 111.;
        for (i, label, key) in [
            (
                0,
                if self.running {
                    "Sending Request…"
                } else {
                    "Send Request"
                },
                "⌘ + ↵",
            ),
            (1, "New Request", "⌘ + N"),
            (2, "Edit Environments", "⌘ + E"),
        ] {
            text(ui, pos2(c.x - 100., y + 35. * i as f32), label, 13., p.text);
            text(ui, pos2(c.x + 50., y + 35. * i as f32), key, 12., p.muted);
        }
    }
    fn overview(&mut self, ui: &mut Ui, r: Rect) {
        let p = self.palette();
        region(ui, r.shrink2(vec2(24., 24.)), "overview-content", |ui| {
            ui.heading(&self.collection.name);
            ui.add_space(10.);
            ui.horizontal(|ui| {
                ui.label(format!("{} requests", self.collection.requests.len()));
                ui.separator();
                ui.label(format!("{} environments", self.environments.items.len()));
            });
            ui.add_space(6.);
            ui.label(
                RichText::new(self.project.display().to_string())
                    .size(12.)
                    .color(p.muted),
            );
            ui.add_space(24.);
            ui.label(RichText::new("Quick Actions").strong());
            ui.add_space(8.);
            ui.horizontal(|ui| {
                if ui.button("New Request").clicked() {
                    self.new_request();
                }
                if ui.button("Import cURL").clicked() {
                    self.import_open = true;
                }
                if ui.button("Open Collection").clicked() {
                    self.open_project();
                }
                if ui.button("Import into project").clicked() {
                    self.import_collection_file(true);
                }
            });
            ui.add_space(28.);
            ui.label(RichText::new("Documentation").strong());
            ui.add_space(10.);
            if ui.add_sized(vec2(ui.available_width(),200.),TextEdit::multiline(&mut self.collection.description).frame(egui::Frame::NONE).hint_text("Add documentation to help your team work smoothly.\n\nProject overview, setup instructions, key workflows…")).changed(){self.changed();}
        });
    }
    pub(super) fn open_project_settings(&mut self, tab: usize) {
        self.project_settings = true;
        self.collection_tab = tab;
        self.settings = false;
        self.settings_chooser = false;
    }
    /// Left list of tabs shared by both settings dialogs.
    fn settings_nav(ui: &mut Ui, labels: &[(&str, Icon)], current: &mut usize, p: Palette) {
        for (i, (label, kind)) in labels.iter().enumerate() {
            let (rect, res) =
                ui.allocate_exact_size(vec2(ui.available_width(), 32.), Sense::click());
            let res = res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
            if *current == i || res.hovered() {
                ui.painter()
                    .rect_filled(rect, 4, if *current == i { p.selected } else { p.hover });
            }
            style::icon(
                ui,
                Rect::from_min_size(rect.min + vec2(10., 8.), vec2(16., 16.)),
                *kind,
                p.muted,
            );
            text(
                ui,
                pos2(rect.left() + 35., rect.center().y),
                label,
                13.,
                if *current == i { p.text } else { p.muted },
            );
            if res.clicked() {
                *current = i;
            }
        }
    }
    /// Settings of the app itself, the same for every project.
    fn app_settings_dialog(&mut self, ctx: &egui::Context) {
        let p = self.palette();
        let open = self.settings;
        let close = modal(ctx, "app-settings", open, 820., "App settings", p, |ui| {
            let labels = [
                ("General", Icon::Settings),
                ("Themes", Icon::Palette),
                ("Display", Icon::Sidebar),
                ("Network", Icon::Globe),
                ("Keybindings", Icon::Code),
                ("About", Icon::Info),
            ];
            let mut tab = self.prefs_tab.min(labels.len() - 1);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(vec2(160., 440.), Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.;
                    Self::settings_nav(ui, &labels, &mut tab, p);
                });
                ui.add_space(8.);
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), 440.),
                    Layout::top_down(Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("app-settings-scroll")
                            .max_height(440.)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let mut changed = false;
                                match tab {
                                    0 => self.general_tab(ui, p),
                                    1 => changed |= self.themes_tab(ui, p),
                                    2 => changed |= self.display_tab(ui, p),
                                    3 => changed |= self.network_tab(ui, p),
                                    4 => Self::keybindings_tab(ui, p),
                                    _ => self.about_tab(ui, p),
                                }
                                if changed {
                                    self.store_preferences(ui.ctx());
                                }
                            });
                    },
                );
            });
            self.prefs_tab = tab;
        });
        if close {
            self.settings = false;
        }
    }
    fn general_tab(&mut self, ui: &mut Ui, p: Palette) {
        ui.label(RichText::new("GENERAL").size(12.).color(p.muted));
        ui.add_space(20.);
        ui.label("Saving");
        ui.add_space(4.);
        ui.label(
            RichText::new(
                "Changes to a request are kept until you press Save or ⌘/Ctrl+S; unsaved requests show a dot on their tab. Renames, folders, project settings and environments are written automatically.",
            )
            .size(12.)
            .color(p.muted),
        );
        ui.add_space(24.);
        ui.label("Where the app keeps its own data");
        ui.add_space(4.);
        ui.add(
            egui::Label::new(
                RichText::new(crate::state_dir().display().to_string()).color(p.muted),
            )
            .wrap(),
        );
        ui.label(
            RichText::new("Preferences and the list of projects. Each project's requests live in the project's own folder.")
                .size(12.)
                .color(p.muted),
        );
    }
    /// Proxy, extra certificates and cookies: how requests leave this computer.
    fn network_tab(&mut self, ui: &mut Ui, p: Palette) -> bool {
        let mut changed = false;
        ui.label(RichText::new("NETWORK").size(12.).color(p.muted));
        ui.add_space(20.);
        ui.label("Proxy");
        ui.add_space(4.);
        changed |= ui
            .add(
                TextEdit::singleline(&mut self.prefs.proxy)
                    .hint_text("http://proxy.example:8080")
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(8, 7)),
            )
            .changed();
        ui.add_space(2.);
        ui.label(
            RichText::new("Leave empty to follow the system: HTTP_PROXY, HTTPS_PROXY and NO_PROXY are honoured.")
                .size(12.)
                .color(p.muted),
        );
        ui.add_space(22.);
        ui.label("Certificates");
        ui.add_space(6.);
        changed |= file_field(
            ui,
            "CA certificate (PEM), trusted in addition to the system ones",
            &mut self.prefs.ca_cert,
            p,
        );
        changed |= file_field(
            ui,
            "Client certificate, for servers that ask for one",
            &mut self.prefs.client_cert,
            p,
        );
        changed |= file_field(
            ui,
            "Client key, when it is a separate file",
            &mut self.prefs.client_key,
            p,
        );
        ui.add_space(14.);
        ui.label("Cookies");
        ui.add_space(6.);
        changed |= ui
            .checkbox(
                &mut self.prefs.keep_cookies,
                "Keep cookies between requests (separate for each project)",
            )
            .changed();
        let jar = crate::cookie_file(&self.project);
        let count = std::fs::read_to_string(&jar)
            .map(|text| {
                text.lines()
                    .filter(|l| {
                        !l.trim().is_empty() && (!l.starts_with('#') || l.starts_with("#HttpOnly_"))
                    })
                    .count()
            })
            .unwrap_or(0);
        ui.add_space(4.);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{count} cookie{} saved for this project.",
                    if count == 1 { "" } else { "s" }
                ))
                .size(12.)
                .color(p.muted),
            );
            if count > 0 && ui.link("Clear").clicked() {
                let _ = std::fs::remove_file(&jar);
            }
        });
        changed
    }
    fn keybindings_tab(ui: &mut Ui, p: Palette) {
        ui.label(RichText::new("KEYBINDINGS").size(12.).color(p.muted));
        ui.add_space(20.);
        egui::Grid::new("keybindings")
            .spacing([60., 16.])
            .show(ui, |ui| {
                for (label, key) in [
                    ("Send Request", "⌘ / Ctrl + Enter"),
                    ("New Request", "⌘ / Ctrl + N"),
                    ("Save Request", "⌘ / Ctrl + S"),
                    ("Close Tab", "⌘ / Ctrl + W"),
                    ("Search Requests", "⌘ / Ctrl + K  or  ⌘ / Ctrl + P"),
                    ("Filter Sidebar", "⌘ / Ctrl + F"),
                    ("Edit Environments", "⌘ / Ctrl + E"),
                    ("Import cURL", "⌘ / Ctrl + I"),
                    ("Settings", "⌘ / Ctrl + ,"),
                ] {
                    ui.label(label);
                    ui.label(RichText::new(key).color(p.muted));
                    ui.end_row();
                }
            });
    }
    fn about_tab(&mut self, ui: &mut Ui, p: Palette) {
        let id = egui::Id::new("about-icon");
        let texture = ui
            .ctx()
            .data(|d| d.get_temp::<egui::TextureHandle>(id))
            .unwrap_or_else(|| {
                let img = image::load_from_memory(include_bytes!("../assets/icon-256.png"))
                    .expect("bundled app icon")
                    .into_rgba8();
                let texture = ui.ctx().load_texture(
                    "about-icon",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [img.width() as usize, img.height() as usize],
                        img.as_raw(),
                    ),
                    egui::TextureOptions::LINEAR,
                );
                ui.ctx().data_mut(|d| d.insert_temp(id, texture.clone()));
                texture
            });
        ui.horizontal(|ui| {
            ui.add(egui::Image::new((texture.id(), vec2(72., 72.))));
            ui.add_space(8.);
            ui.vertical(|ui| {
                ui.add_space(10.);
                ui.label(RichText::new("Post Ronda").size(20.).strong());
                ui.label(
                    RichText::new("A native HTTP client. Local collections, no account.")
                        .size(12.5)
                        .color(p.muted),
                );
            });
        });
        ui.add_space(24.);
        egui::Grid::new("about-info")
            .spacing([48., 12.])
            .show(ui, |ui| {
                ui.label("Version");
                ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).color(p.muted));
                ui.end_row();
                ui.label("Platform");
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        std::env::consts::OS,
                        std::env::consts::ARCH
                    ))
                    .color(p.muted),
                );
                ui.end_row();
            });
        ui.add_space(12.);
        ui.label("App data");
        ui.add_space(2.);
        ui.add(
            egui::Label::new(
                RichText::new(crate::state_dir().display().to_string()).color(p.muted),
            )
            .wrap(),
        );
        ui.add_space(26.);
        ui.label("Software updates");
        ui.add_space(6.);
        if switch(
            ui,
            &mut self.prefs.auto_update,
            "Check for updates when the app starts",
            p,
        ) {
            self.store_preferences(ui.ctx());
        }
        ui.add_space(10.);
        ui.label(RichText::new("Update address").size(12.5));
        ui.add_space(3.);
        let changed = ui
            .add(
                TextEdit::singleline(&mut self.prefs.update_url)
                    .hint_text("https://github.com/owner/repo")
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(8, 7)),
            )
            .changed();
        if changed {
            self.update_state = crate::UpdateState::Idle;
            self.store_preferences(ui.ctx());
        }
        ui.add_space(2.);
        ui.add(
            egui::Label::new(
                RichText::new(
                    "A GitHub repository (its latest release is used), or any address that returns JSON such as {\"version\": \"1.2.3\", \"url\": \"https://…/Post-Ronda-macos.zip\"}.",
                )
                .size(12.)
                .color(p.muted),
            )
            .wrap(),
        );
        ui.add_space(8.);
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            let checking = matches!(self.update_state, crate::UpdateState::Checking);
            if ui
                .add_enabled(
                    !checking,
                    egui::Button::new("Check now").min_size(vec2(96., 30.)),
                )
                .on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
                .clicked()
            {
                self.check_updates(&ctx, true);
            }
            if self.prefs.update_url != post_ronda::update::DEFAULT_FEED
                && ui.link("Use the default").clicked()
            {
                self.prefs.update_url = post_ronda::update::DEFAULT_FEED.into();
                self.update_state = crate::UpdateState::Idle;
                self.store_preferences(&ctx);
            }
        });
        ui.add_space(6.);
        let mut download = None;
        match &self.update_state {
            crate::UpdateState::Idle => {}
            crate::UpdateState::Checking => {
                ui.label(RichText::new("Checking…").size(12.5).color(p.muted));
            }
            crate::UpdateState::UpToDate => {
                ui.label(
                    RichText::new(format!(
                        "You are up to date (v{}).",
                        env!("CARGO_PKG_VERSION")
                    ))
                    .size(12.5)
                    .color(p.green),
                );
            }
            crate::UpdateState::Failed(why) => {
                ui.add(egui::Label::new(RichText::new(why).size(12.5).color(p.red)).wrap());
            }
            crate::UpdateState::Available(release) => {
                ui.label(
                    RichText::new(format!(
                        "Version {} is available.",
                        release.version.trim_start_matches('v')
                    ))
                    .size(13.)
                    .color(p.gold),
                );
                if !release.notes.trim().is_empty() {
                    let notes: String = release.notes.trim().chars().take(600).collect();
                    ui.add(egui::Label::new(RichText::new(notes).size(12.).color(p.muted)).wrap());
                }
                ui.add_space(6.);
                ui.horizontal(|ui| {
                    if let Some(url) = release.download_for_this_computer()
                        && ui
                            .add(egui::Button::new("Download").min_size(vec2(96., 30.)))
                            .clicked()
                    {
                        download = Some(url);
                    }
                    if !release.page.is_empty() && ui.link("Release page").clicked() {
                        download = Some(release.page.clone());
                    }
                });
            }
        }
        if let Some(url) = download {
            self.open_url(&url);
        }
        ui.add_space(22.);
        ui.label("Telemetry");
        ui.add_space(4.);
        ui.label(
            RichText::new("None. The only network traffic is the requests you send and the Git commands you run.")
                .size(12.)
                .color(p.muted),
        );
        ui.add_space(26.);
        ui.label(
            RichText::new("Uses the Inter typeface (SIL OFL) and Tabler icons (MIT).")
                .size(11.5)
                .color(p.muted),
        );
    }
    /// Settings of the open project: name, variables, headers, defaults and Git.
    fn project_settings_dialog(&mut self, ctx: &egui::Context) {
        let p = self.palette();
        let open = self.project_settings;
        let title = format!("Project settings: {}", self.collection.name);
        let close = modal(ctx, "project-settings", open, 880., &title, p, |ui| {
            let labels = [
                ("Settings", Icon::Settings),
                ("Documentation", Icon::Book),
                ("Variables", Icon::Code),
                ("Headers", Icon::Dots),
                ("This request", Icon::Send),
                ("Git", Icon::Git),
            ];
            let mut tab = self.collection_tab.min(labels.len() - 1);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(vec2(160., 480.), Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.;
                    Self::settings_nav(ui, &labels, &mut tab, p);
                });
                ui.add_space(8.);
                self.collection_tab = tab;
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), 480.),
                    Layout::top_down(Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("project-settings-scroll")
                            .max_height(480.)
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.project_tab(ui, p));
                    },
                );
            });
        });
        if close {
            self.project_settings = false;
        }
    }
    fn project_tab(&mut self, ui: &mut Ui, p: Palette) {
        match self.collection_tab {
            0 => {
                ui.label(RichText::new("PROJECT SETTINGS").size(12.).color(p.muted));
                ui.add_space(20.);
                ui.label("Name");
                if ui
                    .add(TextEdit::singleline(&mut self.collection.name).desired_width(420.))
                    .changed()
                {
                    self.changed();
                }
                ui.add_space(18.);
                ui.label("Location");
                ui.label(RichText::new(self.project.display().to_string()).color(p.muted));
                ui.add_space(24.);
                ui.label("Defaults for new requests");
                ui.add_space(10.);
                let mut changed = ui
                    .add(
                        egui::DragValue::new(&mut self.collection.default_timeout)
                            .range(1..=300)
                            .suffix(" seconds timeout"),
                    )
                    .changed();
                changed |= ui
                    .checkbox(&mut self.collection.follow_redirects, "Follow redirects")
                    .changed();
                let mut tls = !self.collection.insecure;
                changed |= ui
                    .checkbox(&mut tls, "SSL/TLS Certificate Verification")
                    .changed();
                self.collection.insecure = !tls;
                if changed {
                    self.changed();
                }
                ui.add_space(22.);
                ui.horizontal(|ui| {
                    if ui.button("Export collection").clicked() {
                        self.export_file(false);
                    }
                    if ui.button("Generate documentation").clicked() {
                        self.export_file(true);
                    }
                });
            }
            1 => {
                ui.label("Collection Documentation");
                ui.add_space(12.);
                if ui
                    .add_sized(
                        vec2(ui.available_width(), 360.),
                        TextEdit::multiline(&mut self.collection.description)
                            .frame(egui::Frame::NONE)
                            .hint_text("Document this collection using Markdown…"),
                    )
                    .changed()
                {
                    self.changed();
                }
            }
            2 => {
                ui.label("Collection Variables");
                ui.add_space(14.);
                if variable_table(
                    ui,
                    &mut self.collection.variables,
                    &mut self.draft_header,
                    p,
                ) {
                    self.changed();
                }
                ui.add_space(10.);
                ui.checkbox(
                    &mut self.show_secret,
                    "Show values of secrets (names with token, key, password…)",
                );
            }
            3 => {
                ui.label("Collection Headers");
                ui.add_space(14.);
                if header_table(
                    ui,
                    &mut self.collection.headers,
                    &mut self.draft_header,
                    p,
                    true,
                    None,
                    false,
                ) {
                    self.changed();
                }
                ui.add_space(14.);
                ui.label(RichText::new("Inherited by every request. A request header with the same name overrides the collection header.").color(p.muted));
            }
            4 => {
                ui.label("Request name / folder path");
                if ui
                    .add(
                        TextEdit::singleline(&mut self.collection.requests[self.selected].name)
                            .desired_width(480.),
                    )
                    .changed()
                {
                    self.changed();
                }
                ui.add_space(20.);
                let req = &mut self.collection.requests[self.selected];
                let mut changed = ui
                    .add(
                        egui::DragValue::new(&mut req.timeout)
                            .range(1..=300)
                            .suffix(" seconds timeout"),
                    )
                    .changed();
                changed |= ui
                    .checkbox(&mut req.follow_redirects, "Follow redirects")
                    .changed();
                let mut tls = !req.insecure;
                changed |= ui
                    .checkbox(&mut tls, "SSL/TLS Certificate Verification")
                    .changed();
                req.insecure = !tls;
                if changed {
                    self.changed();
                }
                ui.add_space(20.);
                if ui.button("Delete request…").clicked() {
                    self.confirm_delete = true;
                }
            }

            _ => {
                ui.label(RichText::new("GIT").size(12.).color(p.muted));
                ui.add_space(8.);
                ui.label(RichText::new("This project is its own Git repository: its collection and environments are versioned in the project folder.").size(12.).color(p.muted));
                ui.add_space(20.);
                ui.label("Remote URL (origin)");
                ui.add_space(8.);
                ui.add(
                    TextEdit::singleline(&mut self.remote)
                        .desired_width(f32::INFINITY)
                        .hint_text("git@github.com:you/collection.git"),
                );
                ui.add_space(12.);
                let ctx = ui.ctx().clone();
                ui.horizontal_wrapped(|ui| {
                    let repo = crate::git_branch(&self.project);
                    match &repo {
                        Some(b) => {
                            ui.label(RichText::new(format!("On branch {b}")).color(p.muted));
                        }
                        None => {
                            if ui.button("Initialize Git").clicked() {
                                self.git("init", &ctx);
                            }
                        }
                    }
                    if ui.button("Set origin").clicked() {
                        self.git("remote", &ctx);
                    }
                    if ui.button("Pull").clicked() {
                        self.git("pull", &ctx);
                    }
                    if ui.button("Commit & Push").clicked() {
                        self.git("push", &ctx);
                    }
                });
                ui.add_space(20.);
                ui.label(
                    RichText::new("Uses your local Git credentials.")
                        .size(12.)
                        .color(p.muted),
                );
            }
        }
    }
    fn environments_view(&mut self, ui: &mut Ui, r: Rect) {
        let p = self.palette();
        let nav = Rect::from_min_size(r.min + vec2(12., 16.), vec2(200., r.height() - 32.));
        region(ui, nav, "environment-list", |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Environments").strong());
                if ui.small_button("+").clicked() {
                    self.environments.items.push(Environment {
                        name: format!("Environment {}", self.environments.items.len() + 1),
                        variables: vec![],
                    });
                    self.editing_environment = Some(self.environments.items.len() - 1);
                    self.changed();
                }
            });
            ui.add_space(14.);
            for i in 0..self.environments.items.len() {
                let label = format!(
                    "{}{}",
                    self.environments.items[i].name,
                    if self.environments.active == Some(i) {
                        "  •"
                    } else {
                        ""
                    }
                );
                if ui
                    .selectable_label(self.editing_environment == Some(i), label)
                    .clicked()
                {
                    self.editing_environment = Some(i);
                }
            }
            ui.add_space(20.);
            if ui.link("Import environment…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("JSON", &["json"])
                    .pick_file()
            {
                let result = std::fs::read_to_string(path)
                    .map_err(|e| e.to_string())
                    .and_then(|s| Environment::from_json(&s));
                match result {
                    Ok(e) => {
                        self.environments.items.push(e);
                        self.editing_environment = Some(self.environments.items.len() - 1);
                        self.changed();
                    }
                    Err(e) => self.notify(Err(e)),
                }
            }
        });
        let content = Rect::from_min_max(r.min + vec2(236., 20.), r.max - vec2(24., 20.));
        region(ui, content, "environment-editor", |ui| {
            let Some(i) = self
                .editing_environment
                .filter(|&i| i < self.environments.items.len())
            else {
                ui.heading("Environments");
                ui.add_space(16.);
                ui.label("Create an environment to use {{variables}} in your requests.");
                ui.add_space(8.);
                ui.label(
                    RichText::new(
                        "Select an environment in the top-right dropdown before sending a request.",
                    )
                    .color(p.muted),
                );
                return;
            };
            let mut changed = ui
                .add(
                    TextEdit::singleline(&mut self.environments.items[i].name)
                        .font(egui::TextStyle::Heading)
                        .frame(egui::Frame::NONE)
                        .desired_width(380.),
                )
                .changed();
            ui.add_space(18.);
            ui.horizontal(|ui|{if ui.button(if self.environments.active==Some(i){"Active environment"}else{"Use this environment"}).clicked(){self.environments.active=Some(i);changed=true;}
                if ui.button("Export").clicked()&&let Some(path)=rfd::FileDialog::new().set_file_name("environment.json").save_file(){let e=&self.environments.items[i];let data=serde_json::json!({"name":e.name,"values":e.variables.iter().map(|v|serde_json::json!({"key":v.name,"value":v.value,"enabled":v.enabled})).collect::<Vec<_>>(),"_postman_variable_scope":"environment"});self.notify(std::fs::write(path,serde_json::to_vec_pretty(&data).unwrap()).map(|_|"Environment exported".into()).map_err(|e|e.to_string()));}
            });
            ui.add_space(20.);
            changed |= variable_table(
                ui,
                &mut self.environments.items[i].variables,
                &mut self.draft_header,
                p,
            );
            ui.add_space(10.);
            ui.checkbox(
                &mut self.show_secret,
                "Show values of secrets (names with token, key, password…)",
            );
            ui.add_space(8.);
            ui.label(RichText::new("Environment variables override collection variables. Values are stored locally in environments.json.").size(12.).color(p.muted));
            if changed {
                self.changed();
            }
        });
    }
    fn search_palette(&mut self, ctx: &egui::Context, fade: f32) {
        let p = self.palette();
        // While fading out the palette stays drawn but no longer takes input.
        let live = self.search_open;
        // Reopening, or typing, starts the list at the top again.
        let mut scroll_to_top = live && !self.palette_ready;
        if live && !self.palette_ready {
            self.palette_ready = true;
            self.palette_query.clear();
            self.palette_sel = 0;
        }
        struct Hit {
            title: String,
            subtitle: String,
            badge: String,
            color: Color32,
            icon: Icon,
            target: PaletteTarget,
        }
        let query = self.palette_query.trim().to_lowercase();
        if self.palette_prev_query != self.palette_query {
            self.palette_prev_query = self.palette_query.clone();
            scroll_to_top = true;
        }
        let matches = |s: &str| query.is_empty() || s.to_lowercase().contains(&query);
        let mut hits = Vec::new();
        if matches("documentation docs collection documentation") {
            hits.push(Hit {
                title: "Documentation".into(),
                subtitle: "Read and edit this collection's documentation".into(),
                badge: "DOCUMENTATION".into(),
                color: p.blue,
                icon: Icon::Book,
                target: PaletteTarget::Docs,
            });
        }
        if matches(&format!(
            "{} {}",
            self.collection.name, self.collection.description
        )) {
            hits.push(Hit {
                title: self.collection.name.clone(),
                subtitle: format!("{} requests", self.collection.requests.len()),
                badge: "COLLECTION".into(),
                color: p.blue,
                icon: Icon::Cube,
                target: PaletteTarget::Collection,
            });
        }
        for (i, r) in self.collection.requests.iter().enumerate() {
            if !self.drafts.contains(&i)
                && matches(&format!(
                    "{} {} {} {}",
                    r.method, r.name, r.url, r.description
                ))
            {
                hits.push(Hit {
                    title: r.name.rsplit(" / ").next().unwrap_or(&r.name).to_owned(),
                    subtitle: if r.url.is_empty() {
                        r.method.clone()
                    } else {
                        r.url.clone()
                    },
                    badge: r.method.clone(),
                    color: style::method_color(&r.method, p),
                    icon: Icon::Send,
                    target: PaletteTarget::Request(i),
                });
            }
        }
        let (up, down, enter, esc) = if live {
            ctx.input_mut(|i| {
                let none = egui::Modifiers::NONE;
                (
                    i.consume_key(none, egui::Key::ArrowUp),
                    i.consume_key(none, egui::Key::ArrowDown),
                    i.consume_key(none, egui::Key::Enter),
                    i.consume_key(none, egui::Key::Escape),
                )
            })
        } else {
            (false, false, false, false)
        };
        let last = hits.len().saturating_sub(1);
        let before = self.palette_sel;
        if up {
            self.palette_sel = self.palette_sel.saturating_sub(1);
        }
        if down {
            self.palette_sel += 1;
        }
        self.palette_sel = self.palette_sel.min(last);
        let keyed = self.palette_sel != before;
        let mut selected = self.palette_sel;
        let mut picked = enter
            .then(|| hits.get(selected).map(|h| h.target))
            .flatten();
        let mut close = esc;
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::new("palette-backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, res) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter()
                    .rect_filled(rect, 0, Color32::from_black_alpha((120. * fade) as u8));
                close |= live && res.clicked();
            });
        let width = (screen.width() - 48.).clamp(320., 600.);
        egui::Area::new(egui::Id::new("palette"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(
                screen.center().x - width / 2.,
                screen.top() + 56. - 12. * (1. - fade),
            ))
            .show(ctx, |ui| {
                ui.set_opacity(fade);
                egui::Frame::new()
                    .fill(p.bg)
                    .stroke(Stroke::new(1., p.border))
                    .corner_radius(8)
                    .shadow(egui::Shadow {
                        offset: [0, 10],
                        blur: 28,
                        spread: 0,
                        color: Color32::from_black_alpha(140),
                    })
                    .show(ui, |ui| {
                        let inner = width - 2.;
                        ui.set_width(inner);
                        ui.spacing_mut().item_spacing = vec2(0., 0.);
                        let (rect, _) = ui.allocate_exact_size(vec2(inner, 62.), Sense::hover());
                        let field = rect.shrink2(vec2(12., 11.));
                        ui.painter().rect_stroke(
                            field,
                            5,
                            Stroke::new(1., p.gold),
                            StrokeKind::Inside,
                        );
                        style::icon_sized(
                            ui,
                            Rect::from_min_size(
                                pos2(field.left() + 10., field.center().y - 10.),
                                vec2(20., 20.),
                            ),
                            Icon::Search,
                            p.muted,
                            18.,
                        );
                        let edit = ui.put(
                            Rect::from_min_max(
                                field.min + vec2(40., 3.),
                                field.max - vec2(10., 3.),
                            ),
                            TextEdit::singleline(&mut self.palette_query)
                                .frame(egui::Frame::NONE)
                                .hint_text("Search collections, requests, or documentation…")
                                .font(FontId::proportional(14.))
                                .desired_width(f32::INFINITY)
                                .vertical_align(Align::Center),
                        );
                        if live {
                            edit.request_focus();
                        }
                        line(ui, rect.left_bottom(), rect.right_bottom(), p);
                        let moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
                        let mut results = egui::ScrollArea::vertical()
                            .max_height(54. * 6. + 8.)
                            .auto_shrink([false, true]);
                        if scroll_to_top {
                            results = results.vertical_scroll_offset(0.);
                        }
                        results.show(ui, |ui| {
                            ui.add_space(4.);
                            if hits.is_empty() {
                                let (r, _) =
                                    ui.allocate_exact_size(vec2(inner, 64.), Sense::hover());
                                ui.painter().text(
                                    r.center(),
                                    Align2::CENTER_CENTER,
                                    "No results found",
                                    FontId::proportional(13.),
                                    p.muted,
                                );
                            }
                            for (i, hit) in hits.iter().enumerate() {
                                let (row, res) =
                                    ui.allocate_exact_size(vec2(inner, 54.), Sense::click());
                                let res =
                                    res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
                                let row = row.shrink2(vec2(6., 1.));
                                if res.hovered() && moved {
                                    selected = i;
                                }
                                if live && res.clicked() {
                                    picked = Some(hit.target);
                                }
                                if i == selected {
                                    ui.painter().rect_filled(row, 5, p.selected);
                                    if keyed {
                                        ui.scroll_to_rect(row, None);
                                    }
                                }
                                style::icon_sized(
                                    ui,
                                    Rect::from_min_size(
                                        pos2(row.left() + 12., row.center().y - 10.),
                                        vec2(20., 20.),
                                    ),
                                    hit.icon,
                                    p.muted,
                                    18.,
                                );
                                let badge = ui.painter().layout_no_wrap(
                                    hit.badge.clone(),
                                    FontId::proportional(10.),
                                    hit.color,
                                );
                                let badge_rect = Rect::from_center_size(
                                    pos2(
                                        row.right() - 12. - (badge.size().x + 16.) / 2.,
                                        row.center().y,
                                    ),
                                    vec2(badge.size().x + 16., 22.),
                                );
                                ui.painter().rect(
                                    badge_rect,
                                    4,
                                    hit.color.gamma_multiply(0.14),
                                    Stroke::new(1., hit.color.gamma_multiply(0.55)),
                                    StrokeKind::Inside,
                                );
                                let galley_pos = badge_rect.center() - badge.size() / 2.;
                                ui.painter().galley(galley_pos, badge, hit.color);
                                let clip = Rect::from_min_max(
                                    pos2(row.left() + 46., row.top()),
                                    pos2(badge_rect.left() - 12., row.bottom()),
                                );
                                let painter = ui.painter().with_clip_rect(clip);
                                painter.text(
                                    pos2(clip.left(), row.center().y - 9.),
                                    Align2::LEFT_CENTER,
                                    &hit.title,
                                    FontId::proportional(13.5),
                                    p.text,
                                );
                                painter.text(
                                    pos2(clip.left(), row.center().y + 10.),
                                    Align2::LEFT_CENTER,
                                    &hit.subtitle,
                                    FontId::proportional(12.),
                                    p.muted,
                                );
                            }
                            ui.add_space(4.);
                        });
                        let (foot, _) = ui.allocate_exact_size(vec2(inner, 40.), Sense::hover());
                        line(ui, foot.left_top(), foot.right_top(), p);
                        let parts: [(&str, bool); 7] = [
                            ("↑", true),
                            ("↓", true),
                            ("to navigate", false),
                            ("↵", true),
                            ("to select", false),
                            ("esc", true),
                            ("to close", false),
                        ];
                        let mut items = Vec::new();
                        let mut total = 0.;
                        for (i, (label, chip)) in parts.iter().enumerate() {
                            let galley = ui.painter().layout_no_wrap(
                                (*label).into(),
                                FontId::proportional(11.),
                                p.text,
                            );
                            let w = if *chip {
                                (galley.size().x + 14.).max(24.)
                            } else {
                                galley.size().x
                            };
                            let gap = match (chip, parts.get(i + 1)) {
                                (_, None) => 0.,
                                (true, Some((_, true))) => 6.,
                                (true, Some(_)) => 8.,
                                (false, _) => 22.,
                            };
                            items.push((galley, *chip, total, w));
                            total += w + gap;
                        }
                        let x0 = foot.center().x - total / 2.;
                        for (galley, chip, offset, w) in items {
                            let left = x0 + offset;
                            if chip {
                                let r = Rect::from_center_size(
                                    pos2(left + w / 2., foot.center().y),
                                    vec2(w, 22.),
                                );
                                ui.painter().rect(
                                    r,
                                    4,
                                    p.field,
                                    Stroke::new(1., p.border),
                                    StrokeKind::Inside,
                                );
                                ui.painter().galley(
                                    r.center() - galley.size() / 2.,
                                    galley,
                                    p.text,
                                );
                            } else {
                                ui.painter().galley(
                                    pos2(left, foot.center().y - galley.size().y / 2.),
                                    galley,
                                    p.muted,
                                );
                            }
                        }
                    });
            });
        self.palette_sel = selected;
        if let Some(target) = picked.filter(|_| live) {
            match target {
                PaletteTarget::Docs => self.open_project_settings(1),
                PaletteTarget::Collection => {
                    self.page = 1;
                    self.settings = false;
                }
                PaletteTarget::Request(i) => self.activate(i),
            }
            close = true;
        }
        if close && live {
            self.search_open = false;
            self.palette_ready = false;
        }
    }
    fn codegen_dialog(&mut self, ctx: &egui::Context) {
        let p = self.palette();
        let open = self.codegen_open;
        let base = self.collection.requests[self.selected].clone();
        let variables = self
            .environments
            .active
            .and_then(|i| self.environments.items.get(i))
            .map(|e| e.variables.clone())
            .unwrap_or_default();
        let (request, warning) = if self.codegen_interpolate {
            match post_ronda::environment::resolve(
                &base,
                &self.collection.variables,
                &variables,
                &self.collection.headers,
            ) {
                Ok(r) => (r, None),
                Err(e) => (base, Some(e)),
            }
        } else {
            (base, None)
        };
        let languages = post_ronda::codegen::LANGUAGES;
        let (mut lang, mut client, mut interpolate) = (
            self.codegen_lang,
            self.codegen_client,
            self.codegen_interpolate,
        );
        let mut copied = false;
        let close = modal(ctx, "codegen", open, 720., "Generate Code", p, |ui| {
            ui.horizontal(|ui| {
                let names: Vec<&str> = languages.iter().map(|l| l.0).collect();
                if let Some(choice) =
                    dropdown_field(ui, "codegen-language", languages[lang].0, 160., &names, p)
                {
                    if choice != lang {
                        client = 0;
                    }
                    lang = choice;
                }
                client = client.min(languages[lang].1.len() - 1);
                for (i, name) in languages[lang].1.iter().enumerate() {
                    let font = FontId::proportional(12.5);
                    let width = ui
                        .painter()
                        .layout_no_wrap((*name).into(), font.clone(), p.text)
                        .size()
                        .x;
                    let (rect, res) =
                        ui.allocate_exact_size(vec2(width + 28., 30.), Sense::click());
                    let res = res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
                    let active = client == i;
                    ui.painter().rect(
                        rect,
                        5,
                        if active {
                            p.gold.gamma_multiply(0.18)
                        } else if res.hovered() {
                            p.hover
                        } else {
                            p.bg
                        },
                        Stroke::new(1., if active { p.gold } else { p.border }),
                        StrokeKind::Inside,
                    );
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        name,
                        font,
                        if active { p.gold } else { p.text },
                    );
                    if res.clicked() {
                        client = i;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.checkbox(&mut interpolate, "Interpolate Variables");
                });
            });
            ui.add_space(4.);
            let code = post_ronda::codegen::generate(&request, lang, client);
            let lines = style::highlight_code(&code, languages[lang].0, p);
            let gutter = 14. + 7. * lines.len().to_string().len() as f32;
            let frame = egui::Frame::new()
                .fill(p.field)
                .stroke(Stroke::new(1., p.border))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(8, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::both()
                        .id_salt("codegen-code")
                        .max_height(330.)
                        .auto_shrink([false, true])
                        .show_rows(ui, 18., lines.len(), |ui, range| {
                            ui.spacing_mut().item_spacing.y = 0.;
                            for i in range {
                                let (rect, _) = ui.allocate_exact_size(
                                    vec2(ui.available_width(), 18.),
                                    Sense::hover(),
                                );
                                ui.painter().text(
                                    pos2(rect.left() + gutter - 8., rect.center().y),
                                    Align2::RIGHT_CENTER,
                                    (i + 1).to_string(),
                                    FontId::monospace(11.5),
                                    p.muted,
                                );
                                ui.scope_builder(
                                    egui::UiBuilder::new()
                                        .max_rect(Rect::from_min_max(
                                            pos2(rect.left() + gutter + 8., rect.top()),
                                            rect.max,
                                        ))
                                        .layout(Layout::left_to_right(Align::Center)),
                                    |ui| {
                                        ui.add(
                                            egui::Label::new(lines[i].clone())
                                                .wrap_mode(egui::TextWrapMode::Extend)
                                                .selectable(true),
                                        );
                                    },
                                );
                            }
                        });
                });
            let corner = frame.response.rect;
            if icon_button(
                ui,
                Rect::from_min_size(
                    pos2(corner.right() - 38., corner.top() + 8.),
                    vec2(28., 28.),
                ),
                Icon::Copy,
                "Copy to clipboard",
                p,
            )
            .clicked()
            {
                ui.ctx().copy_text(code.clone());
                copied = true;
            }
            if let Some(warning) = &warning {
                ui.add_space(4.);
                ui.label(
                    RichText::new(format!(
                        "{warning} — showing the request without variables."
                    ))
                    .size(12.)
                    .color(p.red),
                );
            }
        });
        (
            self.codegen_lang,
            self.codegen_client,
            self.codegen_interpolate,
        ) = (lang, client, interpolate);
        if copied {
            self.notify(Ok("Copied to clipboard!".into()));
        }
        if close {
            self.codegen_open = false;
        }
    }
    fn save_dialog(&mut self, ctx: &egui::Context) {
        let p = self.palette();
        let open = self.save_open;
        let mut folders = std::collections::BTreeSet::new();
        let mut add = |path: &str| {
            let parts: Vec<_> = path.split(" / ").collect();
            for end in 1..=parts.len() {
                folders.insert(parts[..end].join(" / "));
            }
        };
        for r in &self.collection.requests {
            if let Some((dir, _)) = r.name.rsplit_once(" / ") {
                add(dir);
            }
        }
        for mark in &self.collection.folders {
            add(&mark.path);
        }
        if !self.save_folder.is_empty() {
            add(&self.save_folder);
        }
        let root = self.collection.name.clone();
        let (mut name, mut folder, mut creating) = (
            self.save_name.clone(),
            self.save_folder.clone(),
            self.save_new_folder.clone(),
        );
        let mut focus = self.save_focus;
        let (mut confirm, mut cancel) = (false, false);
        let close = modal(ctx, "save", open, 500., "Save Request", p, |ui| {
            ui.label(RichText::new("Request name").strong());
            let field = ui.add(
                TextEdit::singleline(&mut name)
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(8, 7)),
            );
            if focus {
                field.request_focus();
                focus = false;
            }
            if creating.is_none() && open && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                confirm = true;
            }
            ui.add_space(8.);
            ui.label(RichText::new("Select a folder to save to").strong());
            egui::Frame::new()
                .stroke(Stroke::new(1., p.border))
                .corner_radius(6)
                .inner_margin(6.)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical()
                        .id_salt("save-folders")
                        .max_height(190.)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            let mut rows = vec![(String::new(), root.clone(), 0)];
                            for f in &folders {
                                let depth = f.matches(" / ").count() + 1;
                                let label = f.rsplit(" / ").next().unwrap_or(f).to_owned();
                                rows.push((f.clone(), label, depth));
                            }
                            for (path, label, depth) in rows {
                                let (rect, res) = ui.allocate_exact_size(
                                    vec2(ui.available_width(), 34.),
                                    Sense::click(),
                                );
                                let res =
                                    res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
                                let selected = folder == path;
                                if selected || res.hovered() {
                                    ui.painter().rect(
                                        rect,
                                        5,
                                        if selected { p.selected } else { p.hover },
                                        Stroke::new(1., if selected { p.gold } else { p.border }),
                                        StrokeKind::Inside,
                                    );
                                }
                                let x = rect.left() + 10. + depth as f32 * 18.;
                                style::icon(
                                    ui,
                                    Rect::from_min_size(
                                        pos2(x, rect.center().y - 9.),
                                        vec2(18., 18.),
                                    ),
                                    if depth == 0 { Icon::Cube } else { Icon::Folder },
                                    p.muted,
                                );
                                text(ui, pos2(x + 28., rect.center().y), &label, 13., p.text);
                                if res.clicked() {
                                    folder = path;
                                }
                            }
                        });
                });
            ui.add_space(6.);
            if let Some(mut new_name) = creating.take() {
                let mut keep = true;
                ui.horizontal(|ui| {
                    let edit = ui.add(
                        TextEdit::singleline(&mut new_name)
                            .hint_text("Folder name")
                            .desired_width(260.),
                    );
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    edit.request_focus();
                    let create = ui.button("Create").clicked() || enter;
                    if create && !new_name.trim().is_empty() {
                        let clean = new_name.trim().replace(" / ", " - ");
                        folder = if folder.is_empty() {
                            clean
                        } else {
                            format!("{folder} / {clean}")
                        };
                        keep = false;
                    } else if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
                if keep {
                    creating = Some(new_name);
                }
            } else {
                ui.horizontal(|ui| {
                    let (rect, res) = ui.allocate_exact_size(vec2(124., 30.), Sense::click());
                    let res = res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
                    style::icon(
                        ui,
                        Rect::from_min_size(rect.min + vec2(2., 6.), vec2(18., 18.)),
                        Icon::Folder,
                        p.gold,
                    );
                    text(
                        ui,
                        pos2(rect.left() + 26., rect.center().y),
                        "New folder",
                        13.,
                        p.gold,
                    );
                    if res.clicked() {
                        creating = Some(String::new());
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Save").color(Color32::BLACK))
                                    .fill(p.gold)
                                    .min_size(vec2(72., 30.)),
                            )
                            .clicked()
                        {
                            confirm = true;
                        }
                        if ui
                            .add(egui::Button::new("Cancel").min_size(vec2(72., 30.)))
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
            }
        });
        (
            self.save_name,
            self.save_folder,
            self.save_new_folder,
            self.save_focus,
        ) = (name, folder, creating, focus);
        if confirm && open {
            self.confirm_save();
        }
        if (close || cancel) && open {
            self.save_open = false;
        }
    }
    fn extra_windows(&mut self, ctx: &egui::Context) {
        let fade = ctx.animate_bool_with_time_and_easing(
            egui::Id::new("palette-fade"),
            self.search_open,
            0.14,
            egui::emath::easing::cubic_out,
        );
        if self.search_open || fade > 0. {
            self.search_palette(ctx, fade);
        }
        self.drag_ghost(ctx);
        self.codegen_dialog(ctx);
        self.save_dialog(ctx);
        self.rename_dialog(ctx);
        self.info_dialog(ctx);
        self.delete_dialog(ctx);
        self.close_dialog(ctx);
        self.import_dialog(ctx);
        self.project_dialog(ctx);
        self.app_settings_dialog(ctx);
        self.project_settings_dialog(ctx);
        self.toast_view(ctx);
    }
}

/// Shared dialog chrome: dimmed backdrop, fade and slide in, Esc or an outside click closes.
/// Returns true when the user asked to close it.
fn modal(
    ctx: &egui::Context,
    id: &'static str,
    open: bool,
    width: f32,
    title: &str,
    p: Palette,
    body: impl FnOnce(&mut Ui),
) -> bool {
    let fade = ctx.animate_bool_with_time_and_easing(
        egui::Id::new((id, "fade")),
        open,
        0.14,
        egui::emath::easing::cubic_out,
    );
    if !open && fade <= 0. {
        return false;
    }
    let mut close =
        open && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new((id, "backdrop")))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, res) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0, Color32::from_black_alpha((120. * fade) as u8));
            close |= open && res.clicked();
        });
    let width = (screen.width() - 48.).clamp(320., width);
    egui::Area::new(egui::Id::new((id, "panel")))
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(
            screen.center().x - width / 2.,
            screen.top() + 56. - 12. * (1. - fade),
        ))
        .show(ctx, |ui| {
            ui.set_opacity(fade);
            egui::Frame::new()
                .fill(p.bg)
                .stroke(Stroke::new(1., p.border))
                .corner_radius(8)
                .shadow(egui::Shadow {
                    offset: [0, 10],
                    blur: 28,
                    spread: 0,
                    color: Color32::from_black_alpha(140),
                })
                .show(ui, |ui| {
                    let inner = width - 2.;
                    ui.set_width(inner);
                    ui.spacing_mut().item_spacing = vec2(8., 8.);
                    let (head, _) = ui.allocate_exact_size(vec2(inner, 46.), Sense::hover());
                    text(
                        ui,
                        pos2(head.left() + 16., head.center().y),
                        title,
                        14.,
                        p.text,
                    );
                    if icon_button(
                        ui,
                        Rect::from_center_size(
                            pos2(head.right() - 24., head.center().y),
                            vec2(26., 26.),
                        ),
                        Icon::Close,
                        "Close",
                        p,
                    )
                    .clicked()
                    {
                        close = open;
                    }
                    line(ui, head.left_bottom(), head.right_bottom(), p);
                    egui::Frame::new().inner_margin(16.).show(ui, |ui| {
                        ui.set_width(inner - 32.);
                        body(ui);
                    });
                });
        });
    close
}
#[derive(Clone, Copy)]
enum PaletteTarget {
    Docs,
    Collection,
    Request(usize),
}
fn tab_background(ui: &Ui, rect: Rect, active: bool, hovered: bool, p: Palette) {
    if active {
        ui.painter().rect(
            rect,
            egui::CornerRadius {
                nw: 7,
                ne: 7,
                sw: 0,
                se: 0,
            },
            p.bg,
            Stroke::new(1., p.border),
            StrokeKind::Inside,
        );
        ui.painter().line_segment(
            [
                pos2(rect.left() + 1., rect.bottom()),
                pos2(rect.right() - 1., rect.bottom()),
            ],
            Stroke::new(2., p.bg),
        );
    } else if hovered {
        ui.painter()
            .rect_filled(rect.shrink2(vec2(0., 3.)), 5, p.hover);
    }
}
/// A tree entry for an explicitly created folder; the trailing empty segment keeps it in the tree.
/// Names usable as `{{name}}` right now (collection plus selected environment), published by
/// the app each frame so any text field can colour variables without being handed the data.
pub const KNOWN_VARIABLES: &str = "known-variables";
/// Variable name -> (value, where it comes from).
pub type Variables = std::sync::Arc<std::collections::HashMap<String, (String, String)>>;
/// Names that look like they hold a secret: their values are not shown in tooltips.
pub fn looks_secret(name: &str) -> bool {
    let name = name.to_lowercase();
    [
        "secret",
        "token",
        "password",
        "passwd",
        "pwd",
        "key",
        "auth",
        "credential",
    ]
    .iter()
    .any(|word| name.contains(word))
}
/// Hovering a `{{variable}}` in a text field says what it stands for right now.
fn variable_tooltip(ui: &Ui, output: &egui::text_edit::TextEditOutput, p: Palette) {
    let response = &output.response;
    let Some(pointer) = response.hover_pos().filter(|_| response.hovered()) else {
        return;
    };
    let text = output.galley.text();
    let char_index = output
        .galley
        .cursor_from_pos(pointer - output.galley_pos)
        .index
        .0;
    let byte = text
        .char_indices()
        .nth(char_index)
        .map_or(text.len(), |(b, _)| b);
    let Some(&(_, _, name)) = variable_spans(text)
        .iter()
        .find(|&&(start, end, _)| start <= byte && byte < end)
    else {
        return;
    };
    let known: Variables = ui
        .ctx()
        .data(|d| d.get_temp(egui::Id::new(KNOWN_VARIABLES)))
        .unwrap_or_default();
    egui::Tooltip::for_widget(response).at_pointer().show(|ui| {
        ui.set_max_width(360.);
        match known.get(name) {
            Some((value, source)) => {
                ui.label(
                    RichText::new(format!("{{{{{name}}}}}"))
                        .strong()
                        .color(p.blue),
                );
                let shown = if looks_secret(name) {
                    "•••••••• (hidden)".to_owned()
                } else {
                    value.clone()
                };
                ui.label(if shown.is_empty() {
                    "(empty)".to_owned()
                } else {
                    shown
                });
                ui.label(RichText::new(source).size(11.).color(p.muted));
            }
            None => {
                ui.label(
                    RichText::new(format!("{{{{{name}}}}}"))
                        .strong()
                        .color(p.red),
                );
                ui.label("Not defined in the selected environment or in the project.");
            }
        }
    });
}
/// `{{name}}` occurrences in `text` as (start, end, name).
fn variable_spans(text: &str) -> Vec<(usize, usize, &str)> {
    let mut spans = vec![];
    let mut at = 0;
    while let Some(open) = text[at..].find("{{") {
        let start = at + open;
        let Some(close) = text[start + 2..].find("}}") else {
            break;
        };
        let end = start + 2 + close + 2;
        spans.push((start, end, text[start + 2..end - 2].trim()));
        at = end;
    }
    spans
}
/// Colours every `{{name}}` in an already formatted job: blue when the name exists in the
/// selected environment or collection, red when it does not. The text itself is untouched.
fn tint_variables(job: egui::text::LayoutJob, ui: &Ui, p: Palette) -> egui::text::LayoutJob {
    let spans = variable_spans(&job.text);
    if spans.is_empty() {
        return job;
    }
    let known: Variables = ui
        .ctx()
        .data(|d| d.get_temp(egui::Id::new(KNOWN_VARIABLES)))
        .unwrap_or_default();
    let style = |name: &str| {
        let color = if known.contains_key(name) {
            p.blue
        } else {
            p.red
        };
        let [r, g, b, _] = color.to_array();
        (color, Color32::from_rgba_unmultiplied(r, g, b, 38))
    };
    let mut tinted = job.clone();
    tinted.sections.clear();
    for section in &job.sections {
        let (start, end) = (section.byte_range.start.0, section.byte_range.end.0);
        let mut first = true;
        let mut push = |from: usize, to: usize, format: egui::TextFormat| {
            if from < to {
                tinted.sections.push(egui::text::LayoutSection {
                    leading_space: if std::mem::take(&mut first) {
                        section.leading_space
                    } else {
                        0.
                    },
                    byte_range: egui::text::ByteIndex(from)..egui::text::ByteIndex(to),
                    format,
                });
            }
        };
        let mut at = start;
        for &(s, e, name) in spans.iter().filter(|&&(s, e, _)| s < end && e > start) {
            let (from, to) = (s.max(start), e.min(end));
            push(at, from, section.format.clone());
            let (color, background) = style(name);
            push(
                from,
                to,
                egui::TextFormat {
                    color,
                    background,
                    ..section.format.clone()
                },
            );
            at = to;
        }
        push(at, end, section.format.clone());
        if section.byte_range.is_empty() {
            push(start, start, section.format.clone());
        }
    }
    tinted
}
/// Layouter for one-line fields (URL, table cells) that colours `{{variables}}`.
fn variable_layouter(
    p: Palette,
    font: FontId,
) -> impl FnMut(&Ui, &dyn egui::TextBuffer, f32) -> std::sync::Arc<egui::Galley> {
    move |ui, text, _| {
        let job = egui::text::LayoutJob::simple_singleline(
            text.as_str().to_owned(),
            font.clone(),
            ui.visuals().text_color(),
        );
        let job = tint_variables(job, ui, p);
        ui.fonts_mut(|f| f.layout_job(job))
    }
}
/// Dragging a text selection past the edge of a scroll area scrolls it, like in other apps: the
/// further the pointer is beyond the edge, the faster. Only a press that began inside the area
/// (and not on its scroll bar) counts, so other drags never move it.
fn drag_autoscroll<R>(ui: &Ui, mut out: egui::scroll_area::ScrollAreaOutput<R>) {
    let (down, origin, pos) = ui.input(|i| {
        (
            i.pointer.primary_down(),
            i.pointer.press_origin(),
            i.pointer.latest_pos(),
        )
    });
    let area = out.inner_rect;
    let (true, Some(origin), Some(pos)) = (down, origin, pos) else {
        return;
    };
    if !area.contains(origin) || origin.x > area.right() - 14. || origin.y > area.bottom() - 14. {
        return;
    }
    let step = |at: f32, low: f32, high: f32| {
        let zone = 22.;
        let over = if at > high - zone {
            at - (high - zone)
        } else if at < low + zone {
            -((low + zone) - at)
        } else {
            return 0.;
        };
        (over.abs() * 0.3 + 2.).min(45.) * over.signum()
    };
    let delta = vec2(
        step(pos.x, area.left(), area.right()),
        step(pos.y, area.top(), area.bottom()),
    );
    if delta == egui::Vec2::ZERO {
        return;
    }
    let limit = (out.content_size - area.size()).max(egui::Vec2::ZERO);
    out.state.offset = (out.state.offset + delta).clamp(egui::Vec2::ZERO, limit);
    out.state.store(ui.ctx(), out.id);
    ui.ctx().request_repaint();
}
/// A variable name being typed after `{{`, and the names that fit it.
struct Completion {
    /// Byte positions of the `{{` and of the caret in the URL.
    open: usize,
    caret: usize,
    names: Vec<String>,
}
/// A (line, column) position in the response text; columns count characters.
type TextPos = (usize, usize);
/// Lays out one response line; the same galley is used to paint it and to find the character
/// under the pointer.
fn code_galley(
    ui: &Ui,
    mut job: egui::text::LayoutJob,
    wrap: Option<f32>,
) -> std::sync::Arc<egui::Galley> {
    match wrap {
        Some(width) => {
            job.wrap.max_width = width;
            job.wrap.break_anywhere = true;
        }
        None => job.wrap.max_width = f32::INFINITY,
    }
    ui.fonts_mut(|f| f.layout_job(job))
}
/// Paints a response line with the part of `selection` that falls on it highlighted.
#[allow(clippy::too_many_arguments)]
fn paint_code_line(
    ui: &Ui,
    pos: egui::Pos2,
    galley: &std::sync::Arc<egui::Galley>,
    line: usize,
    length: usize,
    selection: Option<(TextPos, TextPos)>,
    finds: &[(usize, usize, usize)],
    current: Option<(usize, usize, usize)>,
    p: Palette,
) {
    let mut painted = galley.clone();
    // Matches of the find bar: all in yellow, the current one in orange.
    for find in finds {
        let mut visuals = ui.visuals().clone();
        visuals.selection.bg_fill = if current == Some(*find) {
            Color32::from_rgba_unmultiplied(240, 140, 40, 170)
        } else {
            Color32::from_rgba_unmultiplied(235, 200, 60, 90)
        };
        egui::text_selection::visuals::paint_text_selection(
            &mut painted,
            &visuals,
            &egui::text_selection::CCursorRange::two(
                egui::text::CCursor::new(find.1),
                egui::text::CCursor::new(find.2),
            ),
            None,
        );
    }
    if let Some((a, b)) = selection {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        if (start.0..=end.0).contains(&line) {
            let from = if line == start.0 { start.1 } else { 0 };
            let to = if line == end.0 { end.1 } else { length };
            if from < to {
                egui::text_selection::visuals::paint_text_selection(
                    &mut painted,
                    ui.visuals(),
                    &egui::text_selection::CCursorRange::two(
                        egui::text::CCursor::new(from),
                        egui::text::CCursor::new(to),
                    ),
                    None,
                );
            }
        }
    }
    ui.painter().galley(pos, painted, p.text);
}
/// The find matches on one line, from the sorted list of all matches.
fn finds_on(matches: &[(usize, usize, usize)], line: usize) -> &[(usize, usize, usize)] {
    let from = matches.partition_point(|m| m.0 < line);
    let to = matches.partition_point(|m| m.0 <= line);
    &matches[from..to]
}
/// The word around `col`, as a column range (a run of letters/digits/underscores, or of
/// anything else).
fn word_range(text: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let col = col.min(chars.len() - 1);
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let kind = word(chars[col]);
    let mut start = col;
    while start > 0 && word(chars[start - 1]) == kind {
        start -= 1;
    }
    let mut end = col + 1;
    while end < chars.len() && word(chars[end]) == kind {
        end += 1;
    }
    (start, end)
}
/// What the pointer is doing to the response text this frame.
struct CodePointer {
    /// Press that began inside the viewer: 1 single, 2 double, 3 triple click.
    pressed: Option<u8>,
    down: bool,
    pos: Option<egui::Pos2>,
    clip: Rect,
    /// Character under the pointer, found while the visible lines are drawn.
    hit: Option<TextPos>,
    first: Option<usize>,
    last: Option<(usize, usize)>,
}
impl CodePointer {
    fn new(ui: &Ui) -> Self {
        let clip = ui.clip_rect();
        let (pressed, down, pos) = ui.input(|i| {
            let pos = i.pointer.latest_pos();
            let inside = pos.is_some_and(|p| clip.contains(p) && p.x < clip.right() - 14.);
            let button = egui::PointerButton::Primary;
            let pressed = (i.pointer.button_pressed(button) && inside).then(|| {
                if i.pointer.button_triple_clicked(button) {
                    3
                } else if i.pointer.button_double_clicked(button) {
                    2
                } else {
                    1
                }
            });
            (pressed, i.pointer.button_down(button), pos)
        });
        if pos.is_some_and(|p| clip.contains(p)) {
            ui.set_cursor_icon(egui::CursorIcon::Text);
        }
        Self {
            pressed,
            down,
            pos,
            clip,
            hit: None,
            first: None,
            last: None,
        }
    }
    /// Called for each drawn line with the vertical extent of its rows.
    fn visit(
        &mut self,
        line: usize,
        length: usize,
        galley: &egui::Galley,
        origin: egui::Pos2,
        rows: std::ops::Range<f32>,
    ) {
        self.first.get_or_insert(line);
        self.last = Some((line, length));
        if let Some(pos) = self.pos
            && rows.contains(&pos.y)
        {
            let col = galley.cursor_from_pos(pos - origin).index.0;
            self.hit = Some((line, col.min(length)));
        }
    }
    /// The position to extend the selection to; beyond the top or bottom it is the start or end
    /// of the first or last drawn line.
    fn target(&self) -> Option<TextPos> {
        let pos = self.pos?;
        self.hit.or_else(|| {
            if pos.y < self.clip.top() {
                self.first.map(|line| (line, 0))
            } else {
                self.last
            }
        })
    }
}
fn mark_entry(mark: &post_ronda::collection::FolderMark) -> (Option<usize>, Vec<&str>) {
    let mut parts: Vec<&str> = mark.path.split(" / ").collect();
    parts.push("");
    (None, parts)
}
/// Entries of one tree level in display order: first appearance, with pinned items first.
fn ordered_slots(
    entries: &[(Option<usize>, Vec<&str>)],
    depth: usize,
    requests: &[Request],
    pinned: &[String],
) -> Vec<usize> {
    let mut seen: Vec<&str> = vec![];
    let mut slots: Vec<(bool, usize)> = vec![];
    for (k, (index, path)) in entries.iter().enumerate() {
        if path.len() > depth + 1 {
            let folder = path[depth];
            if seen.contains(&folder) {
                continue;
            }
            seen.push(folder);
            slots.push((pinned.contains(&path[..=depth].join(" / ")), k));
        } else if let Some(i) = index {
            slots.push((pinned.iter().any(|p| *p == requests[*i].name), k));
        }
    }
    // Stable, so pinned items keep their relative order and so do the rest.
    slots.sort_by_key(|(pin, _)| !pin);
    slots.into_iter().map(|(_, k)| k).collect()
}
/// 0 for the top third of a row, 2 for the bottom third, 1 for the middle.
fn pointer_zone(ui: &Ui, rect: Rect) -> u8 {
    let Some(pos) = ui.input(|i| i.pointer.interact_pos()) else {
        return 1;
    };
    let t = (pos.y - rect.top()) / rect.height().max(1.);
    if t < 0.3 {
        0
    } else if t > 0.7 {
        2
    } else {
        1
    }
}
fn tree_rows(
    ui: &mut Ui,
    entries: &[(Option<usize>, Vec<&str>)],
    depth: usize,
    requests: &[Request],
    selected: usize,
    pinned: &[String],
    p: Palette,
) -> Option<RowAction> {
    let mut picked = None;
    for k in ordered_slots(entries, depth, requests, pinned) {
        let (index, path) = &entries[k];
        if path.len() > depth + 1 {
            let folder = path[depth];
            let full = path[..=depth].join(" / ");
            let folder_pinned = pinned.contains(&full);
            let children: Vec<_> = entries
                .iter()
                .filter(|(_, p)| p.len() > depth + 1 && p[depth] == folder)
                .cloned()
                .collect();
            // Open state is kept here (not by the header): the drag handle over the row takes
            // the clicks, so the header could never toggle itself.
            let open_key = egui::Id::new(("folder-open", &full));
            let open: bool = ui
                .ctx()
                .data_mut(|d| d.get_persisted(open_key))
                .unwrap_or(true);
            let header = egui::CollapsingHeader::new(RichText::new(folder).size(13.))
                .id_salt(("folder", &full))
                .icon(move |ui, open, res| style::chevron(ui, res.rect, open, p.muted))
                .open(Some(open))
                .show(ui, |ui| {
                    if let Some(action) =
                        tree_rows(ui, &children, depth + 1, requests, selected, pinned, p)
                    {
                        picked = Some(action);
                    }
                });
            let hr = &header.header_response;
            let row = Rect::from_min_max(
                pos2(ui.max_rect().left(), hr.rect.top()),
                pos2(ui.max_rect().right(), hr.rect.bottom()),
            );
            // Drag handle: the row except the "⋯" button, which sits on top of it.
            let grab = ui.interact(
                Rect::from_min_max(row.min, pos2(row.right() - 34., row.bottom())),
                ui.id().with(("folder-drag", &full)),
                Sense::click_and_drag(),
            );
            if grab.clicked() {
                ui.ctx().data_mut(|d| d.insert_persisted(open_key, !open));
                ui.request_repaint();
            }
            let grab = grab.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
            if grab.drag_started() {
                grab.dnd_set_drag_payload(Dragged::Folder(full.clone()));
            }
            // Edges of a folder row mean "next to it"; the middle means "inside it".
            let zone = pointer_zone(ui, row);
            if let Some(payload) = grab.dnd_hover_payload::<Dragged>()
                && *payload != Dragged::Folder(full.clone())
            {
                match zone {
                    0 => ui
                        .painter()
                        .line_segment([row.left_top(), row.right_top()], Stroke::new(2., p.gold)),
                    2 => ui.painter().line_segment(
                        [row.left_bottom(), row.right_bottom()],
                        Stroke::new(2., p.gold),
                    ),
                    _ => ui.painter().rect_stroke(
                        row,
                        4,
                        Stroke::new(1.5, p.gold),
                        StrokeKind::Inside,
                    ),
                };
            }
            if let Some(payload) = grab.dnd_release_payload::<Dragged>() {
                let target = match zone {
                    0 => DropTarget::BeforeFolder(full.clone()),
                    2 => DropTarget::AfterFolder(full.clone()),
                    _ => DropTarget::IntoFolder(full.clone()),
                };
                picked = Some(RowAction::Move((*payload).clone(), target));
            }
            if let Some(kind) = header_menu(ui, hr, &full, false, folder_pinned, p) {
                picked = Some(RowAction::Folder(full, kind));
            }
        } else {
            // Explicitly created folders that are still empty have no request to draw.
            let Some(i) = index else {
                continue;
            };
            let is_pinned = pinned.iter().any(|x| *x == requests[*i].name);
            let (rect, res) =
                ui.allocate_exact_size(vec2(ui.available_width(), 26.), Sense::click_and_drag());
            let res = res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
            let menu_rect =
                Rect::from_center_size(pos2(rect.right() - 16., rect.center().y), vec2(24., 22.));
            let menu_btn = ui
                .interact(menu_rect, ui.id().with(("row-menu", *i)), Sense::click())
                .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
            let menu_open =
                egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&menu_btn));
            let hot = ui.rect_contains_pointer(rect) || menu_open;
            if selected == *i || hot {
                ui.painter().rect_filled(
                    rect,
                    3,
                    if selected == *i { p.selected } else { p.hover },
                );
            }
            let method = if requests[*i].method == "DELETE" {
                "DEL"
            } else {
                &requests[*i].method
            };
            text(
                ui,
                pos2(rect.left() + 7., rect.center().y),
                method,
                10.,
                style::method_color(&requests[*i].method, p),
            );
            // Names line up at 43px unless a long method (PATCH, OPTIONS…) needs more room.
            let method_width = ui
                .painter()
                .layout_no_wrap(method.into(), FontId::proportional(10.), p.text)
                .size()
                .x;
            let label = Rect::from_min_max(
                pos2(rect.left() + (7. + method_width + 8.).max(43.), rect.top()),
                pos2(
                    if hot {
                        menu_rect.left() - 2.
                    } else {
                        rect.right()
                    },
                    rect.bottom(),
                ),
            );
            let galley =
                ui.painter()
                    .layout_no_wrap(path[depth].into(), FontId::proportional(13.), p.text);
            ui.painter().with_clip_rect(label).galley(
                pos2(label.left(), rect.center().y - 8.),
                galley,
                p.text,
            );
            if hot {
                if menu_btn.hovered() || menu_open {
                    ui.painter().rect_filled(menu_rect, 4, p.border);
                }
                style::icon(
                    ui,
                    menu_rect,
                    Icon::Dots,
                    if menu_btn.hovered() { p.text } else { p.muted },
                );
            } else if is_pinned {
                style::icon_sized(ui, menu_rect, Icon::Pin, p.gold, 13.);
            }
            if res.double_clicked() {
                picked = Some(RowAction::OpenKept(*i));
            } else if res.clicked() {
                picked = Some(RowAction::Open(*i));
            }
            if res.drag_started() {
                res.dnd_set_drag_payload(Dragged::Request(*i));
            }
            let above = pointer_above(ui, rect);
            if let Some(payload) = res.dnd_hover_payload::<Dragged>()
                && *payload != Dragged::Request(*i)
            {
                // A line where the dragged request would land.
                let y = if above { rect.top() } else { rect.bottom() };
                ui.painter().line_segment(
                    [pos2(rect.left(), y), pos2(rect.right(), y)],
                    Stroke::new(2., p.gold),
                );
            }
            if let Some(payload) = res.dnd_release_payload::<Dragged>() {
                let target = if above {
                    DropTarget::Before(*i)
                } else {
                    DropTarget::After(*i)
                };
                picked = Some(RowAction::Move((*payload).clone(), target));
            }
            if let Some(kind) = egui::Popup::menu(&menu_btn)
                .show(|ui| row_menu_items(ui, is_pinned, p))
                .and_then(|r| r.inner)
            {
                picked = Some(RowAction::Menu(*i, kind));
            }
            res.context_menu(|ui| {
                if let Some(kind) = row_menu_items(ui, is_pinned, p) {
                    picked = Some(RowAction::Menu(*i, kind));
                }
            });
        }
    }
    picked
}
#[derive(Clone, Copy)]
enum TabMenu {
    New,
    Clone,
    Revert,
    Close,
    CloseOthers,
    CloseLeft,
    CloseRight,
    CloseSaved,
    CloseAll,
}
/// Right-click menu of a request tab; `enabled` follows the order of the entries below.
fn tab_menu_items(ui: &mut Ui, enabled: [bool; 9], p: Palette) -> Option<TabMenu> {
    ui.set_width(190.);
    ui.spacing_mut().item_spacing.y = 1.;
    let entries = [
        (TabMenu::New, "New Request"),
        (TabMenu::Clone, "Clone Request"),
        (TabMenu::Revert, "Revert Changes"),
        (TabMenu::Close, "Close"),
        (TabMenu::CloseOthers, "Close Others"),
        (TabMenu::CloseLeft, "Close to the Left"),
        (TabMenu::CloseRight, "Close to the Right"),
        (TabMenu::CloseSaved, "Close Saved"),
        (TabMenu::CloseAll, "Close All"),
    ];
    let mut chosen = None;
    for ((kind, label), enabled) in entries.into_iter().zip(enabled) {
        let (rect, res) = ui.allocate_exact_size(
            vec2(ui.available_width(), 28.),
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        if enabled && res.hovered() {
            ui.painter().rect_filled(rect, 4, p.hover);
        }
        text(
            ui,
            pos2(rect.left() + 12., rect.center().y),
            label,
            13.,
            if enabled {
                p.text
            } else {
                p.muted.gamma_multiply(0.55)
            },
        );
        if enabled
            && res
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
        {
            chosen = Some(kind);
        }
    }
    if chosen.is_some() {
        ui.close();
    }
    chosen
}
#[derive(Clone, Copy)]
enum RowMenu {
    Pin,
    Clone,
    Copy,
    Rename,
    Code,
    Reveal,
    Info,
    Delete,
}
#[derive(Clone, Copy, PartialEq)]
enum FolderMenu {
    Pin,
    NewRequest,
    NewFolder,
    Clone,
    Rename,
    Collapse,
    Expand,
    Export,
    Reveal,
    Settings,
    Delete,
}
enum RowAction {
    Open(usize),
    /// Double click: open and keep.
    OpenKept(usize),
    Menu(usize, RowMenu),
    /// Folder path (empty for the collection itself) and the chosen entry.
    Folder(String, FolderMenu),
    /// Something was dropped in the tree.
    Move(Dragged, DropTarget),
}
/// Index at the top or bottom edge of a row, depending on where the pointer is.
fn pointer_above(ui: &Ui, rect: Rect) -> bool {
    ui.input(|i| i.pointer.interact_pos())
        .is_some_and(|pos| pos.y < rect.center().y)
}
fn menu_item(ui: &mut Ui, icon: Icon, label: &str, danger: bool, p: Palette) -> bool {
    let (rect, res) = ui.allocate_exact_size(vec2(ui.available_width(), 28.), Sense::click());
    let res = res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
    if res.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    let color = if danger { p.red } else { p.text };
    style::icon(
        ui,
        Rect::from_center_size(pos2(rect.left() + 16., rect.center().y), vec2(16., 16.)),
        icon,
        if danger { p.red } else { p.muted },
    );
    text(
        ui,
        pos2(rect.left() + 34., rect.center().y),
        label,
        13.,
        color,
    );
    res.clicked()
}
/// A row of a small dropdown where one entry can be the current choice (shown with a check).
fn menu_choice(
    ui: &mut Ui,
    label: &str,
    checked: bool,
    color: Option<Color32>,
    p: Palette,
) -> bool {
    let (rect, res) = ui.allocate_exact_size(vec2(ui.available_width(), 28.), Sense::click());
    if res.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    text(
        ui,
        pos2(rect.left() + 12., rect.center().y),
        label,
        13.,
        color.unwrap_or(p.text),
    );
    if checked {
        text(
            ui,
            pos2(rect.right() - 22., rect.center().y),
            "✓",
            13.,
            p.gold,
        );
    }
    res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
        .clicked()
}
fn menu_gap(ui: &mut Ui) {
    ui.add_space(2.);
    ui.separator();
    ui.add_space(2.);
}
fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else if cfg!(target_os = "windows") {
        "Show in Explorer"
    } else {
        "Show in File Manager"
    }
}
/// Items of the sidebar row menu (hover "⋯" button and right click).
fn row_menu_items(ui: &mut Ui, pinned: bool, p: Palette) -> Option<RowMenu> {
    ui.set_width(176.);
    ui.spacing_mut().item_spacing.y = 1.;
    let mut chosen = None;
    for (kind, icon, label) in [
        (
            RowMenu::Pin,
            Icon::Pin,
            if pinned { "Unpin" } else { "Pin to top" },
        ),
        (RowMenu::Clone, Icon::Copy, "Clone"),
        (RowMenu::Copy, Icon::Copy, "Copy as cURL"),
        (RowMenu::Rename, Icon::Pencil, "Rename"),
        (RowMenu::Code, Icon::Code, "Generate Code"),
        (RowMenu::Reveal, Icon::Folder, reveal_label()),
    ] {
        if menu_item(ui, icon, label, false, p) {
            chosen = Some(kind);
        }
    }
    ui.add_space(2.);
    ui.separator();
    ui.add_space(2.);
    if menu_item(ui, Icon::Info, "Info", false, p) {
        chosen = Some(RowMenu::Info);
    }
    if menu_item(ui, Icon::Trash, "Delete", true, p) {
        chosen = Some(RowMenu::Delete);
    }
    if chosen.is_some() {
        ui.close();
    }
    chosen
}
/// Items of the folder menu; the collection root gets Settings instead of Clone and Delete.
fn folder_menu_items(ui: &mut Ui, root: bool, pinned: bool, p: Palette) -> Option<FolderMenu> {
    ui.set_width(176.);
    ui.spacing_mut().item_spacing.y = 1.;
    let mut chosen = None;
    let mut items = vec![];
    if !root {
        items.push((
            FolderMenu::Pin,
            Icon::Pin,
            if pinned { "Unpin" } else { "Pin to top" },
        ));
    }
    items.extend([
        (FolderMenu::NewRequest, Icon::Plus, "New Request"),
        (FolderMenu::NewFolder, Icon::Folder, "New Folder"),
    ]);
    if !root {
        items.push((FolderMenu::Clone, Icon::Copy, "Clone"));
    }
    items.extend([
        (FolderMenu::Rename, Icon::Pencil, "Rename"),
        (FolderMenu::Collapse, Icon::Chevron, "Collapse all"),
        (FolderMenu::Expand, Icon::Chevron, "Expand all"),
    ]);
    if !root {
        items.push((FolderMenu::Export, Icon::Push, "Export as Postman…"));
    }
    items.push((FolderMenu::Reveal, Icon::Folder, reveal_label()));
    for (kind, icon, label) in items {
        if menu_item(ui, icon, label, false, p) {
            chosen = Some(kind);
        }
    }
    ui.add_space(2.);
    ui.separator();
    ui.add_space(2.);
    if root {
        if menu_item(ui, Icon::Settings, "Settings", false, p) {
            chosen = Some(FolderMenu::Settings);
        }
    } else if menu_item(ui, Icon::Trash, "Delete", true, p) {
        chosen = Some(FolderMenu::Delete);
    }
    if chosen.is_some() {
        ui.close();
    }
    chosen
}
/// Hover "⋯" button plus right-click menu on a folder (or collection) header row.
fn header_menu(
    ui: &mut Ui,
    header: &egui::Response,
    id: &str,
    root: bool,
    pinned: bool,
    p: Palette,
) -> Option<FolderMenu> {
    let row = Rect::from_min_max(
        pos2(ui.max_rect().left(), header.rect.top()),
        pos2(ui.max_rect().right(), header.rect.bottom()),
    );
    let menu_rect = Rect::from_center_size(pos2(row.right() - 16., row.center().y), vec2(24., 22.));
    let button = ui
        .interact(menu_rect, ui.id().with(("header-menu", id)), Sense::click())
        .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&button));
    if ui.rect_contains_pointer(row) || open {
        if button.hovered() || open {
            ui.painter().rect_filled(menu_rect, 4, p.border);
        }
        style::icon(
            ui,
            menu_rect,
            Icon::Dots,
            if button.hovered() { p.text } else { p.muted },
        );
    } else if pinned {
        style::icon_sized(ui, menu_rect, Icon::Pin, p.gold, 13.);
    }
    let mut chosen = egui::Popup::menu(&button)
        .show(|ui| folder_menu_items(ui, root, pinned, p))
        .and_then(|r| r.inner);
    header.context_menu(|ui| {
        if let Some(kind) = folder_menu_items(ui, root, pinned, p) {
            chosen = Some(kind);
        }
    });
    chosen
}
/// Name / Value / Description rows (headers, params, form bodies).
fn header_table(
    ui: &mut Ui,
    rows: &mut Vec<Header>,
    draft: &mut Header,
    p: Palette,
    checkboxes: bool,
    bulk: Option<&mut bool>,
    kinds: bool,
) -> bool {
    kv_table(ui, rows, draft, p, checkboxes, bulk, kinds, true)
}
/// Name / Value rows for collection and environment variables, same behaviour as the others:
/// the empty last row turns into a real row as soon as anything is typed.
fn variable_table(ui: &mut Ui, rows: &mut Vec<Variable>, draft: &mut Header, p: Palette) -> bool {
    let mut as_rows: Vec<Header> = rows
        .iter()
        .map(|v| {
            let mut h = Header::new(&v.name, &v.value);
            h.enabled = v.enabled;
            h
        })
        .collect();
    let changed = kv_table(ui, &mut as_rows, draft, p, true, None, false, false);
    if changed {
        *rows = as_rows
            .into_iter()
            .map(|h| Variable {
                enabled: h.enabled,
                name: h.name,
                value: h.value,
            })
            .collect();
    }
    changed
}
#[allow(clippy::too_many_arguments)]
fn kv_table(
    ui: &mut Ui,
    rows: &mut Vec<Header>,
    draft: &mut Header,
    p: Palette,
    checkboxes: bool,
    bulk: Option<&mut bool>,
    kinds: bool,
    describe: bool,
) -> bool {
    let cols = if describe { 3 } else { 2 };
    // Values of variables that look like secrets stay hidden unless asked for.
    let mask_secrets = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(egui::Id::new("mask-secrets")))
        .unwrap_or(false);
    let spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.;
    let width = ui.available_width().max(260.);
    let height = 35.;
    let mut changed = false;
    let mut remove = None;
    // Cells get explicit ids so focus can follow typing from the empty last row into the row
    // it turns into.
    let table_id = ui.id().with("kv-table");
    // Form-data rows reserve room in the name cell for the Text/File selector, so that
    // column starts wider.
    let columns: Vec<f32> = match (describe, kinds) {
        (true, true) => vec![0., width * 0.40, width * 0.72, width - 40., width],
        (true, false) => vec![0., width * 0.26, width * 0.62, width - 40., width],
        (false, _) => vec![0., width * 0.40, width - 40., width],
    };
    let kind_width = if kinds { 66. } else { 0. };
    if let Some(bulk) = bulk {
        // Same spot as "Key-Value Edit" in the bulk view, so the toggle does not move.
        let (row, res) = ui.allocate_exact_size(vec2(width, 22.), Sense::click());
        let res = res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
        ui.painter().text(
            row.right_center(),
            Align2::RIGHT_CENTER,
            "Bulk Edit",
            FontId::proportional(12.5),
            if res.hovered() { p.text } else { p.muted },
        );
        if res.clicked() {
            *bulk = true;
        }
        ui.add_space(6.);
    }
    let top = ui.cursor().min;
    let head = Rect::from_min_size(top, vec2(width, 31.));
    ui.allocate_rect(head, Sense::hover());
    for (i, label) in ["Name", "Value", "Description"]
        .iter()
        .take(cols)
        .enumerate()
    {
        text(
            ui,
            pos2(
                head.left() + columns[i] + if i == 0 && checkboxes { 35. } else { 10. },
                head.center().y,
            ),
            label,
            13.,
            p.text,
        );
    }
    line(ui, head.left_bottom(), head.right_bottom(), p);
    for (i, h) in rows.iter_mut().enumerate() {
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let offset = if checkboxes { 33. } else { 10. };
        ui.push_id(i, |ui| {
            if checkboxes {
                let check = Rect::from_center_size(
                    pos2(rect.left() + 17., rect.center().y),
                    vec2(14., 14.),
                );
                let res = ui
                    .interact(check.expand(3.), ui.id().with("enabled"), Sense::click())
                    .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
                if res.clicked() {
                    h.enabled = !h.enabled;
                    changed = true;
                }
                ui.painter().rect(
                    check,
                    2,
                    if h.enabled { p.gold } else { p.bg },
                    Stroke::new(1., if h.enabled { p.gold } else { p.muted }),
                    StrokeKind::Inside,
                );
                if h.enabled {
                    ui.painter().add(egui::Shape::line(
                        vec![
                            check.min + vec2(3., 7.),
                            check.min + vec2(6., 10.),
                            check.min + vec2(11., 4.),
                        ],
                        Stroke::new(
                            1.6,
                            if p.dark {
                                Color32::from_gray(43)
                            } else {
                                Color32::WHITE
                            },
                        ),
                    ));
                }
            }
            for j in 0..cols {
                let cell = Rect::from_min_max(
                    pos2(
                        rect.left() + columns[j] + if j == 0 { offset } else { 10. },
                        rect.top() + 9.,
                    ),
                    pos2(
                        rect.left() + columns[j + 1] - 7. - if j == 0 { kind_width } else { 0. },
                        rect.bottom() - 5.,
                    ),
                );
                if j == 1 && kinds && h.file {
                    changed |= file_cell(ui, cell, rect.center().y, &mut h.value, p);
                    continue;
                }
                let secret = !describe && j == 1 && mask_secrets && looks_secret(&h.name);
                let (value, hint) = match j {
                    0 => (&mut h.name, if describe { "Name" } else { "Variable name" }),
                    1 => (&mut h.value, "Value"),
                    _ => (&mut h.description, "Description"),
                };
                let mut layouter = variable_layouter(p, FontId::proportional(13.));
                let edit = TextEdit::singleline(value)
                    .id(table_id.with(("cell", i, j)))
                    .frame(egui::Frame::NONE)
                    .font(FontId::proportional(13.))
                    .hint_text(hint);
                let edit = if secret {
                    edit.password(true)
                } else {
                    edit.layouter(&mut layouter)
                };
                changed |= ui.put(cell, edit).changed();
            }
            if kinds {
                changed |= kind_chip(ui, rect, columns[1], kind_width, h, p);
            }
            if icon_button(
                ui,
                Rect::from_center_size(pos2(rect.right() - 17., rect.center().y), vec2(26., 28.)),
                Icon::Trash,
                "Remove row",
                p,
            )
            .clicked()
            {
                remove = Some(i);
            }
        });
        line(ui, rect.left_bottom(), rect.right_bottom(), p);
    }
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    // Like Postman: the empty last row becomes a real row as soon as anything is entered.
    let mut created = None;
    ui.push_id("draft", |ui| {
        for j in 0..cols {
            let cell = Rect::from_min_max(
                pos2(
                    rect.left() + columns[j] + if j == 0 && checkboxes { 35. } else { 10. },
                    rect.top() + 9.,
                ),
                pos2(
                    rect.left() + columns[j + 1] - 7. - if j == 0 { kind_width } else { 0. },
                    rect.bottom() - 5.,
                ),
            );
            if j == 1 && kinds && draft.file {
                if file_cell(ui, cell, rect.center().y, &mut draft.value, p) {
                    created = Some(j);
                }
                continue;
            }
            let secret = !describe && j == 1 && mask_secrets && looks_secret(&draft.name);
            let (value, hint) = match j {
                0 => (
                    &mut draft.name,
                    if describe { "Name" } else { "Variable name" },
                ),
                1 => (&mut draft.value, "Value"),
                _ => (&mut draft.description, "Description"),
            };
            let mut layouter = variable_layouter(p, FontId::proportional(13.));
            let field = TextEdit::singleline(value)
                .id(table_id.with(("draft", j)))
                .frame(egui::Frame::NONE)
                .font(FontId::proportional(13.))
                .hint_text(hint);
            let field = if secret {
                field.password(true)
            } else {
                field.layouter(&mut layouter)
            };
            let edit = ui.put(cell, field);
            if edit.changed() {
                created = Some(j);
            }
        }
        if kinds {
            kind_chip(ui, rect, columns[1], kind_width, draft, p);
        }
    });
    if let Some(j) = created {
        rows.push(draft.clone());
        *draft = Header::new("", "");
        changed = true;
        // Keep typing where it was: focus the same column of the new row, caret at the end.
        let index = rows.len() - 1;
        let id = table_id.with(("cell", index, j));
        let length = match j {
            0 => rows[index].name.chars().count(),
            1 => rows[index].value.chars().count(),
            _ => rows[index].description.chars().count(),
        };
        let ctx = ui.ctx().clone();
        ctx.memory_mut(|m| m.request_focus(id));
        let mut state = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(length),
            )));
        state.store(&ctx, id);
        ctx.request_repaint();
    }
    let full = Rect::from_min_max(top, pos2(top.x + width, rect.bottom()));
    ui.painter()
        .rect_stroke(full, 5, Stroke::new(1., p.border), StrokeKind::Inside);
    for x in &columns[1..=cols] {
        line(
            ui,
            pos2(top.x + x, top.y),
            pos2(top.x + x, full.bottom()),
            p,
        );
    }
    if let Some(i) = remove {
        rows.remove(i);
        changed = true;
    }
    ui.spacing_mut().item_spacing.y = spacing;
    changed
}
/// Value cell of a form-data file row: shows the chosen file name and opens a file dialog.
fn file_cell(ui: &mut Ui, cell: Rect, centre_y: f32, value: &mut String, p: Palette) -> bool {
    let mut res = ui
        .interact(cell, ui.id().with("file-cell"), Sense::click())
        .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
    if !value.is_empty() {
        res = res.on_hover_text(value.as_str());
    }
    let label = if value.is_empty() {
        "Select file".to_owned()
    } else {
        value.rsplit(['/', '\\']).next().unwrap_or(value).to_owned()
    };
    ui.painter().with_clip_rect(cell).text(
        pos2(cell.left(), centre_y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.),
        if res.hovered() { p.text } else { p.blue },
    );
    if res.clicked()
        && let Some(path) = rfd::FileDialog::new().pick_file()
    {
        *value = path.to_string_lossy().into_owned();
        return true;
    }
    false
}
/// The "Text ⌄ / File ⌄" selector at the right edge of a form-data name cell.
fn kind_chip(
    ui: &mut Ui,
    row: Rect,
    name_right: f32,
    width: f32,
    field: &mut Header,
    p: Palette,
) -> bool {
    let chip = Rect::from_min_max(
        pos2(row.left() + name_right - 7. - width + 4., row.top() + 7.),
        pos2(row.left() + name_right - 6., row.bottom() - 7.),
    );
    let res = ui
        .interact(chip, ui.id().with("kind"), Sense::click())
        .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
    ui.painter().text(
        pos2(chip.left() + 4., chip.center().y),
        Align2::LEFT_CENTER,
        if field.file { "File" } else { "Text" },
        FontId::proportional(12.),
        if res.hovered() { p.text } else { p.muted },
    );
    style::chevron(
        ui,
        Rect::from_center_size(pos2(chip.right() - 9., chip.center().y), vec2(12., 12.)),
        1.,
        p.muted,
    );
    let chosen = egui::Popup::menu(&res)
        .show(|ui| {
            ui.set_width(90.);
            let mut chosen = None;
            if menu_item_plain(ui, "Text", p) {
                chosen = Some(false);
            }
            if menu_item_plain(ui, "File", p) {
                chosen = Some(true);
            }
            if chosen.is_some() {
                ui.close();
            }
            chosen
        })
        .and_then(|r| r.inner);
    match chosen {
        Some(file) if file != field.file => {
            field.file = file;
            field.value.clear();
            true
        }
        _ => false,
    }
}
/// Text-only entry of a small dropdown menu.
fn menu_item_plain(ui: &mut Ui, label: &str, p: Palette) -> bool {
    let (rect, res) = ui.allocate_exact_size(vec2(ui.available_width(), 28.), Sense::click());
    if res.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    text(
        ui,
        pos2(rect.left() + 12., rect.center().y),
        label,
        13.,
        p.text,
    );
    res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
        .clicked()
}
fn format_size(bytes: usize) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.),
    }
}
/// File name for "Save response": the server's own name if it sent one, else the last URL
/// segment with an extension taken from the content type.
fn suggested_file_name(response: &post_ronda::http::Response, url: &str) -> String {
    let clean = |name: &str| name.replace(['/', '\\', ':'], "_");
    if !response.file_name.is_empty() {
        return clean(&response.file_name);
    }
    let segment = url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.path_segments()
                .and_then(|s| s.rev().find(|p| !p.is_empty()).map(str::to_owned))
        })
        .unwrap_or_else(|| "response".into());
    let ct = response.content_type.to_ascii_lowercase();
    let extension = [
        ("spreadsheetml", "xlsx"),
        ("ms-excel", "xls"),
        ("wordprocessingml", "docx"),
        ("presentationml", "pptx"),
        ("pdf", "pdf"),
        ("png", "png"),
        ("jpeg", "jpg"),
        ("gif", "gif"),
        ("webp", "webp"),
        ("svg", "svg"),
        ("zip", "zip"),
        ("json", "json"),
        ("xml", "xml"),
        ("html", "html"),
        ("csv", "csv"),
        ("text/plain", "txt"),
    ]
    .iter()
    .find(|(needle, _)| ct.contains(needle))
    .map(|(_, ext)| *ext)
    .unwrap_or(if response.binary { "bin" } else { "txt" });
    if segment.contains('.') {
        clean(&segment)
    } else {
        format!("{}.{extension}", clean(&segment))
    }
}
/// A selectable preview card drawn with the colours of `preview`; returns true when clicked.
fn theme_card(ui: &mut Ui, preview: Palette, label: &str, selected: bool, p: Palette) -> bool {
    let (rect, res) = ui.allocate_exact_size(vec2(146., 112.), Sense::click());
    let res = res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
    let frame = Rect::from_min_size(rect.min, vec2(146., 83.));
    ui.painter().rect(
        frame,
        7,
        preview.bg,
        Stroke::new(
            if selected { 2. } else { 1. },
            if selected { p.gold } else { p.border },
        ),
        StrokeKind::Inside,
    );
    ui.painter().rect_filled(
        Rect::from_min_size(frame.min + vec2(6., 7.), vec2(134., 10.)),
        2,
        preview.field,
    );
    ui.painter().rect_filled(
        Rect::from_min_size(frame.min + vec2(6., 22.), vec2(31., 54.)),
        2,
        preview.sidebar,
    );
    for y in [27., 36., 45.] {
        ui.painter().line_segment(
            [frame.min + vec2(11., y), frame.min + vec2(30., y)],
            Stroke::new(2., preview.muted),
        );
    }
    for y in [28., 40., 52.] {
        ui.painter().line_segment(
            [frame.min + vec2(45., y), frame.min + vec2(125., y)],
            Stroke::new(2., preview.border),
        );
    }
    ui.painter().line_segment(
        [frame.min + vec2(46., 24.), frame.min + vec2(70., 24.)],
        Stroke::new(2., preview.gold),
    );
    text(
        ui,
        pos2(rect.left(), rect.bottom() - 12.),
        label,
        13.,
        p.text,
    );
    res.clicked()
}
/// A choice shown as a bordered card with a radio mark; returns true when clicked.
/// A labelled on/off switch; returns true when it was flipped.
/// A field-like button showing `current` that opens a list of `options`; returns the chosen index.
/// A path field with a Browse button, for the certificate settings; true when it changed.
fn file_field(ui: &mut Ui, label: &str, value: &mut String, p: Palette) -> bool {
    let mut changed = false;
    ui.label(RichText::new(label).size(12.5));
    ui.add_space(3.);
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 150.).max(120.);
        changed |= ui
            .add(
                TextEdit::singleline(value)
                    .hint_text("No file chosen")
                    .desired_width(width)
                    .margin(egui::Margin::symmetric(8, 7)),
            )
            .changed();
        if ui
            .add(egui::Button::new("Browse…").min_size(vec2(76., 30.)))
            .clicked()
            && let Some(path) = rfd::FileDialog::new().pick_file()
        {
            *value = path.display().to_string();
            changed = true;
        }
        if !value.is_empty()
            && ui
                .add(
                    egui::Button::new("Clear")
                        .frame(false)
                        .min_size(vec2(48., 30.)),
                )
                .clicked()
        {
            value.clear();
            changed = true;
        }
    });
    ui.add_space(8.);
    let _ = p;
    changed
}
fn dropdown_field(
    ui: &mut Ui,
    id: &str,
    current: &str,
    width: f32,
    options: &[&str],
    p: Palette,
) -> Option<usize> {
    let (rect, res) = ui.allocate_exact_size(vec2(width, 32.), Sense::click());
    let res = res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
    ui.painter().rect(
        rect,
        6,
        p.field,
        Stroke::new(1., if res.hovered() { p.muted } else { p.border }),
        StrokeKind::Inside,
    );
    text(
        ui,
        pos2(rect.left() + 12., rect.center().y),
        current,
        13.,
        p.text,
    );
    style::chevron(
        ui,
        Rect::from_center_size(pos2(rect.right() - 16., rect.center().y), vec2(14., 14.)),
        1.,
        p.muted,
    );
    let mut chosen = None;
    let _ = id;
    egui::Popup::menu(&res).show(|ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 1.;
        egui::ScrollArea::vertical()
            .max_height(360.)
            .show(ui, |ui| {
                for (i, option) in options.iter().enumerate() {
                    if menu_choice(ui, option, *option == current, None, p) {
                        chosen = Some(i);
                        ui.close();
                    }
                }
            });
    });
    chosen
}
fn switch(ui: &mut Ui, on: &mut bool, label: &str, p: Palette) -> bool {
    let (rect, res) = ui.allocate_exact_size(vec2(ui.available_width(), 28.), Sense::click());
    let res = res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
    let track = Rect::from_min_size(rect.min + vec2(0., 4.), vec2(38., 20.));
    let t = ui.ctx().animate_bool_with_time(res.id, *on, 0.12);
    ui.painter().rect(
        track,
        10,
        if *on { p.gold } else { p.border },
        Stroke::NONE,
        StrokeKind::Inside,
    );
    let knob = pos2(track.left() + 10. + 18. * t, track.center().y);
    ui.painter().circle_filled(
        knob,
        7.5,
        if p.dark {
            Color32::WHITE
        } else {
            Color32::from_gray(250)
        },
    );
    text(
        ui,
        pos2(track.right() + 12., rect.center().y),
        label,
        13.,
        p.text,
    );
    if res.clicked() {
        *on = !*on;
        return true;
    }
    false
}
fn option_card(ui: &mut Ui, selected: bool, title: &str, detail: &str, p: Palette) -> bool {
    let (rect, res) = ui.allocate_exact_size(vec2(ui.available_width(), 56.), Sense::click());
    let res = res.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
    ui.painter().rect(
        rect,
        6,
        if selected {
            p.selected
        } else if res.hovered() {
            p.hover
        } else {
            p.bg
        },
        Stroke::new(
            if selected { 1.5 } else { 1. },
            if selected { p.gold } else { p.border },
        ),
        StrokeKind::Inside,
    );
    let centre = pos2(rect.left() + 20., rect.center().y);
    ui.painter().circle_stroke(
        centre,
        7.,
        Stroke::new(1.5, if selected { p.gold } else { p.muted }),
    );
    if selected {
        ui.painter().circle_filled(centre, 3.5, p.gold);
    }
    let clip = ui.painter().with_clip_rect(rect.shrink2(vec2(8., 0.)));
    clip.text(
        pos2(rect.left() + 38., rect.center().y - 9.),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.5),
        p.text,
    );
    clip.text(
        pos2(rect.left() + 38., rect.center().y + 10.),
        Align2::LEFT_CENTER,
        detail,
        FontId::proportional(12.),
        p.muted,
    );
    res.clicked()
}
fn rows_to_bulk(rows: &[Header]) -> String {
    rows.iter()
        .map(|h| {
            format!(
                "{}{}:{}",
                if h.enabled { "" } else { "//" },
                h.name,
                h.value
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
/// Parses bulk text back into rows, keeping descriptions and file flags of rows whose name
/// is unchanged. `//` at the start of a line switches that row off.
fn bulk_to_rows(text: &str, previous: &[Header]) -> Vec<Header> {
    let mut used = vec![false; previous.len()];
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (enabled, line) = match line.strip_prefix("//") {
                Some(rest) => (false, rest.trim_start()),
                None => (true, line),
            };
            let (name, value) = line.split_once(':').unwrap_or((line, ""));
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            let mut row = Header::new(name, value.trim());
            row.enabled = enabled;
            if let Some(i) = previous
                .iter()
                .enumerate()
                .position(|(i, h)| !used[i] && h.name == name)
            {
                used[i] = true;
                row.description = previous[i].description.clone();
                row.file = previous[i].file;
            }
            Some(row)
        })
        .collect()
}
#[cfg(test)]
mod tests {
    #![allow(clippy::field_reassign_with_default)]

    #[test]
    fn typing_two_braces_in_the_url_offers_variables_and_enter_picks_one() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.environments.items = vec![Environment {
            name: "Local".into(),
            variables: vec![
                Variable {
                    enabled: true,
                    name: "token".into(),
                    value: "t".into(),
                },
                Variable {
                    enabled: true,
                    name: "host".into(),
                    value: "h".into(),
                },
            ],
        }];
        app.environments.active = Some(0);
        app.focus_url = true;
        draw(&ctx, &mut app, vec![]);
        draw(&ctx, &mut app, vec![]);
        draw(
            &ctx,
            &mut app,
            vec![egui::Event::Text("http://x/{{to".into())],
        );
        draw(&ctx, &mut app, vec![]);
        assert_eq!(app.collection.requests[0].url, "http://x/{{to");
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: Some(egui::Key::Enter),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        draw(&ctx, &mut app, vec![enter]);
        assert_eq!(app.collection.requests[0].url, "http://x/{{token}}");
    }

    #[test]
    fn find_in_response_lists_matches_case_insensitively() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.response = Some(post_ronda::http::Response {
            status: "200 OK".into(),
            headers: String::new(),
            body: "alpha Beta\nBETA beta\ngamma".into(),
            raw: vec![],
            elapsed: Duration::ZERO,
            bytes: 0,
            truncated: false,
            binary: false,
            content_type: "text/plain".into(),
            file_name: String::new(),
        });
        app.pretty = false;
        app.response_body();
        app.find_query = "beta".into();
        app.find_in_response();
        assert_eq!(app.find_matches, [(0, 6, 10), (1, 0, 4), (1, 5, 9)]);
        assert_eq!(finds_on(&app.find_matches, 1).len(), 2);
        assert!(finds_on(&app.find_matches, 2).is_empty());
        app.find_query = "zzz".into();
        app.find_in_response();
        assert!(app.find_matches.is_empty());
        // Opening a different response invalidates the old matches.
        app.find_query = "beta".into();
        app.find_in_response();
        app.response_body();
        assert_ne!(app.find_for, app.find_query);
    }

    #[test]
    fn sent_request_history_survives_a_restart_and_stays_per_project() {
        let ctx = egui::Context::default();
        let app = app(&ctx);
        let other = app.project.with_file_name("another-project");
        let sent = Request {
            name: "Ping".into(),
            url: "http://a.test".into(),
            ..Default::default()
        };
        crate::save_history(&app.project, std::slice::from_ref(&sent));
        assert_eq!(crate::load_history(&app.project), vec![sent]);
        assert!(crate::load_history(&other).is_empty());
        // The history is kept outside the project folder.
        assert!(!crate::history_file(&app.project).starts_with(&app.project));
    }

    #[test]
    fn a_folder_exports_as_its_own_collection() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let req = |name: &str| Request {
            name: name.into(),
            url: "http://a.test".into(),
            ..Default::default()
        };
        app.collection.requests = vec![
            req("Top"),
            req("Pay / List"),
            req("Pay / Cards / Add"),
            req("Other / X"),
        ];
        app.collection.folders = vec![post_ronda::collection::FolderMark::new("Pay / Empty", 3)];
        app.collection.variables = vec![Variable {
            enabled: true,
            name: "host".into(),
            value: "h".into(),
        }];
        let exported = app.folder_as_collection("Pay");
        assert_eq!(exported.name, "Pay");
        let names: Vec<_> = exported.requests.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["List", "Cards / Add"]);
        assert_eq!(exported.folders.len(), 1);
        assert_eq!(exported.folders[0].path, "Empty");
        assert_eq!(exported.variables.len(), 1);
        // And it reads back as a normal Postman collection.
        let again = post_ronda::collection::import(
            &post_ronda::collection::export_postman(&exported).to_string(),
        )
        .unwrap();
        assert_eq!(again.requests.len(), 2);
        // Expand all / collapse all reach folders that are nested or empty.
        let state = |path: &str| -> Option<bool> {
            ctx.data_mut(|d| d.get_persisted(egui::Id::new(("folder-open", path))))
        };
        app.set_folders_open(&ctx, "Pay", false);
        assert_eq!(state("Pay"), Some(false));
        assert_eq!(state("Pay / Cards"), Some(false));
        assert_eq!(state("Pay / Empty"), Some(false));
        assert_eq!(state("Other"), None);
        app.set_folders_open(&ctx, "", true);
        assert_eq!(state("Other"), Some(true));
        assert_eq!(state("Pay / Cards"), Some(true));
    }

    #[test]
    fn looking_at_requests_reuses_one_preview_tab_until_you_interact() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.collection.requests = ["A", "B", "C", "D", "E"]
            .iter()
            .map(|n| Request {
                name: (*n).into(),
                ..Default::default()
            })
            .collect();
        app.baseline = app
            .collection
            .requests
            .iter()
            .cloned()
            .enumerate()
            .collect();
        app.tabs = vec![0];
        app.selected = 0;
        // Each look replaces the previous one; the tab that was already there stays.
        app.open_preview(1);
        assert_eq!(app.tabs, [0, 1]);
        app.open_preview(2);
        assert_eq!(app.tabs, [0, 2]);
        assert_eq!((app.selected, app.preview), (2, Some(2)));
        // Opening a request that already has a tab just switches to it.
        app.open_preview(0);
        assert_eq!(app.tabs, [0, 2]);
        assert_eq!(app.preview, Some(2));
        // Editing keeps the tab: the next look gets a tab of its own.
        app.activate(2);
        app.collection.requests[2].url = "http://a.test".into();
        draw(&ctx, &mut app, vec![]);
        assert_eq!(app.preview, None);
        app.open_preview(3);
        assert_eq!(app.tabs, [0, 2, 3]);
        // A double click (or sending) keeps it too.
        app.keep_tab(3);
        app.open_preview(4);
        assert_eq!(app.tabs, [0, 2, 3, 4]);
        app.activate(4);
        app.preview = Some(4);
        app.send(&ctx);
        assert_eq!(app.preview, None);
    }

    #[derive(Debug)]
    struct Dropped(std::path::PathBuf);
    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|e| e.to_string())
        }
    }
    fn drop_files(ctx: &egui::Context, app: &mut App, files: &[&std::path::Path]) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0., 0.), vec2(1440., 900.))),
            dropped_files: files
                .iter()
                .map(|p| std::sync::Arc::new(Dropped(p.to_path_buf())) as egui::DroppedFileHandle)
                .collect(),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui, &mut eframe::Frame::_new_kittest()));
        output.textures_delta.clear();
    }
    #[test]
    fn dropping_files_opens_the_import_dialog_or_adds_environments() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, text: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, text).unwrap();
            path
        };
        let collection = |name: &str| {
            format!(
                r#"{{"info":{{"name":"{name}","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"}},"item":[{{"name":"Ping","request":{{"method":"GET","url":"http://localhost/ping"}}}}]}}"#
            )
        };
        let one = write("one.json", &collection("One"));
        let two = write("two.json", &collection("Two"));
        let env = write(
            "env.json",
            r#"{"name":"Stage","values":[{"key":"host","value":"h","enabled":true}]}"#,
        );
        let junk = write("junk.json", "{\"hello\":1}");
        // A collection opens the dialog, pre-filled with its name.
        drop_files(&ctx, &mut app, &[&one]);
        assert!(app.np_open);
        assert_eq!(app.np_name, "One");
        // Several at once: the rest wait until the dialog is free.
        app.np_open = false;
        drop_files(&ctx, &mut app, &[&one, &two]);
        assert_eq!(app.np_name, "One");
        assert_eq!(app.import_queue.len(), 1);
        app.np_open = false;
        draw(&ctx, &mut app, vec![]);
        assert!(app.np_open && app.np_import.as_ref().is_some_and(|c| c.name == "Two"));
        // An environment is added right away.
        app.np_open = false;
        app.import_queue.clear();
        drop_files(&ctx, &mut app, &[&env]);
        assert!(!app.np_open);
        assert_eq!(
            app.environments.items.last().map(|e| e.name.as_str()),
            Some("Stage")
        );
        // Something else changes nothing.
        let before = app.environments.items.len();
        drop_files(&ctx, &mut app, &[&junk]);
        assert!(!app.np_open);
        assert_eq!(app.environments.items.len(), before);
    }

    #[test]
    fn response_selection_spans_lines_and_words() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.response = Some(post_ronda::http::Response {
            status: "200 OK".into(),
            headers: String::new(),
            body: "{\"a\": 1,\n \"user_name\": \"x\"}".into(),
            raw: vec![],
            elapsed: Duration::ZERO,
            bytes: 0,
            truncated: false,
            binary: false,
            content_type: "text/plain".into(),
            file_name: String::new(),
        });
        app.pretty = false;
        app.response_body();
        assert_eq!(app.selected_code(), None);
        // From the middle of the first line to the middle of the second, in either direction.
        app.code_sel = Some(((0, 2), (1, 7)));
        let expected = app.selected_code().unwrap();
        assert!(expected.starts_with("a\": 1,\n"));
        app.code_sel = Some(((1, 7), (0, 2)));
        assert_eq!(app.selected_code().unwrap(), expected);
        // Reloading the response drops the selection.
        app.response_body();
        assert!(app.code_sel.is_none());
        assert_eq!(word_range("\"user_name\": 1", 3), (1, 10));
        assert_eq!(word_range("", 0), (0, 0));
    }

    #[test]
    fn variables_are_found_and_split_out_of_text() {
        let spans = variable_spans("{{ base_url }}/a/{{id}}?x={{open");
        let names: Vec<_> = spans.iter().map(|s| s.2).collect();
        assert_eq!(names, ["base_url", "id"]);
        assert_eq!(&"{{ base_url }}/a/{{id}}"[spans[1].0..spans[1].1], "{{id}}");
        assert!(variable_spans("no variables {}").is_empty());
    }
    use super::*;
    use eframe::App as _;
    /// A fresh App that lives entirely in a temporary folder: its own state directory and its
    /// own project, so no test can read or overwrite the developer's real data.
    fn app(ctx: &egui::Context) -> App {
        use std::sync::{
            Once, OnceLock,
            atomic::{AtomicUsize, Ordering},
        };
        static STATE: OnceLock<tempfile::TempDir> = OnceLock::new();
        static PROJECTS: AtomicUsize = AtomicUsize::new(0);
        static SET_ENV: Once = Once::new();
        let state = STATE.get_or_init(|| tempfile::tempdir().expect("temporary state directory"));
        SET_ENV.call_once(|| {
            // SAFETY: runs once, before any test builds an App and reads the variable.
            unsafe { std::env::set_var("POST_RONDA_STATE_DIR", state.path()) };
        });
        let mut app = App::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        let project = state.path().join(format!(
            "project-{}",
            PROJECTS.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&project).expect("temporary project");
        app.project = project;
        app.collection = Collection::default();
        app.environments = Default::default();
        app.tabs = vec![0];
        app.selected = 0;
        app.dirty = None;
        app.response = None;
        app
    }
    fn draw(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0., 0.), vec2(1440., 900.))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui, &mut eframe::Frame::_new_kittest()));
        output.textures_delta.clear();
    }
    #[test]
    fn paste_multiline_curl_into_url_maps_request() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        draw(&ctx, &mut app, vec![]);
        draw(&ctx, &mut app, vec![]);
        let command = "curl 'http://localhost:9417/echo?x=1' \\\n -X POST \\\n -H 'Content-Type: application/json' \\\n -H 'Cookie: session=test' \\\n --data-raw '{\"hello\":\"ronda\"}'";
        draw(&ctx, &mut app, vec![egui::Event::Paste(command.into())]);
        let r = &app.collection.requests[0];
        assert_eq!(r.method, "POST");
        assert_eq!(r.url, "http://localhost:9417/echo?x=1");
        assert_eq!(r.headers.len(), 2);
        assert_eq!(r.body, "{\n  \"hello\": \"ronda\"\n}");
        assert_eq!(
            app.tab, 1,
            "a pasted request with a body opens on the Body tab"
        );
    }
    #[test]
    fn shortcut_creates_new_tab_and_switches_back() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        draw(&ctx, &mut app, vec![]);
        draw(
            &ctx,
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::N,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
        assert_eq!(app.tabs, vec![0, 1]);
        assert_eq!(app.selected, 1);
        app.activate(0);
        assert_eq!(app.selected, 0);
        assert_eq!(app.tabs.len(), 2);
    }
    #[test]
    fn all_views_render_in_both_themes() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.environments.items.push(Environment {
            name: "Local".into(),
            variables: vec![Variable {
                enabled: true,
                name: "base_url".into(),
                value: "http://localhost:9417".into(),
            }],
        });
        app.editing_environment = Some(0);
        for theme in [Appearance::Light, Appearance::Dark] {
            app.prefs.appearance = theme;
            style::apply(&ctx, &app.prefs);
            for tab in 0..6 {
                app.tab = tab;
                app.page = 0;
                app.settings = false;
                draw(&ctx, &mut app, vec![]);
            }
            for page in 1..=3 {
                app.page = page;
                draw(&ctx, &mut app, vec![]);
            }
            app.settings = true;
            for tab in 0..6 {
                app.prefs_tab = tab;
                draw(&ctx, &mut app, vec![]);
            }
            app.settings = false;
            app.project_settings = true;
            for tab in 0..6 {
                app.collection_tab = tab;
                draw(&ctx, &mut app, vec![]);
            }
            app.project_settings = false;
            app.search_open = true;
            draw(&ctx, &mut app, vec![]);
            app.search_open = false;
            app.codegen_open = true;
            for lang in 0..post_ronda::codegen::LANGUAGES.len() {
                app.codegen_lang = lang;
                draw(&ctx, &mut app, vec![]);
            }
            app.codegen_open = false;
            app.save_open = true;
            draw(&ctx, &mut app, vec![]);
            app.save_open = false;
        }
    }
    #[test]
    fn saving_a_new_request_asks_for_name_and_folder() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let before = app.collection.requests.len();
        app.new_request();
        app.save_request();
        assert!(app.save_open);
        app.save_name = "Create invoice".into();
        app.save_folder = "Billing / Invoices".into();
        app.confirm_save();
        assert!(!app.save_open);
        assert!(
            app.toast
                .as_ref()
                .is_some_and(|t| !t.error && t.text == "Saved locally")
        );
        draw(&ctx, &mut app, vec![]);
        assert!(app.drafts.is_empty());
        assert_eq!(
            app.collection.requests[before].name,
            "Billing / Invoices / Create invoice"
        );
        // An already saved request saves straight away without a dialog.
        app.save_request();
        assert!(!app.save_open);
    }
    #[test]
    fn unsaved_requests_stay_out_of_the_file_and_vanish_with_their_tab() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let saved = app.collection.requests.len();
        app.new_request();
        let draft = app.collection.requests.len() - 1;
        assert!(app.drafts.contains(&draft));
        app.save().unwrap();
        let text = std::fs::read_to_string(app.project.join("collection.json")).unwrap();
        let on_disk = post_ronda::collection::import(&text).unwrap();
        assert_eq!(on_disk.requests.len(), saved);
        app.close_tab(draft);
        assert_eq!(app.collection.requests.len(), saved);
        assert!(app.drafts.is_empty());
        assert!(app.selected < saved);
    }
    #[test]
    fn untitled_drafts_are_numbered_and_row_menu_actions_work() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.new_request();
        app.new_request();
        let names: Vec<_> = app
            .drafts
            .iter()
            .map(|&i| app.collection.requests[i].name.clone())
            .collect();
        assert_eq!(names, ["Untitled 1", "Untitled 2"]);
        // Closing "Untitled 1" frees its number again.
        let first = *app.drafts.iter().next().unwrap();
        app.close_tab(first);
        app.new_request();
        assert!(
            app.collection
                .requests
                .iter()
                .any(|r| r.name == "Untitled 1")
        );
        let before = app.collection.requests.len();
        app.row_menu_action(&ctx, 0, RowMenu::Clone);
        assert_eq!(app.collection.requests.len(), before + 1);
        assert!(
            app.collection
                .requests
                .last()
                .unwrap()
                .name
                .ends_with("(copy)")
        );
        app.row_menu_action(&ctx, 0, RowMenu::Rename);
        assert!(app.rename_open);
        app.rename_name = "Health".into();
        draw(&ctx, &mut app, vec![]);
        app.rename_open = true;
        app.info_open = true;
        app.info_index = 0;
        draw(&ctx, &mut app, vec![]);
        app.row_menu_action(&ctx, 0, RowMenu::Delete);
        assert!(app.confirm_delete);
        draw(&ctx, &mut app, vec![]);
        let count = app.collection.requests.len();
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: Some(egui::Key::Enter),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        draw(&ctx, &mut app, vec![enter]);
        assert!(!app.confirm_delete);
        assert_eq!(app.collection.requests.len(), count - 1);
    }
    #[test]
    fn folder_menu_manages_folders_and_their_requests() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let req = |name: &str| Request {
            name: name.into(),
            ..Default::default()
        };
        app.collection.requests = vec![
            req("Billing / List"),
            req("Billing / Invoices / Create"),
            req("Other"),
        ];
        app.selected = 0;
        app.tabs = vec![0];
        app.add_folder("Billing / Archive".into());
        app.rename_folder("Billing", "Payments");
        let names: Vec<_> = app
            .collection
            .requests
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["Payments / List", "Payments / Invoices / Create", "Other"]
        );
        assert_eq!(folder_paths(&app), ["Payments / Archive"]);
        app.folder_menu_action(&ctx, "Payments".into(), FolderMenu::Clone);
        assert!(
            app.collection
                .requests
                .iter()
                .any(|r| r.name == "Payments (copy) / Invoices / Create")
        );
        assert!(folder_paths(&app).contains(&"Payments (copy) / Archive".to_string()));
        app.folder_menu_action(&ctx, "Payments".into(), FolderMenu::NewRequest);
        let draft = *app.drafts.iter().next().unwrap();
        assert_eq!(app.collection.requests[draft].name, "Payments / Untitled 1");
        app.close_tab(draft);
        // Deleting asks first, then removes the folder with everything inside it.
        app.folder_menu_action(&ctx, "Payments".into(), FolderMenu::Delete);
        assert!(app.confirm_delete);
        assert_eq!(app.delete_count, 2);
        draw(&ctx, &mut app, vec![]);
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: Some(egui::Key::Enter),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        draw(&ctx, &mut app, vec![enter]);
        assert!(!app.confirm_delete);
        assert!(
            app.collection
                .requests
                .iter()
                .all(|r| !r.name.starts_with("Payments / "))
        );
        assert!(
            folder_paths(&app)
                .iter()
                .all(|f| f != "Payments" && !f.starts_with("Payments / "))
        );
        assert!(
            app.collection
                .requests
                .iter()
                .any(|r| r.name == "Payments (copy) / List")
        );
        // The collection itself can be collapsed and renamed from the same menu.
        let open_state = |path: &str| -> Option<bool> {
            ctx.data_mut(|d| d.get_persisted(egui::Id::new(("folder-open", path))))
        };
        app.folder_menu_action(&ctx, String::new(), FolderMenu::Collapse);
        assert_eq!(open_state("Payments (copy)"), Some(false));
        app.folder_menu_action(&ctx, String::new(), FolderMenu::Expand);
        assert_eq!(open_state("Payments (copy)"), Some(true));
        app.folder_menu_action(&ctx, String::new(), FolderMenu::Rename);
        assert_eq!(app.rename_target, NameTarget::Collection);
    }
    #[test]
    fn closing_tabs_in_bulk_keeps_focus_and_discards_drafts() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let req = |name: &str| Request {
            name: name.into(),
            ..Default::default()
        };
        app.collection.requests = vec![req("A"), req("B"), req("C"), req("D")];
        app.save().unwrap();
        app.tabs = vec![0, 1, 2, 3];
        app.selected = 3;
        app.close_tabs(&[2, 3], Some(1));
        assert_eq!(app.tabs, [0, 1]);
        assert_eq!(app.selected, 1);
        // Two drafts between saved tabs: closing everything on the right drops them for good.
        app.new_request();
        app.new_request();
        app.tabs = vec![0, 4, 1, 5];
        app.close_tabs(&[4, 1, 5], Some(0));
        assert_eq!(app.tabs, [0]);
        assert_eq!(app.collection.requests.len(), 4);
        assert!(app.drafts.is_empty());
        assert_eq!(app.selected, 0);
        // Edited requests count as unsaved until saved or reverted.
        app.collection.requests[0].url = "http://a.test".into();
        assert!(app.unsaved_changes(0));
        let saved = app.baseline[&0].clone();
        app.collection.requests[0] = saved;
        assert!(!app.unsaved_changes(0));
        app.tabs = vec![0, 2];
        app.close_tabs(&[0, 2], None);
        assert!(app.tabs.is_empty());
    }
    #[test]
    fn request_edits_wait_for_save_while_structure_autosaves() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let req = |name: &str| Request {
            name: name.into(),
            ..Default::default()
        };
        app.collection.requests = vec![req("A"), req("B")];
        app.save().unwrap();
        let on_disk = |app: &App| {
            let text = std::fs::read_to_string(app.project.join("collection.json")).unwrap();
            post_ronda::collection::import(&text).unwrap().requests
        };
        app.collection.requests[0].url = "http://a.test".into();
        app.collection.requests[1].url = "http://b.test".into();
        assert!(app.request_unsaved(0) && app.request_unsaved(1));
        assert_eq!(app.unsaved_count(), 2);
        // A rename autosaves without dragging the unsaved URL edits along.
        app.collection.requests[0].name = "A renamed".into();
        assert!(app.pending_structural());
        app.write(SaveScope::Structural).unwrap();
        let disk = on_disk(&app);
        assert_eq!(disk[0].name, "A renamed");
        assert!(disk[0].url.is_empty() && disk[1].url.is_empty());
        assert!(app.request_unsaved(0) && !app.pending_structural());
        // Save writes only the selected request.
        app.write(SaveScope::Request(0)).unwrap();
        let disk = on_disk(&app);
        assert_eq!(disk[0].url, "http://a.test");
        assert!(disk[1].url.is_empty());
        assert!(!app.request_unsaved(0) && app.request_unsaved(1));
        assert_eq!(app.unsaved_count(), 1);
        // Collection settings are not request content and autosave.
        app.collection
            .variables
            .push(post_ronda::environment::Variable {
                enabled: true,
                name: "k".into(),
                value: "v".into(),
            });
        assert!(app.pending_structural());
        // A draft with content counts as unsaved; an empty one does not.
        app.new_request();
        assert_eq!(app.unsaved_count(), 1);
        let draft = app.collection.requests.len() - 1;
        app.collection.requests[draft].url = "http://c.test".into();
        assert_eq!(app.unsaved_count(), 2);
        app.write(SaveScope::Everything).unwrap();
        assert_eq!(on_disk(&app).len(), 2);
    }
    fn folder_paths(app: &App) -> Vec<String> {
        app.collection
            .folders
            .iter()
            .map(|m| m.path.clone())
            .collect()
    }
    /// What the sidebar would list at the top level, in order (folders and requests).
    fn top_level(app: &App) -> Vec<String> {
        let entries = app.tree_entries("");
        ordered_slots(
            &entries,
            0,
            &app.collection.requests,
            &app.collection.pinned,
        )
        .into_iter()
        .map(|k| {
            let (_, path) = &entries[k];
            path[0].to_owned()
        })
        .collect()
    }
    #[test]
    fn empty_folders_have_a_place_in_the_list_and_can_be_moved_like_any_other() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let req = |name: &str| Request {
            name: name.into(),
            ..Default::default()
        };
        app.collection.requests = vec![req("A"), req("B"), req("C")];
        app.tabs = vec![0];
        app.selected = 0;
        app.drafts.clear();
        // Several folders at the root, created one after another, land at the end in order.
        app.add_folder("Satu".into());
        app.add_folder("Dua".into());
        assert_eq!(top_level(&app), ["A", "B", "C", "Satu", "Dua"]);
        // Empty folders reorder among themselves ...
        app.apply_move(
            Dragged::Folder("Dua".into()),
            DropTarget::BeforeFolder("Satu".into()),
        );
        assert_eq!(top_level(&app), ["A", "B", "C", "Dua", "Satu"]);
        app.apply_move(
            Dragged::Folder("Dua".into()),
            DropTarget::AfterFolder("Satu".into()),
        );
        assert_eq!(top_level(&app), ["A", "B", "C", "Satu", "Dua"]);
        // ... and can be placed among requests.
        app.apply_move(Dragged::Folder("Dua".into()), DropTarget::Before(1));
        assert_eq!(top_level(&app), ["A", "Dua", "B", "C", "Satu"]);
        app.apply_move(Dragged::Folder("Satu".into()), DropTarget::After(1));
        assert_eq!(top_level(&app), ["A", "Dua", "B", "Satu", "C"]);
        // A request can land right before or after an empty folder.
        app.apply_move(Dragged::Request(2), DropTarget::BeforeFolder("Dua".into()));
        assert_eq!(top_level(&app), ["A", "C", "Dua", "B", "Satu"]);
        let names: Vec<_> = app
            .collection
            .requests
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(names.len(), 3);
        // Into an empty folder, and back out to the root.
        app.apply_move(
            Dragged::Folder("Satu".into()),
            DropTarget::IntoFolder("Dua".into()),
        );
        assert!(folder_paths(&app).contains(&"Dua / Satu".to_string()));
        app.apply_move(Dragged::Folder("Dua / Satu".into()), DropTarget::Root);
        assert!(folder_paths(&app).contains(&"Satu".to_string()));
        assert_eq!(
            top_level(&app)
                .iter()
                .filter(|n| *n == "Satu" || *n == "Dua")
                .count(),
            2
        );
        // Deleting a request keeps empty folders where they were relative to the others.
        app.collection.folders = vec![
            post_ronda::collection::FolderMark::new("X", 1),
            post_ronda::collection::FolderMark::new("Y", 2),
        ];
        app.remove_request(0);
        assert_eq!(app.collection.folders[0].before, 0);
        assert_eq!(app.collection.folders[1].before, 1);
    }
    #[test]
    fn empty_folder_positions_survive_saving_and_loading() {
        let mut c = Collection::default();
        c.requests = vec![
            Request {
                name: "A".into(),
                ..Default::default()
            },
            Request {
                name: "B".into(),
                ..Default::default()
            },
        ];
        c.folders = vec![
            post_ronda::collection::FolderMark::new("Satu", 1),
            post_ronda::collection::FolderMark::new("Dua", 1),
            post_ronda::collection::FolderMark::new("Tiga", 2),
        ];
        let json = post_ronda::collection::export(&c);
        let order: Vec<_> = json["item"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(order, ["A", "Satu", "Dua", "B", "Tiga"]);
        let again = post_ronda::collection::import(&json.to_string()).unwrap();
        assert_eq!(again.folders, c.folders);
    }
    #[test]
    fn bulk_edit_text_round_trips_and_keeps_descriptions() {
        let mut described = Header::new("name", "Arif");
        described.description = "who".into();
        let mut off = Header::new("age", "35");
        off.enabled = false;
        let text = rows_to_bulk(&[described.clone(), off]);
        assert_eq!(text, "name:Arif\n//age:35");
        let rows = bulk_to_rows(
            "name: Arif R\n\n//age:35\nurl:http://a.test:80/x\n:novalue\n",
            &[described],
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(
            (rows[0].value.as_str(), rows[0].description.as_str()),
            ("Arif R", "who")
        );
        assert!(!rows[1].enabled && rows[1].name == "age");
        assert_eq!(rows[2].value, "http://a.test:80/x");
    }
    #[test]
    fn body_tab_renders_every_mode_and_bulk_edit_writes_back() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.tab = 1;
        for mode in [
            BodyMode::None,
            BodyMode::FormData,
            BodyMode::UrlEncoded,
            BodyMode::Raw,
            BodyMode::Binary,
        ] {
            app.collection.requests[0].body_mode = mode;
            draw(&ctx, &mut app, vec![]);
        }
        app.collection.requests[0].body_mode = BodyMode::UrlEncoded;
        app.begin_bulk("urlencoded");
        app.bulk_buf = "name:Arif\nage:35".into();
        let previous = app.bulk_rows("urlencoded");
        let rows = bulk_to_rows(&app.bulk_buf, &previous);
        app.set_bulk_rows("urlencoded", rows);
        assert_eq!(
            app.collection.requests[0].urlencoded_body(),
            "name=Arif&age=35"
        );
        draw(&ctx, &mut app, vec![]);
        // Params read and write the URL query.
        app.collection.requests[0].url = "http://a.test/p?x=1&y=2".into();
        app.begin_bulk("params");
        assert_eq!(app.bulk_buf, "x:1\ny:2");
        app.set_bulk_rows("params", bulk_to_rows("x:9\nz:a b", &app.param_rows()));
        assert_eq!(app.collection.requests[0].url, "http://a.test/p?x=9&z=a+b");
        app.tab = 0;
        draw(&ctx, &mut app, vec![]);
        app.tab = 2;
        app.begin_bulk("headers");
        draw(&ctx, &mut app, vec![]);
    }
    #[test]
    fn pasted_get_without_a_body_opens_params_and_settings_has_a_shortcut() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        draw(&ctx, &mut app, vec![]);
        app.paste_curl("curl 'https://a.test/search?q=1'").unwrap();
        assert_eq!(app.tab, 0);
        assert!(!app.settings);
        let comma = egui::Event::Key {
            key: egui::Key::Comma,
            physical_key: Some(egui::Key::Comma),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        };
        draw(&ctx, &mut app, vec![comma]);
        // The shortcut opens the settings menu, which then asks which settings are meant.
        assert!(!app.settings_chooser && !app.settings && !app.project_settings);
        app.open_project_settings(5);
        assert!(app.project_settings);
        assert_eq!(app.collection_tab, 5);
    }
    #[test]
    fn every_theme_layout_and_wrapping_mode_renders() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let long = format!("{{\"token\":\"{}\",\"ok\":true}}", "x".repeat(900));
        app.response = Some(post_ronda::http::Response {
            status: "HTTP/1.1 200 OK".into(),
            headers: "HTTP/1.1 200 OK\ncontent-type: application/json".into(),
            body: long.clone(),
            raw: long.clone().into_bytes(),
            elapsed: std::time::Duration::from_millis(5),
            bytes: long.len(),
            truncated: false,
            binary: false,
            content_type: "application/json".into(),
            file_name: String::new(),
        });
        for dark in [false, true] {
            let count = if dark {
                style::DARK_THEMES.len()
            } else {
                style::LIGHT_THEMES.len()
            };
            for index in 0..count {
                app.prefs.appearance = if dark {
                    Appearance::Dark
                } else {
                    Appearance::Light
                };
                if dark {
                    app.prefs.dark_theme = index;
                } else {
                    app.prefs.light_theme = index;
                }
                style::apply(&ctx, &app.prefs);
                app.store_preferences(&ctx);
                for (stacked, wrap) in [(false, true), (true, true), (false, false), (true, false)]
                {
                    app.prefs.stacked_layout = stacked;
                    app.prefs.wrap_response = wrap;
                    app.settings = false;
                    draw(&ctx, &mut app, vec![]);
                }
                app.settings = true;
                for tab in [1, 2] {
                    app.prefs_tab = tab;
                    draw(&ctx, &mut app, vec![]);
                }
            }
        }
        assert!(app.code_line_len.iter().any(|&n| n > 900));
    }
    #[test]
    fn saved_responses_get_sensible_names_and_sizes() {
        let response = |ct: &str, file: &str, binary: bool| post_ronda::http::Response {
            status: "HTTP/1.1 200 OK".into(),
            headers: String::new(),
            body: String::new(),
            raw: vec![],
            elapsed: std::time::Duration::ZERO,
            bytes: 0,
            truncated: false,
            binary,
            content_type: ct.into(),
            file_name: file.into(),
        };
        let xlsx = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
        assert_eq!(
            suggested_file_name(
                &response(xlsx, "Laporan Mei.xlsx", true),
                "https://a.test/x"
            ),
            "Laporan Mei.xlsx"
        );
        assert_eq!(
            suggested_file_name(&response(xlsx, "", true), "https://a.test/api/export?x=1"),
            "export.xlsx"
        );
        assert_eq!(
            suggested_file_name(
                &response("application/json", "", false),
                "https://a.test/users/"
            ),
            "users.json"
        );
        assert_eq!(
            suggested_file_name(&response("", "", true), "https://a.test/"),
            "response.bin"
        );
        assert_eq!(
            suggested_file_name(
                &response("image/png", "../../etc/x.png", true),
                "https://a.test/"
            ),
            ".._.._etc_x.png"
        );
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1_048_576), "5.0 MB");
    }
    #[test]
    fn dragging_reorders_and_moves_requests_between_folders() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let req = |name: &str| Request {
            name: name.into(),
            ..Default::default()
        };
        let names = |app: &App| -> Vec<String> {
            app.collection
                .requests
                .iter()
                .map(|r| r.name.clone())
                .collect()
        };
        app.collection.requests = vec![req("A"), req("B"), req("C")];
        app.tabs = vec![0, 2];
        app.selected = 2;
        // Reorder: C goes before A; tabs and the selection keep pointing at the same requests.
        app.apply_move(Dragged::Request(2), DropTarget::Before(0));
        assert_eq!(names(&app), ["C", "A", "B"]);
        assert_eq!(app.selected, 0);
        assert_eq!(app.tabs, [1, 0]);
        // Dropping on itself changes nothing.
        app.apply_move(Dragged::Request(1), DropTarget::After(1));
        assert_eq!(names(&app), ["C", "A", "B"]);
        // Into a folder: lands after the folder's last request and takes its prefix.
        app.collection.requests = vec![req("F / x"), req("y"), req("F / z"), req("w")];
        app.tabs = vec![1];
        app.selected = 1;
        app.apply_move(Dragged::Request(1), DropTarget::IntoFolder("F".into()));
        assert_eq!(names(&app), ["F / x", "F / z", "F / y", "w"]);
        assert_eq!(app.selected, 2);
        assert_eq!(app.tabs, [2]);
        // Next to a request in another folder adopts that folder; to the root drops the prefix.
        app.apply_move(Dragged::Request(3), DropTarget::Before(1));
        assert_eq!(names(&app), ["F / x", "F / w", "F / z", "F / y"]);
        app.apply_move(Dragged::Request(0), DropTarget::Root);
        assert_eq!(names(&app), ["F / w", "F / z", "F / y", "x"]);
        // Folders move with everything inside, and never into themselves.
        app.collection.requests = vec![req("G / z"), req("F / a"), req("F / b / c"), req("d")];
        app.tabs = vec![0];
        app.selected = 0;
        app.collection.folders = vec![post_ronda::collection::FolderMark::new("F / empty", 1)];
        app.apply_move(
            Dragged::Folder("F".into()),
            DropTarget::IntoFolder("G".into()),
        );
        assert_eq!(names(&app), ["G / z", "G / F / a", "G / F / b / c", "d"]);
        assert_eq!(folder_paths(&app), ["G / F / empty"]);
        app.apply_move(
            Dragged::Folder("G".into()),
            DropTarget::IntoFolder("G / F".into()),
        );
        assert_eq!(names(&app), ["G / z", "G / F / a", "G / F / b / c", "d"]);
        app.apply_move(Dragged::Folder("G / F".into()), DropTarget::Root);
        assert_eq!(names(&app), ["G / z", "d", "F / a", "F / b / c"]);
        // Moved requests are written under their new names by the next autosave.
        assert!(app.dirty.is_some());
    }
    #[test]
    fn pinned_items_float_to_the_top_of_their_level_and_follow_renames() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let req = |name: &str| Request {
            name: name.into(),
            ..Default::default()
        };
        app.collection.requests = vec![req("A"), req("B"), req("F / x"), req("F / y"), req("C")];
        app.tabs = vec![0];
        app.selected = 0;
        let entries: Vec<(Option<usize>, Vec<&str>)> = app
            .collection
            .requests
            .iter()
            .enumerate()
            .map(|(i, r)| (Some(i), r.name.split(" / ").collect()))
            .collect();
        let rq = &app.collection.requests;
        let order = |pinned: &[&str]| {
            let pinned: Vec<String> = pinned.iter().map(|s| s.to_string()).collect();
            ordered_slots(&entries, 0, rq, &pinned)
        };
        assert_eq!(order(&[]), [0, 1, 2, 4]);
        assert_eq!(order(&["C"]), [4, 0, 1, 2]);
        assert_eq!(order(&["F"]), [2, 0, 1, 4]);
        assert_eq!(order(&["C", "F"]), [2, 4, 0, 1]);
        // Inside the folder only that level is reordered.
        let inner: Vec<_> = entries[2..4].to_vec();
        assert_eq!(ordered_slots(&inner, 1, rq, &["F / y".to_string()]), [1, 0]);
        // Pins survive renames and moves of what they point at.
        app.toggle_pin("F / y");
        app.toggle_pin("B");
        assert!(app.is_pinned("B"));
        app.rename_folder("F", "G");
        assert!(app.is_pinned("G / y") && !app.is_pinned("F / y"));
        app.apply_move(Dragged::Request(1), DropTarget::IntoFolder("G".into()));
        assert!(app.is_pinned("G / B"));
        app.toggle_pin("G / B");
        assert!(!app.is_pinned("G / B"));
        app.delete_folder_now("G");
        assert!(app.collection.pinned.is_empty());
    }
    #[test]
    fn folders_can_be_reordered_by_dropping_next_to_them() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let req = |name: &str| Request {
            name: name.into(),
            ..Default::default()
        };
        let fresh = |app: &mut App| {
            app.collection.requests = vec![req("A"), req("F / x"), req("G / y"), req("B")];
            app.tabs = vec![0];
            app.selected = 0;
        };
        let names = |app: &App| -> Vec<String> {
            app.collection
                .requests
                .iter()
                .map(|r| r.name.clone())
                .collect()
        };
        fresh(&mut app);
        app.apply_move(
            Dragged::Folder("G".into()),
            DropTarget::BeforeFolder("F".into()),
        );
        assert_eq!(names(&app), ["A", "G / y", "F / x", "B"]);
        fresh(&mut app);
        app.apply_move(
            Dragged::Folder("F".into()),
            DropTarget::AfterFolder("G".into()),
        );
        assert_eq!(names(&app), ["A", "G / y", "F / x", "B"]);
        // A request can land before or after a folder as its sibling.
        fresh(&mut app);
        app.apply_move(Dragged::Request(3), DropTarget::BeforeFolder("F".into()));
        assert_eq!(names(&app), ["A", "B", "F / x", "G / y"]);
        fresh(&mut app);
        app.apply_move(Dragged::Request(0), DropTarget::AfterFolder("G".into()));
        assert_eq!(names(&app), ["F / x", "G / y", "A", "B"]);
        // A folder cannot be placed next to something inside itself.
        fresh(&mut app);
        app.collection.requests[1].name = "F / sub / x".into();
        app.apply_move(
            Dragged::Folder("F".into()),
            DropTarget::BeforeFolder("F / sub".into()),
        );
        assert_eq!(names(&app), ["A", "F / sub / x", "G / y", "B"]);
        // Nested folders keep their parent when reordered among siblings.
        app.collection.requests = vec![req("P / a"), req("P / Q / b"), req("P / R / c")];
        app.apply_move(
            Dragged::Folder("P / R".into()),
            DropTarget::BeforeFolder("P / Q".into()),
        );
        assert_eq!(names(&app), ["P / a", "P / R / c", "P / Q / b"]);
    }
    #[test]
    fn import_curl_dialog_reports_errors_and_imports_on_command_enter() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let command_enter = || egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: Some(egui::Key::Enter),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        };
        let before = app.collection.requests.len();
        app.import_open = true;
        app.curl_text = "this is not curl".into();
        draw(&ctx, &mut app, vec![]);
        draw(&ctx, &mut app, vec![command_enter()]);
        assert!(app.import_open, "a failed import keeps the dialog open");
        assert!(!app.import_error.is_empty());
        assert_eq!(app.collection.requests.len(), before);
        app.curl_text =
            "curl https://a.test/items -H 'Content-Type: application/json' -d '{\"a\":1}'".into();
        draw(&ctx, &mut app, vec![command_enter()]);
        assert!(!app.import_open);
        assert!(app.import_error.is_empty() && app.curl_text.is_empty());
        let imported = app.collection.requests.last().unwrap();
        assert_eq!(imported.url, "https://a.test/items");
        assert_eq!(app.tab, 1, "a request with a body opens on the Body tab");
    }
    #[test]
    fn search_shortcuts_open_the_palette_and_focus_the_sidebar_filter() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let key = |key| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        };
        draw(&ctx, &mut app, vec![]);
        draw(&ctx, &mut app, vec![key(egui::Key::P)]);
        assert!(app.search_open, "Cmd+P opens the search palette like Cmd+K");
        app.search_open = false;
        app.sidebar_visible = false;
        draw(&ctx, &mut app, vec![key(egui::Key::F)]);
        assert!(app.sidebar_visible, "Cmd+F reveals a hidden sidebar");
        draw(&ctx, &mut app, vec![]);
        assert!(
            !app.focus_filter,
            "the request to focus the filter is consumed once"
        );
    }
    #[test]
    fn projects_live_in_their_own_folders_and_switching_starts_clean() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let root = tempfile::tempdir().unwrap();
        app.np_parent = root.path().to_owned();
        // A new project gets its own folder and an empty tree.
        app.begin_new_project(None);
        assert!(app.np_open);
        app.np_name = "Billing".into();
        app.create_project().unwrap();
        let billing = root.path().join("Billing");
        assert_eq!(app.project, billing);
        assert_eq!(app.collection.name, "Billing");
        assert!(billing.join("collection.json").exists());
        assert!(
            app.drafts.contains(&0),
            "the placeholder request stays out of the sidebar"
        );
        assert!(
            app.projects
                .iter()
                .any(|p| p.path == billing && p.name == "Billing")
        );
        // The same folder cannot be claimed twice.
        app.begin_new_project(None);
        app.np_name = "Billing".into();
        app.np_parent = root.path().to_owned();
        assert!(
            app.create_project()
                .unwrap_err()
                .contains("already holds a project")
        );
        // Put a saved request in Billing, then start another project.
        app.collection.requests.push(Request {
            name: "Invoices / List".into(),
            url: "https://a.test/invoices".into(),
            ..Default::default()
        });
        app.save().unwrap();
        app.np_name = "Shipping".into();
        app.create_project().unwrap();
        assert_eq!(app.project, root.path().join("Shipping"));
        assert_eq!(app.collection.name, "Shipping");
        assert!(
            app.collection
                .requests
                .iter()
                .all(|r| r.name != "Invoices / List")
        );
        assert_eq!(app.tabs, [0]);
        // Switching back loads Billing exactly as saved, with its own baseline.
        app.switch_project(billing.clone());
        assert_eq!(app.collection.name, "Billing");
        let list = app
            .collection
            .requests
            .iter()
            .position(|r| r.name == "Invoices / List")
            .unwrap();
        assert!(!app.request_unsaved(list) && !app.drafts.contains(&list));
        // Unsaved edits block the switch instead of being lost.
        app.collection.requests[list].url = "https://changed.test".into();
        app.switch_project(root.path().join("Shipping"));
        assert_eq!(app.project, billing);
        assert!(app.error);
        app.collection.requests[list].url = "https://a.test/invoices".into();
        // Entries can be dropped from the list, except the open project.
        let shipping = root.path().join("Shipping");
        app.remove_project_entry(&shipping);
        app.remove_project_entry(&billing);
        assert!(!app.projects.iter().any(|p| p.path == shipping));
        assert!(app.projects.iter().any(|p| p.path == billing));
        assert!(
            shipping.join("collection.json").exists(),
            "files are never deleted"
        );
    }
    #[test]
    fn a_postman_collection_can_become_a_project() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let postman = r#"{"info":{"name":"Shop API","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
            "item":[{"name":"Orders","item":[{"name":"List","request":{"method":"GET","url":{"raw":"https://shop.test/orders"}}}]},
                    {"name":"Ping","request":{"method":"GET","url":"https://shop.test/ping"}}]}"#;
        let imported = post_ronda::collection::import(postman).unwrap();
        let root = tempfile::tempdir().unwrap();
        app.np_parent = root.path().to_owned();
        app.begin_new_project(Some(imported));
        assert_eq!(app.np_name, "Shop API");
        app.create_project().unwrap();
        assert_eq!(app.project, root.path().join("Shop API"));
        let names: Vec<_> = app
            .collection
            .requests
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(names, ["Orders / List", "Ping"]);
        assert!(
            app.drafts.is_empty(),
            "an imported project shows all of its requests"
        );
        let on_disk = std::fs::read_to_string(app.project.join("collection.json")).unwrap();
        assert_eq!(
            post_ronda::collection::import(&on_disk)
                .unwrap()
                .requests
                .len(),
            2
        );
    }
    #[test]
    fn importing_can_add_to_the_open_project_instead_of_making_a_new_one() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let source = |vars: &str| {
            post_ronda::collection::import(&format!(
                r#"{{"info":{{"name":"Shop API","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"}},
                "variable":[{vars}],
                "item":[{{"name":"Orders","item":[{{"name":"List","request":{{"method":"GET","url":"https://shop.test/orders"}}}}]}},
                        {{"name":"Ping","request":{{"method":"GET","url":"https://shop.test/ping"}}}}]}}"#
            ))
            .unwrap()
        };
        // Added to an existing project: its own requests and name stay.
        app.collection.name = "Billing".into();
        app.collection.requests = vec![Request {
            name: "Existing".into(),
            ..Default::default()
        }];
        app.collection.variables = vec![post_ronda::environment::Variable {
            enabled: true,
            name: "base".into(),
            value: "one".into(),
        }];
        app.drafts.clear();
        let added = app
            .merge_collection(source(
                r#"{"key":"base","value":"one"},{"key":"token","value":"t"}"#,
            ))
            .unwrap();
        assert_eq!(added, 2);
        let names: Vec<_> = app
            .collection
            .requests
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(names, ["Existing", "Orders / List", "Ping"]);
        assert_eq!(app.collection.name, "Billing");
        assert_eq!(app.collection.variables.len(), 2);
        assert!(app.dirty.is_some());
        // A conflicting variable value is reported and nothing is added.
        let before = app.collection.requests.len();
        let err = app
            .merge_collection(source(r#"{"key":"base","value":"two"}"#))
            .unwrap_err();
        assert!(err.contains("base"));
        assert_eq!(app.collection.requests.len(), before);
        // A brand new project only holds its hidden placeholder, which makes way for the import.
        app.collection = Collection::default();
        app.drafts = [0].into();
        app.merge_collection(source("")).unwrap();
        let names: Vec<_> = app
            .collection
            .requests
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(names, ["Orders / List", "Ping"]);
        assert!(app.drafts.is_empty());
        // The dialog remembers the choice and starts on "new project".
        app.begin_new_project(Some(source("")));
        assert!(!app.np_merge);
        app.np_merge = true;
        draw(&ctx, &mut app, vec![]);
        assert!(app.np_open && app.np_merge);
    }
    #[test]
    fn response_pane_renders_while_a_request_is_running() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.running = true;
        app.sent_at = Some(Instant::now());
        draw(&ctx, &mut app, vec![]);
        draw(&ctx, &mut app, vec![]);
        app.running = false;
    }
    #[test]
    fn search_palette_filters_and_enter_opens_request() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.collection.requests.push(Request {
            name: "Create invoice".into(),
            url: "https://a.test/invoices".into(),
            ..Default::default()
        });
        app.search_open = true;
        draw(&ctx, &mut app, vec![]);
        app.palette_query = "invoice".into();
        draw(&ctx, &mut app, vec![]);
        let key = |key| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        draw(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        assert!(!app.search_open);
        assert_eq!(app.selected, app.collection.requests.len() - 1);
    }
}
