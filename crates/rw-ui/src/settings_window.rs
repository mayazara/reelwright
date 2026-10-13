//! The Settings window.

use crate::settings::{PreviewQuality, Settings};
use crate::theme::{ThemeMode, Tokens};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};

pub const TITLE: &str = "Settings";
pub const CLEAR_CACHE_LABEL: &str = "Clear cache";

#[derive(Debug, Clone, PartialEq)]
pub enum SettingsAction {
    Theme(ThemeMode),
    PreviewQuality(PreviewQuality),
    ChooseExportFolder,
    ResetExportFolder,
    ClearCache,
    Reveal(PathBuf),
}

/// The window's state while open.
#[derive(Debug, Default)]
pub struct SettingsWindow {
    /// Size of the closed projects' caches, measured when opened.
    pub cache_bytes: u64,
}

/// Total size of the files under `dir`.
pub fn folder_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => folder_size(&e.path()),
            Ok(_) => e.metadata().map_or(0, |m| m.len()),
            Err(_) => 0,
        })
        .sum()
}

pub struct Paths<'a> {
    pub projects: &'a Path,
    pub reports: &'a Path,
    /// The project open now (its cache isn't cleared).
    pub open_project: Option<&'a Path>,
}

/// Shows the window while `open`.
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &SettingsWindow,
    settings: &Settings,
    theme: ThemeMode,
    paths: Paths<'_>,
    tokens: Tokens,
) -> Option<SettingsAction> {
    let mut action = None;
    let muted = |t: &str| RichText::new(t).small().color(tokens.text_muted);
    egui::Window::new(TITLE)
        .open(open)
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Grid::new("settings")
                .num_columns(2)
                .spacing([16.0, 10.0])
                .show(ui, |ui| {
                    ui.label("Appearance");
                    ui.horizontal(|ui| {
                        for (mode, label) in [(ThemeMode::Light, "Light"), (ThemeMode::Dark, "Dark")] {
                            if ui.selectable_label(theme == mode, label).clicked() && theme != mode {
                                action = Some(SettingsAction::Theme(mode));
                            }
                        }
                    });
                    ui.end_row();

                    ui.label("Preview quality");
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            for q in PreviewQuality::ALL {
                                if ui
                                    .selectable_label(settings.preview_quality == q, q.label())
                                    .clicked()
                                    && settings.preview_quality != q
                                {
                                    action = Some(SettingsAction::PreviewQuality(q));
                                }
                            }
                        });
                        ui.label(muted(
                            "Lower plays more smoothly on slow computers. Exports are always full quality.",
                        ));
                    });
                    ui.end_row();

                    ui.label("Export folder");
                    ui.vertical(|ui| {
                        let shown = settings
                            .export_dir
                            .as_ref()
                            .map_or_else(|| "Beside the projects folder".to_owned(), |d| d.display().to_string());
                        ui.add(egui::Label::new(shown).truncate());
                        ui.horizontal(|ui| {
                            if ui.button("Change…").clicked() {
                                action = Some(SettingsAction::ChooseExportFolder);
                            }
                            if settings.export_dir.is_some() && ui.button("Reset").clicked() {
                                action = Some(SettingsAction::ResetExportFolder);
                            }
                        });
                    });
                    ui.end_row();

                    ui.label("Projects");
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(paths.projects.display().to_string()).truncate());
                        if ui.button("Show in folder").clicked() {
                            action = Some(SettingsAction::Reveal(paths.projects.to_path_buf()));
                        }
                    });
                    ui.end_row();

                    ui.label("Cache");
                    ui.vertical(|ui| {
                        crate::layout::steady_text(
                            ui,
                            &crate::format::bytes(state.cache_bytes),
                            "000.0 MB",
                            egui::FontId::proportional(14.0),
                            tokens.text,
                        );
                        ui.label(muted(
                            "Thumbnails, sound waves and editing copies. They're made again when needed.",
                        ));
                        if paths.open_project.is_some() {
                            ui.label(muted("The open project's cache is kept."));
                        }
                        if ui
                            .add_enabled(state.cache_bytes > 0, egui::Button::new(CLEAR_CACHE_LABEL))
                            .clicked()
                        {
                            action = Some(SettingsAction::ClearCache);
                        }
                    });
                    ui.end_row();

                    if paths.reports.is_dir() {
                        ui.label("Crash reports");
                        if ui.button("Show in folder").clicked() {
                            action = Some(SettingsAction::Reveal(paths.reports.to_path_buf()));
                        }
                        ui.end_row();
                    }
                });
        });
    action
}
