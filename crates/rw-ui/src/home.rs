//! Home screen: create, open, duplicate and delete projects.

use crate::format;
use crate::layout::grid;
use crate::textures::Textures;
use crate::theme::{HEADING_FAMILY, Tokens};
use eframe::egui::{
    self, Align, Button, Color32, CornerRadius, FontFamily, FontId, Layout, RichText, Sense,
    Stroke, StrokeKind, Vec2,
};
use rw_model::library::ProjectSummary;
use rw_model::{AspectPreset, Canvas, app};
use std::path::PathBuf;
use std::time::SystemTime;

/// Label of the primary button; also used by UI tests.
pub const NEW_PROJECT_LABEL: &str = "New project";
pub const OPEN_OTHER_LABEL: &str = "Open from folder…";
pub const CREATE_LABEL: &str = "Create project";
pub const CANCEL_LABEL: &str = "Cancel";
pub const DELETE_CONFIRM_LABEL: &str = "Delete project";
pub const DUPLICATE_LABEL: &str = "Duplicate";
pub const DELETE_LABEL: &str = "Delete…";
pub const THEME_TOGGLE_LABEL: &str = "Toggle light/dark";
/// Message shown when there are no projects.
pub const EMPTY_RECENTS_LABEL: &str = "No projects yet. Start one and it will show up here.";
pub const CUSTOM_LABEL: &str = "Custom";

/// What the user asked for on the home screen this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum HomeAction {
    Create {
        name: String,
        canvas: Canvas,
    },
    Open(PathBuf),
    OpenOther,
    Duplicate(PathBuf),
    Delete(PathBuf),
    ToggleTheme,
    OpenSettings,
    /// Show a crash report in the file manager.
    RevealReport(PathBuf),
}

pub const SETTINGS_LABEL: &str = "Settings";
pub const REOPEN_LABEL: &str = "Open it again";
pub const SHOW_REPORT_LABEL: &str = "Show crash report";

pub const ABOUT_LABEL: &str = "About";
pub const LICENCES_LABEL: &str = "Open-source licences";

/// The About window: what the app is and whose work it includes.
pub fn about(ctx: &egui::Context, open: &mut bool, tokens: Tokens) {
    if !*open {
        return;
    }
    let modal = egui::Modal::new(egui::Id::new("about")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.label(heading(app::NAME, 22.0).color(tokens.text));
        ui.label(RichText::new(format!("Version {}", app::VERSION)).color(tokens.text_muted));
        ui.label("A video editor that works entirely on this computer.");
        ui.add_space(8.0);
        egui::Grid::new("credits")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for (what, who) in rw_assets::CREDITS {
                    ui.label(RichText::new(*what).strong());
                    ui.add(egui::Label::new(*who).wrap());
                    ui.end_row();
                }
            });
        ui.add_space(6.0);
        egui::CollapsingHeader::new("Every bundled file and its licence").show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(rw_assets::ATTRIBUTION).small().monospace())
                            .wrap(),
                    );
                });
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Close").clicked() {
                *open = false;
            }
            // The licences of every library compiled into the app, and
            // FFmpeg's; shipped in the download (not in builds from source).
            let licences = crate::reveal::licences_dir();
            let notices = licences
                .as_ref()
                .map(|d| d.join("THIRD_PARTY_LICENSES.html"))
                .filter(|f| f.is_file());
            if let Some(page) = notices
                && ui.button(LICENCES_LABEL).clicked()
            {
                let _ = crate::reveal::open(&page);
            }
            if let Some(dir) = licences
                && ui.button("Licence files").clicked()
            {
                let _ = crate::reveal::open(&dir);
            }
        });
    });
    if modal.should_close() {
        *open = false;
    }
}

/// State of the "New project" dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct NewProjectDialog {
    pub name: String,
    /// `None` means custom size.
    pub preset: Option<AspectPreset>,
    pub custom: (u32, u32),
}

impl Default for NewProjectDialog {
    fn default() -> Self {
        NewProjectDialog {
            name: "Untitled project".into(),
            preset: Some(AspectPreset::Widescreen16x9),
            custom: (1920, 1080),
        }
    }
}

impl NewProjectDialog {
    pub fn canvas(&self) -> Canvas {
        match self.preset {
            Some(p) => Canvas::from_preset(p),
            None => Canvas::custom(self.custom.0, self.custom.1),
        }
    }
}

#[derive(Default)]
pub struct HomeState {
    /// The About window is open.
    pub about: bool,
    pub projects: Vec<ProjectSummary>,
    pub new_dialog: Option<NewProjectDialog>,
    pub confirm_delete: Option<(PathBuf, String)>,
    pub error: Option<String>,
    /// The last run didn't close properly.
    pub unclean: Option<crate::recovery::Unclean>,
}

/// Draws a rectangle with the canvas's aspect ratio inside `size`.
fn ratio_glyph(ui: &mut egui::Ui, w: u32, h: u32, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let scale = size * 0.8 / w.max(h) as f32;
    let r =
        egui::Rect::from_center_size(rect.center(), Vec2::new(w as f32 * scale, h as f32 * scale));
    ui.painter().rect_stroke(
        r,
        CornerRadius::same(2),
        Stroke::new(1.5, color),
        StrokeKind::Inside,
    );
}

fn heading(text: &str, size: f32) -> RichText {
    RichText::new(text).font(FontId::new(size, FontFamily::Name(HEADING_FAMILY.into())))
}

pub fn show(
    ui: &mut egui::Ui,
    state: &mut HomeState,
    textures: &mut Textures,
    tokens: Tokens,
) -> Option<HomeAction> {
    let mut action = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.add_space(28.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(980.0);
                ui.horizontal(|ui| {
                    ui.label(heading(app::NAME, 28.0).color(tokens.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(THEME_TOGGLE_LABEL).clicked() {
                            action = Some(HomeAction::ToggleTheme);
                        }
                        if ui.button(ABOUT_LABEL).clicked() {
                            state.about = true;
                        }
                        if ui.button(SETTINGS_LABEL).clicked() {
                            action = Some(HomeAction::OpenSettings);
                        }
                    });
                });
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    let new = Button::new(
                        RichText::new(NEW_PROJECT_LABEL)
                            .color(tokens.on_accent)
                            .size(16.0),
                    )
                    .fill(tokens.accent)
                    .min_size(egui::vec2(160.0, 40.0));
                    if ui.add(new).clicked() {
                        state.new_dialog = Some(NewProjectDialog::default());
                    }
                    if ui
                        .add(Button::new(OPEN_OTHER_LABEL).min_size(egui::vec2(0.0, 40.0)))
                        .clicked()
                    {
                        action = Some(HomeAction::OpenOther);
                    }
                });
                if let Some(u) = state.unclean.clone() {
                    ui.add_space(16.0);
                    egui::Frame::new()
                        .fill(tokens.surface_raised)
                        .corner_radius(CornerRadius::same(tokens.radius))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.label(
                                RichText::new(format!("{} didn't close properly last time.", app::NAME))
                                    .strong()
                                    .color(tokens.text),
                            );
                            if let Some(p) = &u.project {
                                let name = p.file_name().unwrap_or_default().to_string_lossy();
                                ui.label(format!(
                                    "\u{201c}{name}\u{201d} was saved automatically a few seconds before."
                                ));
                            }
                            ui.horizontal(|ui| {
                                if let Some(p) = &u.project
                                    && ui.button(REOPEN_LABEL).clicked()
                                {
                                    action = Some(HomeAction::Open(p.clone()));
                                }
                                if let Some(r) = &u.report
                                    && ui.button(SHOW_REPORT_LABEL).clicked()
                                {
                                    action = Some(HomeAction::RevealReport(r.clone()));
                                }
                                if ui.button("Dismiss").clicked() {
                                    state.unclean = None;
                                }
                            });
                            });
                        });
                }
                if let Some(err) = &state.error {
                    ui.add_space(12.0);
                    ui.label(RichText::new(err).color(tokens.danger));
                }
                ui.add_space(28.0);
                ui.horizontal(|ui| ui.label(heading("Recent projects", 18.0)));
                ui.separator();
                ui.add_space(12.0);
                if state.projects.is_empty() {
                    ui.label(RichText::new(EMPTY_RECENTS_LABEL).color(tokens.text_muted));
                } else {
                    let now = SystemTime::now();
                    let mut ask_delete = None;
                    grid(
                        ui,
                        &state.projects,
                        CARD_OUTER_W,
                        16.0,
                        |ui, p| match project_card(ui, p, textures, tokens, now) {
                            Some(CardAction::Home(a)) => action = Some(a),
                            Some(CardAction::AskDelete) => {
                                ask_delete = Some((p.dir.clone(), p.name.clone()));
                            }
                            None => {}
                        },
                    );
                    if ask_delete.is_some() {
                        state.confirm_delete = ask_delete;
                    }
                }
            });
        });

    if let Some(a) = new_project_modal(ui.ctx(), state, tokens) {
        action = Some(a);
    }
    if let Some(a) = delete_modal(ui.ctx(), state, tokens) {
        action = Some(a);
    }
    action
}

enum CardAction {
    Home(HomeAction),
    /// Delete was chosen from the card menu; confirm first.
    AskDelete,
}

const CARD_W: f32 = 220.0;
/// Card width including its frame margin and stroke.
const CARD_OUTER_W: f32 = CARD_W + 18.0;
const THUMB_H: f32 = 124.0;

fn project_card(
    ui: &mut egui::Ui,
    p: &ProjectSummary,
    textures: &mut Textures,
    tokens: Tokens,
    now: SystemTime,
) -> Option<CardAction> {
    let mut action = None;
    let frame = egui::Frame::new()
        .fill(tokens.surface_raised)
        .stroke(Stroke::new(1.0, tokens.border))
        .corner_radius(CornerRadius::same(tokens.radius))
        .inner_margin(egui::Margin::same(8));
    let resp = frame
        .show(ui, |ui| {
            ui.set_width(CARD_W);
            ui.vertical(|ui| {
                let (rect, thumb_resp) =
                    ui.allocate_exact_size(egui::vec2(CARD_W, THUMB_H), Sense::click());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(4), tokens.preview_backdrop);
                match p.thumbnail.as_ref().and_then(|t| textures.get(ui.ctx(), t)) {
                    Some(tex) => {
                        let size = tex.size_vec2();
                        let scale = (rect.width() / size.x).min(rect.height() / size.y);
                        let r = egui::Rect::from_center_size(rect.center(), size * scale);
                        ui.painter().image(
                            tex.id(),
                            r,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    None => {
                        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                        child.centered_and_justified(|ui| {
                            ratio_glyph(
                                ui,
                                p.canvas.width,
                                p.canvas.height,
                                56.0,
                                tokens.text_muted,
                            );
                        });
                    }
                }
                if thumb_resp.clicked() {
                    action = Some(CardAction::Home(HomeAction::Open(p.dir.clone())));
                }
                ui.add_space(6.0);
                let name = ui.add(
                    Button::new(RichText::new(&p.name).strong())
                        .frame(false)
                        .truncate(),
                );
                if name.clicked() {
                    action = Some(CardAction::Home(HomeAction::Open(p.dir.clone())));
                }
                let ratio = p.canvas.preset().map_or_else(
                    || format!("{}×{}", p.canvas.width, p.canvas.height),
                    |r| r.ratio_label().to_owned(),
                );
                ui.label(
                    RichText::new(format!(
                        "{ratio} · {} · {} media",
                        format::duration(p.duration),
                        p.media_count
                    ))
                    .small()
                    .color(tokens.text_muted),
                );
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Edited {}", format::ago(p.modified, now)))
                            .small()
                            .color(tokens.text_muted),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.menu_button("…", |ui| {
                            if ui.button(DUPLICATE_LABEL).clicked() {
                                action =
                                    Some(CardAction::Home(HomeAction::Duplicate(p.dir.clone())));
                                ui.close();
                            }
                            if ui.button(DELETE_LABEL).clicked() {
                                action = Some(CardAction::AskDelete);
                                ui.close();
                            }
                        });
                    });
                });
            });
        })
        .response;
    resp.on_hover_text(p.dir.display().to_string());
    action
}

fn new_project_modal(
    ctx: &egui::Context,
    state: &mut HomeState,
    tokens: Tokens,
) -> Option<HomeAction> {
    let dialog = state.new_dialog.as_mut()?;
    let mut action = None;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("new-project")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.label(heading(NEW_PROJECT_LABEL, 20.0));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label("Name");
            crate::layout::name(
                ui.add(egui::TextEdit::singleline(&mut dialog.name).desired_width(400.0)),
                "Project name",
            );
        });
        ui.add_space(12.0);
        ui.label("Shape");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for preset in AspectPreset::ALL {
                let (w, h) = preset.size();
                let selected = dialog.preset == Some(preset);
                if preset_tile(
                    ui,
                    preset.ratio_label(),
                    preset.description(),
                    w,
                    h,
                    selected,
                    tokens,
                )
                .clicked()
                {
                    dialog.preset = Some(preset);
                }
            }
            let (cw, ch) = dialog.custom;
            if preset_tile(
                ui,
                CUSTOM_LABEL,
                "Any size",
                cw,
                ch,
                dialog.preset.is_none(),
                tokens,
            )
            .clicked()
            {
                dialog.preset = None;
            }
        });
        if dialog.preset.is_none() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let range = Canvas::MIN_SIDE..=Canvas::MAX_SIDE;
                ui.label("Width");
                crate::layout::name(
                    ui.add(
                        egui::DragValue::new(&mut dialog.custom.0)
                            .range(range.clone())
                            .suffix(" px"),
                    ),
                    "Width",
                );
                ui.label("Height");
                crate::layout::name(
                    ui.add(
                        egui::DragValue::new(&mut dialog.custom.1)
                            .range(range)
                            .suffix(" px"),
                    ),
                    "Height",
                );
            });
        }
        let canvas = dialog.canvas();
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!(
                "{} × {} at {} fps",
                canvas.width,
                canvas.height,
                canvas.frame_rate.as_f64()
            ))
            .color(tokens.text_muted),
        );
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let valid = !dialog.name.trim().is_empty();
            let create = Button::new(RichText::new(CREATE_LABEL).color(tokens.on_accent))
                .fill(tokens.accent);
            if ui.add_enabled(valid, create).clicked() {
                action = Some(HomeAction::Create {
                    name: dialog.name.trim().to_owned(),
                    canvas,
                });
                close = true;
            }
            if ui.button(CANCEL_LABEL).clicked() {
                close = true;
            }
        });
    });
    if close || modal.should_close() {
        state.new_dialog = None;
    }
    action
}

fn preset_tile(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    w: u32,
    h: u32,
    selected: bool,
    tokens: Tokens,
) -> egui::Response {
    let size = egui::vec2(96.0, 108.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, title)
    });
    let (fill, stroke) = if selected {
        (tokens.surface_hover, Stroke::new(2.0, tokens.accent))
    } else if resp.hovered() {
        (tokens.surface_hover, Stroke::new(1.0, tokens.border))
    } else {
        (tokens.surface_raised, Stroke::new(1.0, tokens.border))
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(tokens.radius),
        fill,
        stroke,
        StrokeKind::Inside,
    );
    let glyph = egui::Rect::from_min_size(rect.min + egui::vec2(24.0, 8.0), egui::vec2(48.0, 48.0));
    let scale = 40.0 / w.max(h) as f32;
    let r = egui::Rect::from_center_size(
        glyph.center(),
        egui::vec2(w as f32 * scale, h as f32 * scale),
    );
    painter.rect_stroke(
        r,
        CornerRadius::same(2),
        Stroke::new(1.5, tokens.text),
        StrokeKind::Inside,
    );
    painter.text(
        egui::pos2(rect.center().x, rect.min.y + 70.0),
        egui::Align2::CENTER_CENTER,
        title,
        FontId::new(14.0, FontFamily::Name(HEADING_FAMILY.into())),
        tokens.text,
    );
    painter.text(
        egui::pos2(rect.center().x, rect.min.y + 90.0),
        egui::Align2::CENTER_CENTER,
        subtitle,
        FontId::proportional(11.0),
        tokens.text_muted,
    );
    resp
}

fn delete_modal(ctx: &egui::Context, state: &mut HomeState, tokens: Tokens) -> Option<HomeAction> {
    let (dir, name) = state.confirm_delete.clone()?;
    let mut action = None;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("delete-project")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.label(heading(&format!("Delete “{name}”?"), 18.0));
        ui.add_space(8.0);
        ui.label("The project file and its cache will be removed. Your original media files are not touched.");
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let del = Button::new(RichText::new(DELETE_CONFIRM_LABEL).color(Color32::WHITE)).fill(tokens.danger);
            if ui.add(del).clicked() {
                action = Some(HomeAction::Delete(dir.clone()));
                close = true;
            }
            if ui.button(CANCEL_LABEL).clicked() {
                close = true;
            }
        });
    });
    if close || modal.should_close() {
        state.confirm_delete = None;
    }
    action
}
