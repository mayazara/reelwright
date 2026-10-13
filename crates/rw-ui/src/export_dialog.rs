//! The export dialog: what to make, then progress, then the result.

use crate::format;
use crate::theme::Tokens;
use eframe::egui::{self, RichText};
use rw_engine::export::{
    ExportFormat, ExportReport, ExportSettings, Quality, Resolution, bit_rate,
};
use rw_media::VideoCodec;
use rw_model::{FrameRate, ItemId, Project, Time};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

/// Longest GIF the dialog makes.
pub const GIF_MAX: Time = Time::from_secs(30);

pub const EXPORT_LABEL: &str = "Export";
pub const REVEAL_LABEL: &str = "Reveal in folder";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Video,
    Gif,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Whole,
    Selection,
    Markers,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stage {
    Setup,
    /// Asked whether to replace an existing file.
    Overwrite(ExportSettings),
    Running,
    Done(ExportReport),
    Failed(String),
}

/// What the dialog asks the editor to do.
#[derive(Debug, Clone, PartialEq)]
pub enum DialogAction {
    Start(ExportSettings),
    Cancel,
    Close,
    Reveal(PathBuf),
    ChooseFolder,
}

/// Frame rates offered besides the project's own.
const RATES: &[(u32, u32, &str)] = &[
    (24, 1, "24"),
    (25, 1, "25"),
    (30, 1, "30"),
    (50, 1, "50"),
    (60, 1, "60"),
];
const GIF_RATES: &[u32] = &[10, 15, 20, 25];

pub struct ExportDialog {
    pub stage: Stage,
    kind: Kind,
    resolution: Resolution,
    upscale: bool,
    codec: VideoCodec,
    /// `None` = pick automatically.
    encoder: Option<String>,
    quality: Quality,
    /// `None` = the project's rate.
    rate: Option<FrameRate>,
    gif_fps: u32,
    range: Range,
    pub folder: PathBuf,
    name: String,
    advanced: bool,
    /// Encoders that work here, per codec; filled in the background.
    encoders: Option<HashMap<VideoCodec, Vec<String>>>,
    probe: Option<crossbeam_channel::Receiver<HashMap<VideoCodec, Vec<String>>>>,
}

impl std::fmt::Debug for ExportDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportDialog")
            .field("stage", &self.stage)
            .finish()
    }
}

/// A short, friendly name for an FFmpeg encoder.
pub fn encoder_label(name: &str) -> String {
    let vendor = if name.ends_with("_videotoolbox") {
        "Apple VideoToolbox (hardware)"
    } else if name.ends_with("_mf") {
        "Windows Media Foundation"
    } else if name.ends_with("_nvenc") {
        "NVIDIA graphics (hardware)"
    } else if name.ends_with("_qsv") {
        "Intel Quick Sync (hardware)"
    } else if name.ends_with("_amf") {
        "AMD graphics (hardware)"
    } else {
        match name {
            "libopenh264" => "OpenH264 (software)",
            "libx264" => "x264 (software)",
            "mpeg4" => "MPEG-4 Part 2 (basic, software)",
            "libvpx-vp9" => "libvpx (software)",
            "libsvtav1" => "SVT-AV1 (software)",
            "libaom-av1" => "AOM AV1 (software)",
            other => other,
        }
    };
    vendor.to_owned()
}

impl ExportDialog {
    pub fn new(project: &Project, folder: PathBuf) -> Self {
        let (tx, rx) = crossbeam_channel::bounded(1);
        // Trying hardware encoders takes a moment; don't block the UI.
        std::thread::Builder::new()
            .name("encoder-probe".into())
            .spawn(move || {
                let found: HashMap<VideoCodec, Vec<String>> = VideoCodec::ALL
                    .into_iter()
                    .map(|c| (c, rw_media::working_encoders(c)))
                    .collect();
                let _ = tx.send(found);
            })
            .ok();
        let canvas = (project.canvas.width, project.canvas.height);
        let short = canvas.0.min(canvas.1);
        // The largest preset that doesn't upscale, else the smallest.
        let resolution = Resolution::ALL
            .into_iter()
            .rev()
            .find(|r| r.lines() <= short)
            .unwrap_or(Resolution::P480);
        ExportDialog {
            stage: Stage::Setup,
            kind: Kind::Video,
            resolution,
            upscale: false,
            codec: VideoCodec::H264,
            encoder: None,
            quality: Quality::Balanced,
            rate: None,
            gif_fps: 15,
            range: Range::Whole,
            folder,
            name: rw_model::library::sanitize(&project.name),
            advanced: false,
            encoders: None,
            probe: Some(rx),
        }
    }

    fn format(&self) -> ExportFormat {
        match self.kind {
            Kind::Video => ExportFormat::Video { codec: self.codec },
            Kind::Gif => ExportFormat::Gif { fps: self.gif_fps },
            Kind::Audio => ExportFormat::Mp3,
        }
    }

    pub fn path(&self) -> PathBuf {
        let name = if self.name.trim().is_empty() {
            "Export"
        } else {
            self.name.trim()
        };
        self.folder
            .join(format!("{name}.{}", self.format().extension()))
    }

    /// The timeline span `range` covers, if it applies.
    fn span(range: Range, project: &Project, selection: &BTreeSet<ItemId>) -> Option<(Time, Time)> {
        match range {
            Range::Whole => Some((Time::ZERO, project.duration())),
            Range::Selection => {
                let items: Vec<_> = project
                    .items()
                    .filter(|i| selection.contains(&i.id))
                    .collect();
                let start = items.iter().map(|i| i.start).min()?;
                let end = items.iter().map(|i| i.end()).max()?;
                Some((start, end))
            }
            Range::Markers => {
                let mut at: Vec<Time> = project.markers.iter().map(|m| m.at).collect();
                at.sort();
                match (at.first(), at.last()) {
                    (Some(a), Some(b)) if b > a => Some((*a, *b)),
                    _ => None,
                }
            }
        }
    }

    /// Settings for what's chosen, or why it can't export yet.
    pub fn settings(
        &self,
        project: &Project,
        selection: &BTreeSet<ItemId>,
    ) -> Result<ExportSettings, String> {
        let (start, mut end) = Self::span(self.range, project, selection)
            .ok_or("That range is empty; pick another.")?;
        if end <= start {
            return Err("The timeline is empty.".into());
        }
        if self.kind == Kind::Gif {
            end = end.min(start + GIF_MAX);
        }
        let canvas = (project.canvas.width, project.canvas.height);
        let size = match self.kind {
            // GIFs are 480p, or the project's size if smaller.
            Kind::Gif if !Resolution::P480.upscales(canvas) => Resolution::P480.size_for(canvas),
            Kind::Gif => (canvas.0 & !1, canvas.1 & !1),
            _ => self.resolution.size_for(canvas),
        };
        let fps = self.rate.unwrap_or(project.canvas.frame_rate);
        let mut s = ExportSettings::for_project(project, &self.path());
        s.format = self.format();
        s.width = size.0;
        s.height = size.1;
        s.frame_rate = self.rate;
        s.video_bit_rate = bit_rate(size, fps, self.quality, self.codec);
        s.encoder = self.encoder.clone();
        s.range = Some((start, end));
        Ok(s)
    }

    fn poll_probe(&mut self) {
        if let Some(rx) = &self.probe
            && let Ok(found) = rx.try_recv()
        {
            self.encoders = Some(found);
            self.probe = None;
        }
    }

    /// Codecs that have a working encoder here (H.264 always offered).
    fn codecs(&self) -> Vec<VideoCodec> {
        VideoCodec::ALL
            .into_iter()
            .filter(|c| {
                *c == VideoCodec::H264
                    || self
                        .encoders
                        .as_ref()
                        .is_some_and(|e| e.get(c).is_some_and(|v| !v.is_empty()))
            })
            .collect()
    }
}

/// "30", "29.97", "23.976".
fn rate_label(r: FrameRate) -> String {
    let v = r.as_f64();
    if (v - v.round()).abs() < 1e-6 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.3}").trim_end_matches('0').to_owned()
    }
}

fn radio_row<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, options: &[(T, &str)]) {
    ui.horizontal_wrapped(|ui| {
        for (v, label) in options {
            ui.selectable_value(value, *v, *label);
        }
    });
}

/// Draws the dialog. `progress` is the running export's fraction and how
/// long it has run.
pub fn show(
    ctx: &egui::Context,
    d: &mut ExportDialog,
    project: &Project,
    selection: &BTreeSet<ItemId>,
    progress: Option<(f32, std::time::Duration)>,
    tokens: Tokens,
) -> Option<DialogAction> {
    d.poll_probe();
    if d.probe.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    let mut action = None;
    let modal = egui::Modal::new(egui::Id::new("export-dialog")).show(ctx, |ui| {
        ui.set_width(460.0);
        match d.stage.clone() {
            Stage::Setup => setup(ui, d, project, selection, tokens, &mut action),
            Stage::Overwrite(settings) => {
                ui.heading("Replace the existing file?");
                ui.label(format!(
                    "{} already exists. Exporting will replace it.",
                    settings.path.display()
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Replace").clicked() {
                        action = Some(DialogAction::Start(settings.clone()));
                    }
                    if ui.button("Back").clicked() {
                        d.stage = Stage::Setup;
                    }
                });
            }
            Stage::Running => {
                ui.heading("Exporting…");
                let (f, elapsed) = progress.unwrap_or((0.0, std::time::Duration::ZERO));
                ui.add(
                    egui::ProgressBar::new(f)
                        .desired_width(440.0)
                        .show_percentage(),
                );
                let left = if f > 0.02 {
                    let secs = elapsed.as_secs_f64() * f64::from(1.0 - f) / f64::from(f);
                    format!("About {} left", format::duration(Time::from_secs_f64(secs)))
                } else {
                    "Estimating time left…".into()
                };
                crate::layout::steady_text(
                    ui,
                    &left,
                    "About 00:00:00 left",
                    egui::FontId::proportional(12.0),
                    tokens.text_muted,
                );
                ui.add_space(8.0);
                if ui.button("Cancel export").clicked() {
                    action = Some(DialogAction::Cancel);
                }
            }
            Stage::Done(r) => {
                ui.heading("Export finished");
                egui::Grid::new("export-result")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("File");
                        ui.label(
                            r.path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        );
                        ui.end_row();
                        ui.label("Size");
                        ui.label(format::bytes(r.bytes));
                        ui.end_row();
                        ui.label("Length");
                        ui.label(format::duration(r.duration));
                        ui.end_row();
                        if !r.encoder.is_empty() {
                            ui.label("Encoder");
                            ui.label(encoder_label(&r.encoder));
                            ui.end_row();
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(REVEAL_LABEL).clicked() {
                        action = Some(DialogAction::Reveal(r.path.clone()));
                    }
                    if ui.button("Done").clicked() {
                        action = Some(DialogAction::Close);
                    }
                });
            }
            Stage::Failed(e) => {
                ui.heading("Export didn't finish");
                ui.label(RichText::new(e).color(tokens.danger));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Back").clicked() {
                        d.stage = Stage::Setup;
                    }
                    if ui.button("Close").clicked() {
                        action = Some(DialogAction::Close);
                    }
                });
            }
        }
    });
    if modal.should_close() && action.is_none() && !matches!(d.stage, Stage::Running) {
        action = Some(DialogAction::Close);
    }
    action
}

fn setup(
    ui: &mut egui::Ui,
    d: &mut ExportDialog,
    project: &Project,
    selection: &BTreeSet<ItemId>,
    tokens: Tokens,
    action: &mut Option<DialogAction>,
) {
    ui.heading("Export");
    ui.add_space(4.0);
    radio_row(
        ui,
        &mut d.kind,
        &[
            (Kind::Video, "Video"),
            (Kind::Gif, "GIF"),
            (Kind::Audio, "Sound only (MP3)"),
        ],
    );
    ui.separator();
    let canvas = (project.canvas.width, project.canvas.height);
    egui::Grid::new("export-setup")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            if d.kind == Kind::Video {
                ui.label("Resolution");
                ui.horizontal(|ui| {
                    for r in Resolution::ALL {
                        let allowed = !r.upscales(canvas) || d.upscale;
                        let size = r.size_for(canvas);
                        let resp = ui
                            .add_enabled(
                                allowed,
                                egui::Button::selectable(d.resolution == r, r.label()),
                            )
                            .on_hover_text(format!("{} × {}", size.0, size.1))
                            .on_disabled_hover_text(
                                "Larger than the project; tick \"Allow upscaling\" to use it",
                            );
                        if resp.clicked() {
                            d.resolution = r;
                        }
                    }
                });
                ui.end_row();
                if Resolution::ALL.iter().any(|r| r.upscales(canvas)) {
                    ui.label("");
                    if ui
                        .checkbox(&mut d.upscale, "Allow upscaling")
                        .on_hover_text("Export larger than the project's own size")
                        .changed()
                        && !d.upscale
                        && d.resolution.upscales(canvas)
                    {
                        d.resolution = Resolution::ALL
                            .into_iter()
                            .rev()
                            .find(|r| !r.upscales(canvas))
                            .unwrap_or(Resolution::P480);
                    }
                    ui.end_row();
                }
                ui.label("Quality");
                ui.horizontal(|ui| {
                    for q in Quality::ALL {
                        ui.selectable_value(&mut d.quality, q, q.label());
                    }
                });
                ui.end_row();
            }
            if d.kind == Kind::Gif {
                ui.label("Frames a second");
                ui.horizontal(|ui| {
                    for f in GIF_RATES {
                        ui.selectable_value(&mut d.gif_fps, *f, f.to_string());
                    }
                });
                ui.end_row();
            }
            ui.label("Range");
            ui.horizontal(|ui| {
                let options = [
                    (Range::Whole, "Whole project"),
                    (Range::Selection, "Selected clips"),
                    (Range::Markers, "Between first and last marker"),
                ];
                for (r, label) in options {
                    let ok = ExportDialog::span(r, project, selection).is_some_and(|(a, b)| b > a);
                    let resp = ui.add_enabled(ok, egui::Button::selectable(d.range == r, label));
                    if resp.clicked() {
                        d.range = r;
                    }
                }
            });
            ui.end_row();
            ui.label("Name");
            crate::layout::name(
                ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(300.0)),
                "File name",
            );
            ui.end_row();
            ui.label("Folder");
            ui.horizontal(|ui| {
                let shown = d.folder.display().to_string();
                ui.add(
                    egui::Label::new(RichText::new(shown).small().color(tokens.text_muted))
                        .truncate(),
                );
                if ui.button("Change…").clicked() {
                    *action = Some(DialogAction::ChooseFolder);
                }
            });
            ui.end_row();
        });

    if d.kind == Kind::Video {
        egui::CollapsingHeader::new("More options")
            .default_open(d.advanced)
            .show(ui, |ui| {
                d.advanced = true;
                egui::Grid::new("export-advanced")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Format");
                        let codecs = d.codecs();
                        {
                            let combo = egui::ComboBox::from_id_salt("export-codec")
                                .selected_text(d.codec.label())
                                .width(260.0)
                                .show_ui(ui, |ui| {
                                    for c in codecs {
                                        if ui.selectable_value(&mut d.codec, c, c.label()).clicked()
                                        {
                                            d.encoder = None;
                                        }
                                    }
                                });
                            crate::layout::name(combo.response.clone(), "Format");
                            combo
                        };
                        if d.probe.is_some() {
                            ui.spinner()
                                .on_hover_text("Checking which encoders work here");
                        }
                        ui.end_row();
                        ui.label("Encoder");
                        let list = d
                            .encoders
                            .as_ref()
                            .and_then(|e| e.get(&d.codec).cloned())
                            .unwrap_or_default();
                        let current = d
                            .encoder
                            .as_deref()
                            .map_or("Automatic (best available)".to_owned(), encoder_label);
                        {
                            let combo = egui::ComboBox::from_id_salt("export-encoder")
                                .selected_text(current)
                                .width(260.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut d.encoder,
                                        None,
                                        "Automatic (best available)",
                                    );
                                    for e in list {
                                        let label = encoder_label(&e);
                                        ui.selectable_value(&mut d.encoder, Some(e), label);
                                    }
                                });
                            crate::layout::name(combo.response.clone(), "Encoder");
                            combo
                        };
                        ui.end_row();
                        ui.label("Frame rate");
                        let current = d.rate.map_or_else(
                            || {
                                format!(
                                    "Same as project ({})",
                                    rate_label(project.canvas.frame_rate)
                                )
                            },
                            rate_label,
                        );
                        {
                            let combo = egui::ComboBox::from_id_salt("export-rate")
                                .selected_text(current)
                                .width(260.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut d.rate, None, "Same as project");
                                    for (n, den, label) in RATES {
                                        ui.selectable_value(
                                            &mut d.rate,
                                            Some(FrameRate::new(*n, *den)),
                                            *label,
                                        );
                                    }
                                });
                            crate::layout::name(combo.response.clone(), "Frame rate");
                            combo
                        };
                        ui.end_row();
                    });
            });
    }

    let settings = d.settings(project, selection);
    ui.add_space(6.0);
    match &settings {
        Ok(s) => {
            let len = s.range.map_or(project.duration(), |(a, b)| b - a);
            let mut note = format!("{} · {}", s.path.display(), format::duration(len));
            if d.kind == Kind::Gif
                && ExportDialog::span(d.range, project, selection)
                    .is_some_and(|(a, b)| b - a > GIF_MAX)
            {
                note.push_str(" · GIFs stop after 30 seconds");
            }
            ui.label(RichText::new(note).small().color(tokens.text_muted));
        }
        Err(e) => {
            ui.label(RichText::new(e).small().color(tokens.danger));
        }
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let ready = settings.is_ok();
        let go = egui::Button::new(RichText::new(EXPORT_LABEL).color(tokens.on_accent))
            .fill(tokens.accent);
        if ui.add_enabled(ready, go).clicked()
            && let Ok(s) = settings.clone()
        {
            if s.path.exists() {
                d.stage = Stage::Overwrite(s);
            } else {
                *action = Some(DialogAction::Start(s));
            }
        }
        if ui.button("Cancel").clicked() {
            *action = Some(DialogAction::Close);
        }
    });
}
