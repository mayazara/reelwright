//! Recording: the Record tab (what to record) and the recording window
//! (countdown, time, sound level, pause, stop, retake).

use crate::theme::Tokens;
use eframe::egui::{self, RichText};
use rw_capture::clock::TimeSource;
use rw_capture::devices::{self, VideoDevice};
use rw_capture::sources::{self, AudioDevice, AudioInput};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const START_LABEL: &str = "Start recording";
pub const STOP_LABEL: &str = "Stop";
pub const PAUSE_LABEL: &str = "Pause";
pub const RESUME_LABEL: &str = "Resume";
pub const RETAKE_LABEL: &str = "Retake";

/// Width of the recording window's buttons.
const BUTTON_WIDTH: f32 = 80.0;

/// Frames a second for screens and cameras.
pub const FPS: u32 = 30;
/// Seconds counted down before recording starts.
pub const COUNTDOWN: u64 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Mic {
    #[default]
    Off,
    Default,
    Named(String),
}

/// Picture size asked of the camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraSize {
    /// Whatever the camera gives by default.
    #[default]
    Default,
    P720,
    P1080,
}

impl CameraSize {
    pub const ALL: [CameraSize; 3] = [CameraSize::Default, CameraSize::P720, CameraSize::P1080];

    pub fn label(self) -> &'static str {
        match self {
            CameraSize::Default => "Camera's own",
            CameraSize::P720 => "720p",
            CameraSize::P1080 => "1080p",
        }
    }

    fn video_size(self) -> Option<&'static str> {
        match self {
            CameraSize::Default => None,
            CameraSize::P720 => Some("1280x720"),
            CameraSize::P1080 => Some("1920x1080"),
        }
    }
}

/// The devices on this computer, read when the Record tab opens.
#[derive(Debug, Clone, Default)]
pub struct Devices {
    pub screens: Vec<VideoDevice>,
    pub cameras: Vec<VideoDevice>,
    pub mics: Vec<String>,
}

/// What to record, chosen in the Record tab.
pub struct RecordPanel {
    pub screen: Option<usize>,
    pub camera: Option<usize>,
    pub camera_size: CameraSize,
    /// Show the camera flipped, like a mirror.
    pub mirror: bool,
    pub mic: Mic,
    pub system_sound: bool,
    pub cursor: bool,
    devices: Option<Devices>,
    /// The chosen microphone, open for the level meter.
    preview: Option<(Mic, AudioInput)>,
    preview_error: Option<String>,
    /// The clock sound inputs and recordings are stamped with.
    pub time: Arc<dyn TimeSource>,
}

impl std::fmt::Debug for RecordPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordPanel")
            .field("screen", &self.screen)
            .field("camera", &self.camera)
            .field("mic", &self.mic)
            .field("system_sound", &self.system_sound)
            .finish()
    }
}

impl Default for RecordPanel {
    fn default() -> Self {
        RecordPanel {
            screen: Some(0),
            camera: None,
            camera_size: CameraSize::Default,
            mirror: true,
            mic: Mic::Off,
            system_sound: false,
            cursor: true,
            devices: None,
            preview: None,
            preview_error: None,
            time: Arc::new(rw_capture::clock::SystemTime::new()),
        }
    }
}

impl RecordPanel {
    /// Uses `devices` instead of looking (tests).
    pub fn set_devices(&mut self, devices: Devices) {
        self.devices = Some(devices);
    }

    fn devices(&mut self) -> &Devices {
        self.devices.get_or_insert_with(|| Devices {
            screens: devices::screens(FPS, true),
            cameras: devices::cameras(FPS),
            mics: sources::microphones(),
        })
    }

    /// Re-reads the device lists (a camera plugged in).
    pub fn refresh(&mut self) {
        self.devices = None;
    }

    /// What's chosen, ready to open.
    pub fn choice(&mut self) -> Choice {
        let cursor = self.cursor;
        let size = self.camera_size.video_size();
        let d = self.devices().clone();
        let with_size = |dev: &VideoDevice| {
            let mut spec = dev.spec.clone();
            if let Some(size) = size {
                spec.options.retain(|(k, _)| k != "video_size");
                spec.options.push(("video_size".into(), size.into()));
            }
            spec
        };
        let with_cursor = |dev: &VideoDevice| {
            let mut spec = dev.spec.clone();
            for (k, v) in &mut spec.options {
                if k == "draw_mouse" || k == "capture_cursor" {
                    *v = if cursor { "1".into() } else { "0".into() };
                }
            }
            spec
        };
        Choice {
            screen: self.screen.and_then(|i| d.screens.get(i)).map(with_cursor),
            camera: self.camera.and_then(|i| d.cameras.get(i)).map(with_size),
            mirror: self.mirror,
            mic: match &self.mic {
                Mic::Off => None,
                Mic::Default => Some(AudioDevice::Input(None)),
                Mic::Named(n) => Some(AudioDevice::Input(Some(n.clone()))),
            },
            system_sound: self.system_sound && sources::can_record_system_sound(),
        }
    }

    /// Lets go of the microphone (before recording opens it again).
    pub fn close_preview(&mut self) {
        self.preview = None;
    }
}

/// The devices to open for a recording.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub screen: Option<rw_media::DeviceSpec>,
    pub camera: Option<rw_media::DeviceSpec>,
    pub mic: Option<AudioDevice>,
    pub system_sound: bool,
    /// Flip the camera picture on the timeline.
    pub mirror: bool,
}

impl Choice {
    pub fn is_empty(&self) -> bool {
        self.screen.is_none() && self.camera.is_none() && self.mic.is_none() && !self.system_sound
    }
}

fn meter(ui: &mut egui::Ui, level: f32, tokens: Tokens) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(180.0, 8.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(3), tokens.surface_raised);
    let mut fill = rect;
    fill.set_width(rect.width() * level.clamp(0.0, 1.0));
    let colour = if level > 0.9 {
        tokens.danger
    } else {
        tokens.accent
    };
    ui.painter()
        .rect_filled(fill, egui::CornerRadius::same(3), colour);
}

/// The Record tab. Returns true when "Start recording" is pressed.
pub fn panel(ui: &mut egui::Ui, p: &mut RecordPanel, tokens: Tokens) -> bool {
    let mut start = false;
    let d = p.devices().clone();
    egui::Grid::new("record-setup")
        .num_columns(2)
        .spacing([10.0, 8.0])
        .show(ui, |ui| {
            ui.label("Screen");
            let screen_name = p
                .screen
                .and_then(|i| d.screens.get(i))
                .map_or("Don't record", |s| s.label.as_str())
                .to_owned();
            egui::ComboBox::from_id_salt("record-screen")
                .selected_text(screen_name)
                .width(220.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut p.screen, None, "Don't record");
                    for (i, s) in d.screens.iter().enumerate() {
                        ui.selectable_value(&mut p.screen, Some(i), &s.label);
                    }
                });
            ui.end_row();

            ui.label("Camera");
            let camera_name = p
                .camera
                .and_then(|i| d.cameras.get(i))
                .map_or("None", |s| s.label.as_str())
                .to_owned();
            egui::ComboBox::from_id_salt("record-camera")
                .selected_text(camera_name)
                .width(220.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut p.camera, None, "None");
                    for (i, c) in d.cameras.iter().enumerate() {
                        ui.selectable_value(&mut p.camera, Some(i), &c.label);
                    }
                });
            ui.end_row();
            if p.camera.is_some() {
                ui.label("Camera size");
                egui::ComboBox::from_id_salt("record-camera-size")
                    .selected_text(p.camera_size.label())
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for size in CameraSize::ALL {
                            ui.selectable_value(&mut p.camera_size, size, size.label());
                        }
                    });
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut p.mirror, "Mirror the camera");
                ui.end_row();
            }

            ui.label("Microphone");
            let mic_name = match &p.mic {
                Mic::Off => "None".to_owned(),
                Mic::Default => "Default".to_owned(),
                Mic::Named(n) => n.clone(),
            };
            egui::ComboBox::from_id_salt("record-mic")
                .selected_text(mic_name)
                .width(220.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut p.mic, Mic::Off, "None");
                    ui.selectable_value(&mut p.mic, Mic::Default, "Default");
                    for m in &d.mics {
                        ui.selectable_value(&mut p.mic, Mic::Named(m.clone()), m);
                    }
                });
            ui.end_row();
            if p.mic != Mic::Off {
                ui.label("");
                // Open the chosen microphone for the meter (only once it's
                // chosen, so no permission prompt appears unasked).
                if p.preview.as_ref().is_none_or(|(m, _)| *m != p.mic) && p.preview_error.is_none()
                {
                    let device = match &p.mic {
                        Mic::Named(n) => AudioDevice::Input(Some(n.clone())),
                        _ => AudioDevice::Input(None),
                    };
                    match sources::open_audio(&device, p.time.clone()) {
                        Ok(input) => p.preview = Some((p.mic.clone(), input)),
                        Err(e) => {
                            p.preview = None;
                            p.preview_error = Some(devices::explain_open_error("microphone", &e));
                        }
                    }
                }
                match (&p.preview, &p.preview_error) {
                    (Some((_, input)), _) => {
                        // Drain so the meter reflects now.
                        for _ in input.chunks.try_iter() {}
                        meter(ui, input.level(), tokens);
                        ui.ctx().request_repaint_after(Duration::from_millis(50));
                    }
                    (None, Some(e)) => {
                        ui.add(
                            egui::Label::new(RichText::new(e).small().color(tokens.danger)).wrap(),
                        );
                    }
                    _ => {}
                }
                ui.end_row();
            } else {
                p.preview = None;
                p.preview_error = None;
            }

            if sources::can_record_system_sound() {
                ui.label("Computer sound");
                ui.checkbox(&mut p.system_sound, "Record what's playing");
                ui.end_row();
            }
            if p.screen.is_some() {
                ui.label("Pointer");
                ui.checkbox(&mut p.cursor, "Show the mouse pointer");
                ui.end_row();
            }
        });
    ui.horizontal(|ui| {
        if ui.button("Refresh devices").clicked() {
            p.refresh();
            p.preview_error = None;
        }
    });
    ui.add_space(8.0);
    let choice_empty =
        p.screen.is_none() && p.camera.is_none() && p.mic == Mic::Off && !p.system_sound;
    let go = egui::Button::new(RichText::new(START_LABEL).color(tokens.on_accent))
        .fill(tokens.accent)
        .min_size(egui::vec2(180.0, 34.0));
    if ui
        .add_enabled(!choice_empty, go)
        .on_disabled_hover_text("Choose a screen, camera or microphone")
        .clicked()
    {
        start = true;
    }
    ui.label(
        RichText::new(format!(
            "Counts down {COUNTDOWN} seconds first. Recordings are saved with the project and added to the timeline."
        ))
        .small()
        .color(tokens.text_muted),
    );
    start
}

/// A recording between "Start" and "Stop".
pub enum Active {
    Countdown {
        until: Instant,
        choice: Choice,
    },
    Running {
        recorder: rw_capture::Recorder,
        /// For a retake.
        choice: Choice,
    },
}

/// Opens the chosen devices, stamping sound with `time`. Errors say what
/// to do about them.
pub fn open_sources(
    choice: &Choice,
    time: &Arc<dyn TimeSource>,
) -> Result<rw_capture::Sources, String> {
    use rw_capture::sources::VideoSource;
    let video = |spec: &Option<rw_media::DeviceSpec>, what: &str| {
        spec.as_ref()
            .map(|s| {
                rw_media::DeviceReader::open(s)
                    .map(|r| Box::new(r) as Box<dyn VideoSource>)
                    .map_err(|e| devices::explain_open_error(what, &e.to_string()))
            })
            .transpose()
    };
    let screen = video(&choice.screen, "screen recording")?;
    let asked_size = choice
        .camera
        .as_ref()
        .is_some_and(|c| c.options.iter().any(|(k, _)| k == "video_size"));
    let camera = match video(&choice.camera, "camera") {
        // The camera may not do the size asked for: take its own.
        Err(_) if asked_size => {
            let mut spec = choice.camera.clone();
            if let Some(s) = &mut spec {
                s.options.retain(|(k, _)| k != "video_size");
            }
            video(&spec, "camera")?
        }
        other => other?,
    };
    let mut audio = Vec::new();
    if let Some(mic) = &choice.mic {
        audio.push(
            sources::open_audio(mic, time.clone())
                .map_err(|e| devices::explain_open_error("microphone", &e))?,
        );
    }
    if choice.system_sound {
        audio.push(
            sources::open_audio(&AudioDevice::System, time.clone())
                .map_err(|e| devices::explain_open_error("computer sound", &e))?,
        );
    }
    Ok(rw_capture::Sources {
        screen,
        camera,
        audio,
    })
}

/// A file name for the next recording in `folder`: "Recording N".
pub fn next_name(folder: &std::path::Path) -> String {
    let taken = |name: &str| {
        std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{name} - "))
            })
    };
    (1..)
        .map(|n| format!("Recording {n}"))
        .find(|n| !taken(n))
        .expect("a free name")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAction {
    Pause,
    Resume,
    Stop,
    Retake,
    Cancel,
}

fn clock_text(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// The recording window.
pub fn overlay(ctx: &egui::Context, active: &Active, tokens: Tokens) -> Option<OverlayAction> {
    let mut out = None;
    egui::Modal::new(egui::Id::new("recording")).show(ctx, |ui| {
        ui.set_width(320.0);
        match active {
            Active::Countdown { until, .. } => {
                let left = until.saturating_duration_since(Instant::now());
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Recording starts in").color(tokens.text_muted));
                    ui.label(
                        RichText::new(format!("{}", left.as_secs() + 1))
                            .size(64.0)
                            .strong(),
                    );
                    if ui.button("Cancel").clicked() {
                        out = Some(OverlayAction::Cancel);
                    }
                });
                ctx.request_repaint_after(Duration::from_millis(50));
            }
            Active::Running { recorder: rec, .. } => {
                ui.horizontal(|ui| {
                    let colour = if rec.is_paused() {
                        tokens.text_muted
                    } else {
                        tokens.danger
                    };
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 6.0, colour);
                    crate::layout::steady_text(
                        ui,
                        &clock_text(rec.elapsed()),
                        "00:00:00",
                        egui::FontId::proportional(22.0),
                        tokens.text,
                    );
                    ui.label(
                        RichText::new(if rec.is_paused() {
                            "Paused"
                        } else {
                            "Recording"
                        })
                        .color(tokens.text_muted),
                    );
                });
                for level in rec.levels() {
                    meter(ui, level, tokens);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let (label, action) = if rec.is_paused() {
                        (RESUME_LABEL, OverlayAction::Resume)
                    } else {
                        (PAUSE_LABEL, OverlayAction::Pause)
                    };
                    // One width for all, so "Pause"/"Resume" doesn't
                    // move the others.
                    let button = |text: &str| {
                        egui::Button::new(text).min_size(egui::vec2(BUTTON_WIDTH, 0.0))
                    };
                    if ui.add(button(label)).clicked() {
                        out = Some(action);
                    }
                    if ui.add(button(STOP_LABEL)).clicked() {
                        out = Some(OverlayAction::Stop);
                    }
                    if ui
                        .add(button(RETAKE_LABEL))
                        .on_hover_text("Throw this take away and start again")
                        .clicked()
                    {
                        out = Some(OverlayAction::Retake);
                    }
                });
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
    });
    out
}

/// Adds a finished recording's files to the project and the timeline as
/// one undo step: the screen on the main track at `at`, the camera above
/// it in the bottom-right corner starting with it (the two grouped), and
/// a sound-only recording on a free sound track. A mirrored camera is
/// flipped left to right.
#[derive(Debug)]
pub struct PlaceRecording {
    media: Vec<(rw_model::MediaRef, rw_capture::FileKind)>,
    at: rw_model::Time,
    mirror: bool,
    /// The commands run, built on the first apply and re-run on redo (so
    /// new tracks keep their ids).
    done: Option<Vec<Box<dyn rw_edit::Command>>>,
}

impl PlaceRecording {
    pub fn new(
        media: Vec<(rw_model::MediaRef, rw_capture::FileKind)>,
        at: rw_model::Time,
        mirror: bool,
    ) -> Self {
        PlaceRecording {
            media,
            at,
            mirror,
            done: None,
        }
    }

    fn build(
        &self,
        project: &mut rw_model::Project,
    ) -> Result<Vec<Box<dyn rw_edit::Command>>, rw_edit::EditError> {
        use rw_capture::FileKind;
        use rw_edit::timeline::{Placement, TimelineEdit, TimelineOp};
        use rw_model::{Item, TrackKind};
        let mut done: Vec<Box<dyn rw_edit::Command>> = Vec::new();
        let run = |project: &mut rw_model::Project,
                   mut c: Box<dyn rw_edit::Command>,
                   done: &mut Vec<Box<dyn rw_edit::Command>>|
         -> Result<(), rw_edit::EditError> {
            match c.apply(project) {
                Ok(()) => {
                    done.push(c);
                    Ok(())
                }
                Err(e) => {
                    for mut c in done.drain(..).rev() {
                        c.revert(project);
                    }
                    Err(e)
                }
            }
        };
        for (m, _) in &self.media {
            run(
                project,
                Box::new(rw_edit::commands::AddMedia::new(m.clone())),
                &mut done,
            )?;
        }
        let group = rw_model::GroupId::default();
        let picked = |kind| self.media.iter().find(|(_, k)| *k == kind).map(|(m, _)| m);
        let screen = picked(FileKind::Screen);
        let camera = picked(FileKind::Camera);
        let mut start = self.at;
        // The main picture: the screen, or the camera when there's no screen.
        if let Some(main) = screen.or(camera) {
            let mut item = Item::new(main, rw_model::Time::ZERO);
            if screen.is_some() && camera.is_some() {
                item.group = Some(group);
            }
            if screen.is_none() {
                item.look.transform.flip_h = self.mirror;
            }
            let id = item.id;
            let placement = match project.main_track() {
                Some(t) => Placement::Track(t.id),
                None => Placement::NewTrack {
                    kind: TrackKind::Video,
                    index: 0,
                },
            };
            let op = TimelineOp::Insert {
                placement,
                at: self.at,
                items: vec![item],
            };
            run(project, Box::new(TimelineEdit::new(op)), &mut done)?;
            // The main track may have moved it (no gaps); follow it.
            if let Some((_, i)) = project.find_item(id) {
                start = i.start;
            }
        }
        if let (Some(_), Some(cam)) = (screen, camera) {
            let mut item = Item::new(cam, rw_model::Time::ZERO);
            item.group = Some(group);
            item.look.transform = rw_model::visual::Layout::BottomRight.apply(item.look.transform);
            item.look.transform.flip_h = self.mirror;
            let top = project
                .tracks
                .iter()
                .rposition(|t| t.kind != TrackKind::Audio);
            let op = TimelineOp::Insert {
                placement: Placement::NewTrack {
                    kind: TrackKind::Video,
                    index: top.map_or(0, |i| i + 1),
                },
                at: start,
                items: vec![item],
            };
            run(project, Box::new(TimelineEdit::new(op)), &mut done)?;
        }
        if screen.is_none()
            && camera.is_none()
            && let Some(sound) = picked(FileKind::Sound)
        {
            let item = Item::new(sound, rw_model::Time::ZERO);
            let end = self.at + rw_model::default_duration(sound);
            let placement = project
                .tracks
                .iter()
                .find(|t| {
                    t.kind == TrackKind::Audio
                        && !t.locked
                        && !t.items.iter().any(|i| i.start < end && i.end() > self.at)
                })
                .map_or(
                    Placement::NewTrack {
                        kind: TrackKind::Audio,
                        index: project.tracks.len(),
                    },
                    |t| Placement::Track(t.id),
                );
            let op = TimelineOp::Insert {
                placement,
                at: self.at,
                items: vec![item],
            };
            run(project, Box::new(TimelineEdit::new(op)), &mut done)?;
        }
        Ok(done)
    }
}

impl rw_edit::Command for PlaceRecording {
    fn label(&self) -> String {
        "Add recording".into()
    }

    fn apply(&mut self, project: &mut rw_model::Project) -> Result<(), rw_edit::EditError> {
        match &mut self.done {
            None => {
                self.done = Some(self.build(project)?);
                Ok(())
            }
            Some(done) => {
                for (n, c) in done.iter_mut().enumerate() {
                    if let Err(e) = c.apply(project) {
                        for c in done[..n].iter_mut().rev() {
                            c.revert(project);
                        }
                        return Err(e);
                    }
                }
                Ok(())
            }
        }
    }

    fn revert(&mut self, project: &mut rw_model::Project) {
        if let Some(done) = &mut self.done {
            for c in done.iter_mut().rev() {
                c.revert(project);
            }
        }
    }
}
