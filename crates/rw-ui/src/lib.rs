//! egui app: panels, timeline widget, preview, property inspector, dialogs.

pub mod format;
pub mod home;
pub mod layout;
pub mod media_bin;
pub mod session;
pub mod settings;
pub mod textures;
pub mod theme;
pub mod workspace;

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};
use egui_dock::DockState;
use home::{HomeAction, HomeState};
use media_bin::{BinAction, BinState};
use rw_edit::commands::{RemoveMedia, RenameMedia, RenameProject, SetCanvas};
use rw_engine::MediaJobs;
use rw_engine::import::scan_folder;
use rw_media::Ffmpeg;
use rw_model::library::{Library, ProjectSummary};
use rw_model::{AspectPreset, Canvas};
use session::Session;
use settings::Settings;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use textures::Textures;
use theme::{ThemeMode, Tokens};
use workspace::Panel;

pub const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
pub const REDO: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
pub const REDO_ALT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
pub const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

pub const HOME_LABEL: &str = "Home";
pub const UNDO_LABEL: &str = "Undo";
pub const REDO_LABEL: &str = "Redo";

/// Where the app keeps its files. Tests point this at a temporary folder.
#[derive(Debug, Clone)]
pub struct Config {
    pub projects_dir: PathBuf,
    pub settings_file: PathBuf,
}

/// Which top-level screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Editor,
}

/// All UI state. Kept separate from [`eframe::App`] so tests can reach it.
pub struct EditorUi {
    pub theme: ThemeMode,
    theme_applied: Option<ThemeMode>,
    config: Config,
    library: Library,
    settings: Settings,
    pub home: HomeState,
    home_stale: bool,
    pub session: Option<Session>,
    pub bin: BinState,
    jobs: MediaJobs,
    textures: Textures,
    dock: DockState<Panel>,
    editing_name: Option<String>,
}

impl EditorUi {
    pub fn new(ctx: &egui::Context, config: Config) -> Self {
        ctx.set_fonts(theme::fonts());
        let settings = Settings::load(&config.settings_file);
        let waker_ctx = ctx.clone();
        let jobs = MediaJobs::new(
            Arc::new(Ffmpeg::new()),
            Arc::new(move || waker_ctx.request_repaint()),
        );
        EditorUi {
            theme: settings.theme.into(),
            theme_applied: None,
            library: Library::new(&config.projects_dir),
            config,
            settings,
            home: HomeState::default(),
            home_stale: true,
            session: None,
            bin: BinState::default(),
            jobs,
            textures: Textures::default(),
            dock: workspace::default_layout(),
            editing_name: None,
        }
    }

    pub fn screen(&self) -> Screen {
        if self.session.is_some() {
            Screen::Editor
        } else {
            Screen::Home
        }
    }

    pub fn jobs(&self) -> &MediaJobs {
        &self.jobs
    }

    fn save_settings(&self) {
        if let Err(e) = self.settings.save(&self.config.settings_file) {
            tracing::warn!(error = %e, "could not save settings");
        }
    }

    /// Re-reads the project list: everything in the projects folder plus
    /// recently opened projects stored elsewhere.
    pub fn refresh_home(&mut self) {
        let mut projects = self.library.list().unwrap_or_else(|e| {
            self.home.error = Some(format!("Couldn't read the projects folder: {e}"));
            Vec::new()
        });
        let known: HashSet<PathBuf> = projects.iter().map(|p| p.dir.clone()).collect();
        let mut gone = Vec::new();
        for dir in &self.settings.recent {
            if known.contains(dir) {
                continue;
            }
            match ProjectSummary::read(dir) {
                Ok(s) => projects.push(s),
                Err(_) => gone.push(dir.clone()),
            }
        }
        if !gone.is_empty() {
            for d in &gone {
                self.settings.forget_recent(d);
            }
            self.save_settings();
        }
        projects.sort_by_key(|a| std::cmp::Reverse(a.modified));
        self.home.projects = projects;
        self.textures.clear();
        self.home_stale = false;
    }

    /// Opens a project folder in the editor.
    pub fn open_project(&mut self, dir: &Path) {
        self.close_project();
        match Session::open(dir) {
            Ok(mut s) => {
                s.prepare_all(&self.jobs);
                self.settings.touch_recent(dir);
                self.save_settings();
                self.session = Some(s);
                self.bin = BinState::default();
                self.home.error = None;
            }
            Err(e) => {
                self.home.error = Some(format!("Couldn't open {}: {e}", dir.display()));
            }
        }
    }

    pub fn create_project(&mut self, name: &str, canvas: Canvas) {
        match self.library.create(name, canvas) {
            Ok((dir, _)) => self.open_project(&dir),
            Err(e) => self.home.error = Some(format!("Couldn't create the project: {e}")),
        }
    }

    /// Saves and closes the open project, returning to the home screen.
    pub fn close_project(&mut self) {
        if let Some(mut s) = self.session.take() {
            if s.is_dirty() {
                s.save();
            }
            self.jobs.cancel_all();
        }
        self.textures.clear();
        self.editing_name = None;
        self.home_stale = true;
    }

    /// Imports files and folders (folders are scanned for media).
    pub fn import_paths(&mut self, paths: Vec<PathBuf>) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let mut files = Vec::new();
        for p in paths {
            if p.is_dir() {
                match scan_folder(&p) {
                    Ok(found) if found.is_empty() => {
                        session.notify(format!("No media found in {}.", p.display()));
                    }
                    Ok(found) => files.extend(found),
                    Err(e) => session.notify_error(format!("Couldn't read {}: {e}", p.display())),
                }
            } else {
                files.push(p);
            }
        }
        session.import(&self.jobs, files);
    }

    fn handle_home_action(&mut self, action: HomeAction) {
        match action {
            HomeAction::Create { name, canvas } => self.create_project(&name, canvas),
            HomeAction::Open(dir) => self.open_project(&dir),
            HomeAction::OpenOther => {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("Open a project folder")
                    .pick_folder()
                {
                    self.open_project(&dir);
                }
            }
            HomeAction::Duplicate(dir) => {
                if let Err(e) = self.library.duplicate(&dir) {
                    self.home.error = Some(format!("Couldn't duplicate: {e}"));
                }
                self.home_stale = true;
            }
            HomeAction::Delete(dir) => {
                let result = if dir.parent() == Some(self.library.root()) {
                    self.library.delete(&dir)
                } else {
                    // Projects outside the projects folder are only forgotten.
                    Ok(())
                };
                if let Err(e) = result {
                    self.home.error = Some(format!("Couldn't delete: {e}"));
                }
                self.settings.forget_recent(&dir);
                self.save_settings();
                self.home_stale = true;
            }
            HomeAction::ToggleTheme => {
                self.theme = self.theme.toggled();
                self.settings.theme = self.theme.into();
                self.save_settings();
            }
        }
    }

    fn handle_bin_action(&mut self, action: BinAction) {
        match action {
            BinAction::ImportFiles => {
                if let Some(files) = rfd::FileDialog::new()
                    .set_title("Import media")
                    .add_filter("Media", rw_engine::import::MEDIA_EXTENSIONS)
                    .add_filter("All files", &["*"])
                    .pick_files()
                {
                    self.import_paths(files);
                }
            }
            BinAction::ImportFolder => {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("Import a folder")
                    .pick_folder()
                {
                    self.import_paths(vec![dir]);
                }
            }
            BinAction::Remove(id) => {
                if let Some(s) = self.session.as_mut() {
                    let _ = s.execute(Box::new(RemoveMedia::new(id)));
                }
            }
            BinAction::Rename(id, name) => {
                if let Some(s) = self.session.as_mut() {
                    let _ = s.execute(Box::new(RenameMedia::new(id, name)));
                }
            }
        }
    }

    /// Drains background job results into the session.
    fn pump_jobs(&mut self) {
        while let Some(event) = self.jobs.try_recv() {
            if let Some(s) = self.session.as_mut()
                && let Some(changed) = s.handle(&self.jobs, event)
            {
                self.textures.forget(&changed);
            }
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() && self.session.is_some() {
            self.import_paths(dropped);
        }
        let typing = ctx.egui_wants_keyboard_input();
        let Some(s) = self.session.as_mut() else {
            return;
        };
        if typing {
            return;
        }
        // Redo shortcuts first: Cmd+Shift+Z also matches a plain Cmd+Z check.
        if ctx.input_mut(|i| i.consume_shortcut(&REDO) || i.consume_shortcut(&REDO_ALT)) {
            s.redo();
        } else if ctx.input_mut(|i| i.consume_shortcut(&UNDO)) {
            s.undo();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&SAVE)) {
            s.save();
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.theme_applied != Some(self.theme) {
            theme::apply(&ctx, self.theme);
            self.theme_applied = Some(self.theme);
        }
        let tokens = Tokens::for_mode(self.theme);
        self.pump_jobs();
        self.handle_input(&ctx);

        if self.session.is_none() {
            if self.home_stale {
                self.refresh_home();
            }
            let mut action = None;
            egui::Frame::central_panel(ui.style()).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                action = home::show(ui, &mut self.home, &mut self.textures, tokens);
            });
            if let Some(a) = action {
                self.handle_home_action(a);
            }
            return;
        }

        let mut go_home = false;
        self.top_bar(ui, tokens, &mut go_home);
        let session = self.session.as_ref().expect("checked above");
        let bin_action = workspace::show(
            ui,
            &mut self.dock,
            session,
            &mut self.bin,
            &mut self.textures,
            tokens,
        );
        if let Some(a) = bin_action {
            self.handle_bin_action(a);
        }
        if go_home {
            self.close_project();
            return;
        }
        if let Some(s) = self.session.as_mut() {
            s.autosave(Instant::now());
            // Wake up for the idle autosave even if nothing else happens.
            if s.is_dirty() {
                ctx.request_repaint_after(session::AUTOSAVE_IDLE);
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, tokens: Tokens, go_home: &mut bool) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        egui::Frame::new()
            .fill(tokens.surface)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(HOME_LABEL).clicked() {
                        *go_home = true;
                    }
                    ui.separator();
                    match &mut self.editing_name {
                        Some(name) => {
                            let r = ui.add(egui::TextEdit::singleline(name).desired_width(220.0));
                            r.request_focus();
                            if r.lost_focus() {
                                let new = name.trim().to_owned();
                                if !new.is_empty() && new != s.project.name {
                                    let _ = s.execute(Box::new(RenameProject::new(new)));
                                }
                                self.editing_name = None;
                            }
                        }
                        None => {
                            let r = ui
                                .add(
                                    egui::Button::new(RichText::new(&s.project.name).strong())
                                        .frame(false),
                                )
                                .on_hover_text("Click to rename");
                            if r.clicked() {
                                self.editing_name = Some(s.project.name.clone());
                            }
                        }
                    }
                    ui.separator();
                    let canvas = s.project.canvas;
                    let current = canvas.preset().map_or_else(
                        || format!("{}×{}", canvas.width, canvas.height),
                        |p| p.ratio_label().to_owned(),
                    );
                    let mut chosen = None;
                    egui::ComboBox::from_id_salt("canvas-preset")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for p in AspectPreset::ALL {
                                let label = format!("{}  {}", p.ratio_label(), p.description());
                                if ui
                                    .selectable_label(canvas.preset() == Some(p), label)
                                    .clicked()
                                {
                                    chosen = Some(p);
                                }
                            }
                        });
                    if let Some(p) = chosen {
                        let mut next = Canvas::from_preset(p);
                        next.frame_rate = canvas.frame_rate;
                        next.background = canvas.background;
                        if next != canvas {
                            let _ = s.execute(Box::new(SetCanvas::new(next)));
                        }
                    }
                    ui.separator();
                    let undo = ui
                        .add_enabled(s.history.can_undo(), egui::Button::new(UNDO_LABEL))
                        .on_hover_text(s.history.undo_label().unwrap_or_default());
                    if undo.clicked() {
                        s.undo();
                    }
                    let redo = ui
                        .add_enabled(s.history.can_redo(), egui::Button::new(REDO_LABEL))
                        .on_hover_text(s.history.redo_label().unwrap_or_default());
                    if redo.clicked() {
                        s.redo();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let color = if s.save_error.is_some() {
                            tokens.danger
                        } else {
                            tokens.text_muted
                        };
                        ui.label(RichText::new(s.status()).small().color(color));
                        let busy = self.jobs.pending();
                        if busy > 0 {
                            ui.spinner();
                            ui.label(
                                RichText::new(format!("Preparing media ({busy})"))
                                    .small()
                                    .color(tokens.text_muted),
                            );
                        }
                    });
                });
                let mut dismiss = None;
                for (i, n) in s.notices.iter().enumerate().rev().take(3) {
                    ui.horizontal(|ui| {
                        let color = if n.error {
                            tokens.danger
                        } else {
                            tokens.text_muted
                        };
                        ui.label(RichText::new(&n.text).small().color(color));
                        if ui.small_button("Dismiss").clicked() {
                            dismiss = Some(i);
                        }
                    });
                }
                if let Some(i) = dismiss {
                    s.notices.remove(i);
                }
            });
    }

    /// Saves any open project. Called when the app is closing.
    pub fn shutdown(&mut self) {
        self.close_project();
        self.save_settings();
    }
}

/// The eframe application.
pub struct App {
    ui: EditorUi,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        Self {
            ui: EditorUi::new(&cc.egui_ctx, config),
        }
    }

    pub fn editor(&self) -> &EditorUi {
        &self.ui
    }

    pub fn editor_mut(&mut self) -> &mut EditorUi {
        &mut self.ui
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui.ui(ui);
    }

    fn on_exit(&mut self) {
        self.ui.shutdown();
    }
}
