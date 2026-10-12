//! egui app: panels, timeline widget, preview, property inspector, dialogs.

pub mod actions;
pub mod backgrounds;
pub mod brand_kit;
pub mod canvas_tools;
mod editing;
pub mod export_dialog;
pub mod format;
pub mod graphics_library;
pub mod home;
pub mod icons;
pub mod inspector;
pub mod layout;
pub mod media_bin;
pub mod preview;
mod reveal;
pub mod session;
pub mod settings;
pub mod text_library;
pub mod textures;
pub mod theme;
pub mod timeline;
pub mod transitions;
pub mod workspace;

use actions::EditorAction;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};
use egui_dock::DockState;
use home::{HomeAction, HomeState};
use media_bin::{BinAction, BinState};
use preview::UiGpu;
use rw_audio::output::{NullFactory, OutputFactory};
use rw_edit::commands::{RemoveMedia, RenameMedia, RenameProject, SetCanvas};
use rw_engine::MediaJobs;
use rw_engine::export::{ExportReport, ExportSettings};
use rw_engine::import::scan_folder;
use rw_media::Ffmpeg;
use rw_model::library::{Library, ProjectSummary};
use rw_model::{AspectPreset, Canvas};
use session::Session;
use settings::Settings;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
pub const EXPORT_LABEL: &str = "Export";
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
    audio: Arc<dyn OutputFactory>,
    ui_gpu: Option<UiGpu>,
    export: Option<ExportJob>,
    export_dialog: Option<export_dialog::ExportDialog>,
}

/// An export running on a background thread.
struct ExportJob {
    started: Instant,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    result: crossbeam_channel::Receiver<Result<ExportReport, String>>,
}

impl ExportJob {
    fn fraction(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed))
    }
}

impl EditorUi {
    /// `ui_gpu` is the UI's wgpu device (absent in headless tests); `audio`
    /// opens the sound output for playback.
    pub fn new(
        ctx: &egui::Context,
        config: Config,
        ui_gpu: Option<UiGpu>,
        audio: Arc<dyn OutputFactory>,
    ) -> Self {
        ctx.set_fonts(theme::fonts());
        // Fonts people add to the `fonts` folder beside the settings.
        if let Some(dir) = config.settings_file.parent() {
            rw_text::set_user_fonts_dir(dir.join("fonts"));
            // Sticker packs people add: one folder of pictures per pack.
            rw_assets::stickers::set_user_packs_dir(dir.join("packs"));
            rw_assets::stickers::set_brand_kit_dir(dir.join("brand-kit"));
        }
        let settings = Settings::load(&config.settings_file);
        let waker_ctx = ctx.clone();
        let jobs = MediaJobs::new(
            Arc::new(Ffmpeg::new()),
            Arc::new(move || waker_ctx.request_repaint()),
        );
        let mut ui = EditorUi {
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
            audio,
            ui_gpu,
            export: None,
            export_dialog: None,
        };
        ui.reload_brand();
        ui
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
        match Session::open(dir, self.audio.clone(), self.ui_gpu.clone()) {
            Ok(mut s) => {
                s.prepare_all(&self.jobs);
                self.settings.touch_recent(dir);
                self.save_settings();
                self.session = Some(s);
                self.bin = BinState::default();
                self.reload_brand();
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
            BinAction::AddGenerated(fill) => {
                if let Some(s) = self.session.as_mut() {
                    s.apply(EditorAction::AppendGenerated(fill));
                }
            }
            BinAction::Brand(a) => self.handle_brand_action(a),
            BinAction::AddSound(name) => self.add_sound(name),
            BinAction::AddGraphic(item) => {
                if let Some(s) = self.session.as_mut() {
                    s.apply(EditorAction::AddGraphic(item));
                }
            }
            BinAction::AddText(text) => {
                if let Some(s) = self.session.as_mut() {
                    s.apply(EditorAction::AddText(text));
                }
            }
            BinAction::AddTransition(kind) => {
                if let Some(s) = self.session.as_mut() {
                    s.apply(EditorAction::AddTransition(kind));
                }
            }
            BinAction::AddToNewTrack(id) => {
                if let Some(s) = self.session.as_mut() {
                    s.apply(EditorAction::AppendMediaNewTrack(id));
                }
            }
            BinAction::AddToTimeline(id) => {
                if let Some(s) = self.session.as_mut() {
                    s.apply(EditorAction::AppendMedia(id));
                }
            }
        }
    }

    /// Applies an action to the open project (export opens its dialog).
    pub fn apply(&mut self, action: EditorAction) {
        if action == EditorAction::Export {
            self.open_export_dialog();
        } else if let Some(s) = self.session.as_mut() {
            s.apply(action);
        }
    }

    /// Puts bundled sound `name` at the playhead (importing it first).
    fn add_sound(&mut self, name: &str) {
        let Some(dir) = self.config.settings_file.parent().map(|d| d.join("sounds")) else {
            return;
        };
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let path = match rw_assets::sounds::materialise(name, &dir) {
            Ok(p) => p,
            Err(e) => {
                s.notify_error(format!("Couldn't add the sound: {e}"));
                return;
            }
        };
        let existing = s
            .project
            .media
            .iter()
            .find(|m| m.path == path)
            .map(|m| m.id);
        let id = match existing {
            Some(id) => id,
            None => match rw_engine::import::probe_file(&Ffmpeg::new(), &path) {
                Ok(m) => {
                    let id = m.id;
                    if s.execute(Box::new(rw_edit::commands::AddMedia::new(m)))
                        .is_err()
                    {
                        return;
                    }
                    id
                }
                Err(e) => {
                    s.notify_error(format!("Couldn't add the sound: {e}"));
                    return;
                }
            },
        };
        s.apply(EditorAction::AppendMedia(id));
    }

    /// The brand kit's folder, beside the settings.
    fn brand_dir(&self) -> PathBuf {
        self.config
            .settings_file
            .parent()
            .map_or_else(|| PathBuf::from("brand-kit"), |d| d.join("brand-kit"))
    }

    /// Loads the brand kit into the library (at start-up, when a project
    /// opens, and after changes).
    fn reload_brand(&mut self) {
        let dir = self.brand_dir();
        rw_assets::stickers::set_brand_kit_dir(dir.clone());
        self.bin.brand = rw_assets::brand::BrandKit::load(&dir);
        self.bin.brand_logos = rw_assets::brand::BrandKit::logos(&dir);
    }

    fn handle_brand_action(&mut self, action: brand_kit::BrandAction) {
        use brand_kit::BrandAction as B;
        let dir = self.brand_dir();
        let kit = &mut self.bin.brand;
        let mut changed = true;
        let mut result: std::io::Result<()> = Ok(());
        match action {
            B::AddColour(c) => kit.add_colour(c),
            B::RemoveColour(i) if i < kit.colours.len() => {
                kit.colours.remove(i);
            }
            B::AddFont(f) => kit.add_font(&f),
            B::RemoveFont(i) if i < kit.fonts.len() => {
                kit.fonts.remove(i);
            }
            B::ImportLogo => {
                changed = false;
                if let Some(file) = rfd::FileDialog::new()
                    .set_title("Add a logo")
                    .add_filter("Pictures", rw_assets::brand::LOGO_TYPES)
                    .pick_file()
                {
                    result = rw_assets::brand::BrandKit::add_logo(&dir, &file).map(|_| ());
                }
            }
            B::RemoveLogo(id) => {
                changed = false;
                result = rw_assets::brand::BrandKit::remove_logo(&dir, &id);
            }
            B::ApplyColour(c) => {
                changed = false;
                if let Some(s) = self.session.as_mut() {
                    s.apply(EditorAction::ApplyColour(c));
                }
            }
            B::ApplyFont(f) => {
                changed = false;
                if let Some(s) = self.session.as_mut() {
                    s.apply(EditorAction::ApplyFont(f));
                }
            }
            B::AddBrandText => {
                changed = false;
                let mut preset: actions::TextItem = rw_text::presets::plain().into();
                if let Some(f) = kit.fonts.first() {
                    preset.spec.style.font = f.clone();
                }
                if let Some(c) = kit.colours.first() {
                    preset.spec.style.color = *c;
                }
                preset.spec.text = "Your brand text".into();
                if let Some(s) = self.session.as_mut() {
                    s.apply(EditorAction::AddText(preset));
                }
            }
            B::AddLogo(id) => {
                changed = false;
                let item = rw_model::Item::sticker_item(
                    rw_model::StickerRef {
                        pack: rw_assets::brand::LOGO_PACK.into(),
                        id,
                    },
                    rw_model::Time::ZERO,
                );
                if let Some(s) = self.session.as_mut() {
                    s.apply(EditorAction::AddGraphic(item));
                }
            }
            _ => changed = false,
        }
        if changed {
            result = self.bin.brand.save(&dir);
        }
        if let Err(e) = result
            && let Some(s) = self.session.as_mut()
        {
            s.notify_error(format!("Brand kit: {e}"));
        }
        self.reload_brand();
    }

    /// Folder exports go to unless the user picks another.
    fn export_folder(&self) -> PathBuf {
        self.settings
            .export_dir
            .clone()
            .filter(|d| d.is_dir())
            .or_else(|| self.config.projects_dir.parent().map(Path::to_path_buf))
            .unwrap_or_default()
    }

    pub fn open_export_dialog(&mut self) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        if s.project.duration() <= rw_model::Time::ZERO {
            s.notify_error("Add something to the timeline before exporting.");
            return;
        }
        s.player.pause();
        let folder = self.export_folder();
        let project = &self.session.as_ref().expect("checked").project;
        let mut dialog = export_dialog::ExportDialog::new(project, folder);
        if self.export.is_some() {
            dialog.stage = export_dialog::Stage::Running;
        }
        self.export_dialog = Some(dialog);
    }

    /// The open export dialog, for tests.
    pub fn export_dialog(&self) -> Option<&export_dialog::ExportDialog> {
        self.export_dialog.as_ref()
    }

    fn show_export_dialog(&mut self, ctx: &egui::Context, tokens: Tokens) {
        let (Some(d), Some(s)) = (self.export_dialog.as_mut(), self.session.as_ref()) else {
            return;
        };
        let progress = self
            .export
            .as_ref()
            .map(|e| (e.fraction(), e.started.elapsed()));
        let action = export_dialog::show(ctx, d, &s.project, &s.selection, progress, tokens);
        match action {
            None => {}
            Some(export_dialog::DialogAction::Start(settings)) => {
                if let Some(dir) = settings.path.parent() {
                    self.settings.export_dir = Some(dir.to_path_buf());
                    let _ = self.settings.save(&self.config.settings_file);
                }
                self.start_export(settings);
            }
            Some(export_dialog::DialogAction::Cancel) => self.cancel_export(),
            Some(export_dialog::DialogAction::Close) => self.export_dialog = None,
            Some(export_dialog::DialogAction::Reveal(path)) => {
                if let Err(e) = reveal::reveal(&path)
                    && let Some(s) = self.session.as_mut()
                {
                    s.notify_error(format!("Couldn't open the folder: {e}"));
                }
            }
            Some(export_dialog::DialogAction::ChooseFolder) => {
                let start = d.folder.clone();
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("Export to folder")
                    .set_directory(start)
                    .pick_folder()
                    && let Some(d) = self.export_dialog.as_mut()
                {
                    d.folder = dir;
                }
            }
        }
    }

    /// Starts exporting the open project to `path` with default settings
    /// (whole project, canvas size, H.264).
    pub fn export_to(&mut self, path: PathBuf) {
        let Some(s) = self.session.as_ref() else {
            return;
        };
        let settings = ExportSettings::for_project(&s.project, &path);
        self.start_export(settings);
    }

    /// Starts an export in the background.
    pub fn start_export(&mut self, settings: ExportSettings) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        if self.export.is_some() {
            return;
        }
        if s.project.duration() <= rw_model::Time::ZERO {
            s.notify_error("Add something to the timeline before exporting.");
            return;
        }
        s.player.pause();
        let project = s.project.clone();
        let cache = s.cache.clone();
        let fallback_gpu = self.ui_gpu.as_ref().map(|g| g.gpu.clone());
        let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = crossbeam_channel::bounded(1);
        let (p, c) = (progress.clone(), cancel.clone());
        std::thread::Builder::new()
            .name("export".into())
            .spawn(move || {
                // A separate device keeps export from stalling the preview.
                let gpu = rw_render::Gpu::headless().ok().or(fallback_gpu);
                let result = match gpu {
                    None => Err("No graphics device is available for rendering.".to_owned()),
                    Some(gpu) => {
                        rw_engine::export::export(&project, &cache, &gpu, &settings, &c, &|f| {
                            p.store(f.to_bits(), Ordering::Relaxed);
                        })
                        .map_err(|e| e.to_string())
                    }
                };
                let _ = tx.send(result);
            })
            .expect("spawn export");
        self.export = Some(ExportJob {
            started: Instant::now(),
            progress,
            cancel,
            result: rx,
        });
        if let Some(d) = self.export_dialog.as_mut() {
            d.stage = export_dialog::Stage::Running;
        }
    }

    pub fn exporting(&self) -> Option<f32> {
        self.export.as_ref().map(ExportJob::fraction)
    }

    pub fn cancel_export(&mut self) {
        if let Some(e) = &self.export {
            e.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn poll_export(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.export else { return };
        match job.result.try_recv() {
            Ok(result) => {
                self.export = None;
                if let Some(d) = self.export_dialog.as_mut() {
                    d.stage = match result {
                        Ok(r) => export_dialog::Stage::Done(r),
                        Err(e) if e.contains("cancelled") => export_dialog::Stage::Setup,
                        Err(e) => export_dialog::Stage::Failed(e),
                    };
                    return;
                }
                if let Some(s) = self.session.as_mut() {
                    match result {
                        Ok(r) => s.notify(format!(
                            "Exported {} ({} frames, {}) to {}",
                            format::duration(r.duration),
                            r.frames,
                            r.encoder,
                            r.path.display()
                        )),
                        Err(e) if e.contains("cancelled") => s.notify("Export cancelled."),
                        Err(e) => s.notify_error(format!("Export failed: {e}")),
                    }
                }
            }
            Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
        }
    }

    /// Drains background job results into the session.
    fn pump_jobs(&mut self) {
        while let Some(event) = self.jobs.try_recv() {
            if let Some(s) = self.session.as_mut()
                && let Some(changed) = s.handle(&self.jobs, event)
            {
                self.textures.forget(&changed);
                s.peaks_cache.forget(&changed);
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
        for action in shortcuts(ctx) {
            self.apply(action);
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
            home::about(&ctx, &mut self.home.about, tokens);
            if let Some(a) = action {
                self.handle_home_action(a);
            }
            return;
        }

        let mut go_home = false;
        self.poll_export(&ctx);
        self.show_export_dialog(&ctx, tokens);
        self.top_bar(ui, tokens, &mut go_home);
        let session = self.session.as_mut().expect("checked above");
        session.tick_playback();
        let out = workspace::show(
            ui,
            &mut self.dock,
            session,
            &mut self.bin,
            &mut self.textures,
            tokens,
        );
        if let Some(a) = out.bin {
            self.handle_bin_action(a);
        }
        for a in out.actions {
            self.apply(a);
        }
        if go_home {
            self.close_project();
            return;
        }
        if let Some(s) = self.session.as_mut() {
            if s.player.is_playing() {
                ctx.request_repaint();
            }
            s.autosave(Instant::now());
            // Wake up for the idle autosave even if nothing else happens.
            if s.is_dirty() {
                ctx.request_repaint_after(session::AUTOSAVE_IDLE);
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, tokens: Tokens, go_home: &mut bool) {
        let mut start_export = false;
        self.top_bar_inner(ui, tokens, go_home, &mut start_export);
        if start_export {
            self.apply(EditorAction::Export);
        }
    }

    fn top_bar_inner(
        &mut self,
        ui: &mut egui::Ui,
        tokens: Tokens,
        go_home: &mut bool,
        start_export: &mut bool,
    ) {
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
                    ui.separator();
                    let exporting = self.export.as_ref().map(ExportJob::fraction);
                    match exporting {
                        None => {
                            let export = egui::Button::new(
                                RichText::new(EXPORT_LABEL).color(tokens.on_accent),
                            )
                            .fill(tokens.accent);
                            if ui
                                .add(export)
                                .on_hover_text("Export video (Ctrl+E)")
                                .clicked()
                            {
                                *start_export = true;
                            }
                        }
                        Some(f) => {
                            ui.add(
                                egui::ProgressBar::new(f)
                                    .desired_width(140.0)
                                    .show_percentage(),
                            );
                            if ui.button("Cancel").clicked()
                                && let Some(e) = &self.export
                            {
                                e.cancel.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let color = if s.save_error.is_some() {
                            tokens.danger
                        } else {
                            tokens.text_muted
                        };
                        // Fixed widths: the status and counter change often
                        // and shouldn't push their neighbours around.
                        let small = egui::FontId::proportional(11.0);
                        layout::steady_text(
                            ui,
                            &s.status(),
                            "All changes saved",
                            small.clone(),
                            color,
                        );
                        let busy = self.jobs.pending();
                        if busy > 0 {
                            ui.spinner();
                            layout::steady_text(
                                ui,
                                &format!("Preparing media ({busy})"),
                                "Preparing media (000)",
                                small,
                                tokens.text_muted,
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
        self.cancel_export();
        self.close_project();
        self.save_settings();
    }
}

/// Keyboard shortcuts from the plan, as actions. Undo/redo/save are handled
/// separately because they apply even with nothing selected.
fn shortcuts(ctx: &egui::Context) -> Vec<EditorAction> {
    use EditorAction as A;
    let cmd = Modifiers::COMMAND;
    let cmd_shift = Modifiers::COMMAND.plus(Modifiers::SHIFT);
    let table: [(Modifiers, Key, A); 18] = [
        (cmd_shift, Key::G, A::Ungroup),
        (cmd, Key::G, A::Group),
        (cmd, Key::D, A::Duplicate),
        (cmd, Key::E, A::Export),
        (cmd, Key::Equals, A::ZoomIn),
        (cmd, Key::Plus, A::ZoomIn),
        (cmd, Key::Minus, A::ZoomOut),
        (Modifiers::SHIFT, Key::Z, A::ZoomFit),
        (Modifiers::SHIFT, Key::Delete, A::Delete { ripple: true }),
        (Modifiers::NONE, Key::Delete, A::Delete { ripple: false }),
        (Modifiers::NONE, Key::Backspace, A::Delete { ripple: false }),
        (Modifiers::NONE, Key::Space, A::PlayPause),
        (Modifiers::NONE, Key::S, A::SplitAtPlayhead),
        (Modifiers::NONE, Key::M, A::AddMarker),
        (Modifiers::NONE, Key::ArrowLeft, A::StepFrames(-1)),
        (Modifiers::NONE, Key::ArrowRight, A::StepFrames(1)),
        (Modifiers::NONE, Key::ArrowUp, A::PrevEdit),
        (Modifiers::NONE, Key::ArrowDown, A::NextEdit),
    ];
    let mut out = Vec::new();
    ctx.input_mut(|i| {
        for (mods, key, action) in table {
            if i.consume_shortcut(&KeyboardShortcut::new(mods, key)) {
                out.push(action);
            }
        }
        // The platform turns Cmd/Ctrl+C and V into clipboard events.
        for e in &i.events {
            match e {
                egui::Event::Copy => out.push(A::Copy),
                egui::Event::Paste(_) => out.push(A::Paste),
                egui::Event::Key {
                    key: Key::C,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.command => out.push(A::Copy),
                egui::Event::Key {
                    key: Key::V,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.command => out.push(A::Paste),
                _ => {}
            }
        }
    });
    out
}

/// The eframe application.
pub struct App {
    ui: EditorUi,
}

impl App {
    /// Creates the app with silent (device-less) audio output.
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        Self::with_audio(cc, config, Arc::new(NullFactory::default()))
    }

    pub fn with_audio(
        cc: &eframe::CreationContext<'_>,
        config: Config,
        audio: Arc<dyn OutputFactory>,
    ) -> Self {
        let gpu = cc.wgpu_render_state.as_ref().map(UiGpu::from_render_state);
        Self {
            ui: EditorUi::new(&cc.egui_ctx, config, gpu, audio),
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
