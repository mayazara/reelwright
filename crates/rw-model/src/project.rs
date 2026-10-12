//! The project document: canvas, media, tracks and markers.

use crate::ids::{GroupId, ItemId, MarkerId, MediaId, ProjectId, TrackId};
use crate::media::{MediaKind, MediaRef};
use crate::time::{FrameRate, Time};
use serde::{Deserialize, Serialize};

/// Current schema version written by this build. Bump it together with a new
/// step in [`crate::file::migrate`].
pub const SCHEMA_VERSION: u32 = 1;

/// Straight-alpha sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const BLACK: Rgba = Rgba([0, 0, 0, 255]);
}

/// Aspect-ratio presets offered when creating or resizing a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AspectPreset {
    Widescreen16x9,
    Vertical9x16,
    Square1x1,
    Standard4x3,
    Portrait4x5,
    Portrait2x3,
    Cinema21x9,
}

impl AspectPreset {
    pub const ALL: [AspectPreset; 7] = [
        AspectPreset::Widescreen16x9,
        AspectPreset::Vertical9x16,
        AspectPreset::Square1x1,
        AspectPreset::Standard4x3,
        AspectPreset::Portrait4x5,
        AspectPreset::Portrait2x3,
        AspectPreset::Cinema21x9,
    ];

    /// Canvas size at the default (1080-line) resolution.
    pub fn size(self) -> (u32, u32) {
        match self {
            AspectPreset::Widescreen16x9 => (1920, 1080),
            AspectPreset::Vertical9x16 => (1080, 1920),
            AspectPreset::Square1x1 => (1080, 1080),
            AspectPreset::Standard4x3 => (1440, 1080),
            AspectPreset::Portrait4x5 => (1080, 1350),
            AspectPreset::Portrait2x3 => (1080, 1620),
            AspectPreset::Cinema21x9 => (2520, 1080),
        }
    }

    pub fn ratio_label(self) -> &'static str {
        match self {
            AspectPreset::Widescreen16x9 => "16:9",
            AspectPreset::Vertical9x16 => "9:16",
            AspectPreset::Square1x1 => "1:1",
            AspectPreset::Standard4x3 => "4:3",
            AspectPreset::Portrait4x5 => "4:5",
            AspectPreset::Portrait2x3 => "2:3",
            AspectPreset::Cinema21x9 => "21:9",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            AspectPreset::Widescreen16x9 => "Widescreen",
            AspectPreset::Vertical9x16 => "Vertical",
            AspectPreset::Square1x1 => "Square",
            AspectPreset::Standard4x3 => "Classic",
            AspectPreset::Portrait4x5 => "Portrait",
            AspectPreset::Portrait2x3 => "Tall portrait",
            AspectPreset::Cinema21x9 => "Cinematic",
        }
    }

    /// The preset matching a size exactly, if any.
    pub fn matching(width: u32, height: u32) -> Option<AspectPreset> {
        Self::ALL.into_iter().find(|p| p.size() == (width, height))
    }
}

/// Output frame settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub frame_rate: FrameRate,
    pub background: Rgba,
}

impl Canvas {
    pub const MIN_SIDE: u32 = 16;
    pub const MAX_SIDE: u32 = 8192;

    pub fn from_preset(preset: AspectPreset) -> Self {
        let (width, height) = preset.size();
        Canvas {
            width,
            height,
            frame_rate: FrameRate::FPS_30,
            background: Rgba::BLACK,
        }
    }

    /// Custom size, clamped to the supported range and rounded to even
    /// numbers (required by 4:2:0 encoders).
    pub fn custom(width: u32, height: u32) -> Self {
        let fix = |v: u32| (v.clamp(Self::MIN_SIDE, Self::MAX_SIDE) / 2) * 2;
        Canvas {
            width: fix(width),
            height: fix(height),
            ..Canvas::from_preset(AspectPreset::Widescreen16x9)
        }
    }

    pub fn preset(&self) -> Option<AspectPreset> {
        AspectPreset::matching(self.width, self.height)
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Canvas::from_preset(AspectPreset::Widescreen16x9)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
    Text,
    Overlay,
}

/// What an item shows or plays. More kinds arrive with later milestones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemKind {
    Media {
        media: MediaId,
    },
    /// A solid colour or gradient (backgrounds); no media file.
    Generated {
        fill: crate::visual::Generated,
    },
    /// One frame of a video held still (freeze frame).
    Frame {
        media: MediaId,
        at: Time,
    },
    /// A title, lower third, credits or other text.
    Text {
        text: Box<crate::text::TextSpec>,
    },
}

/// Slowest and fastest playback speed.
pub const MIN_SPEED: f32 = 0.1;
pub const MAX_SPEED: f32 = 16.0;

/// Highest item or track gain, +12 dB.
pub const MAX_GAIN: f32 = 3.981_072;

/// Default length of a still image placed on the timeline.
pub const IMAGE_DURATION: Time = Time::from_secs(5);

fn unity() -> f32 {
    1.0
}

fn is_unity(v: &f32) -> bool {
    *v == 1.0
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// A clip placed on a track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: ItemId,
    pub start: Time,
    pub duration: Time,
    /// Offset into the source where playback begins.
    pub source_in: Time,
    pub kind: ItemKind,
    /// Items sharing a group move, trim and delete together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupId>,
    /// Linear audio gain, `0.0..=MAX_GAIN`.
    #[serde(default = "unity", skip_serializing_if = "is_unity")]
    pub gain: f32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub muted: bool,
    /// Set on a video item whose sound was detached to an audio track, so
    /// the sound is not played twice.
    #[serde(default, skip_serializing_if = "is_false")]
    pub audio_detached: bool,
    /// Placement, crop, blending, colour, effects and fades.
    #[serde(default, skip_serializing_if = "crate::visual::Look::is_default")]
    pub look: crate::visual::Look,
    /// Playback speed, `MIN_SPEED..=MAX_SPEED`. The item covers
    /// `duration × speed` of its source.
    #[serde(default = "unity", skip_serializing_if = "is_unity")]
    pub speed: f32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reverse: bool,
    /// When sped up or slowed down, keep the sound's pitch (default) or let
    /// it follow the speed.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub keep_pitch: bool,
}

fn yes() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

impl Item {
    /// A new item showing `media` from its beginning.
    pub fn new(media: &MediaRef, start: Time) -> Self {
        Item {
            id: ItemId::new(),
            start,
            duration: default_duration(media),
            source_in: Time::ZERO,
            kind: ItemKind::Media { media: media.id },
            group: None,
            gain: 1.0,
            muted: false,
            audio_detached: false,
            look: crate::visual::Look::default(),
            speed: 1.0,
            reverse: false,
            keep_pitch: true,
        }
    }

    /// A new generated clip (colour or gradient) of the default length.
    pub fn generated(fill: crate::visual::Generated, start: Time) -> Self {
        Item {
            id: ItemId::new(),
            start,
            duration: IMAGE_DURATION,
            source_in: Time::ZERO,
            kind: ItemKind::Generated { fill },
            group: None,
            gain: 1.0,
            muted: false,
            audio_detached: false,
            look: crate::visual::Look::default(),
            speed: 1.0,
            reverse: false,
            keep_pitch: true,
        }
    }

    pub fn end(&self) -> Time {
        self.start + self.duration
    }

    /// A held frame of `media` at source time `at`.
    /// A text item at `start`, 5 s long.
    pub fn text_item(spec: crate::text::TextSpec, start: Time) -> Self {
        Item {
            kind: ItemKind::Text {
                text: Box::new(spec),
            },
            ..Item::generated(
                crate::visual::Generated::Solid { color: Rgba::BLACK },
                start,
            )
        }
    }

    pub fn frame(media: MediaId, at: Time, start: Time, duration: Time) -> Self {
        Item {
            kind: ItemKind::Frame { media, at },
            duration,
            ..Item::generated(
                crate::visual::Generated::Solid { color: Rgba::BLACK },
                start,
            )
        }
    }

    /// The media file this item shows or plays (including held frames).
    pub fn media(&self) -> Option<MediaId> {
        match self.kind {
            ItemKind::Media { media } | ItemKind::Frame { media, .. } => Some(media),
            ItemKind::Generated { .. } | ItemKind::Text { .. } => None,
        }
    }

    pub fn generated_fill(&self) -> Option<crate::visual::Generated> {
        match self.kind {
            ItemKind::Generated { fill } => Some(fill),
            _ => None,
        }
    }

    /// Source time of a held frame.
    pub fn held_frame(&self) -> Option<Time> {
        match self.kind {
            ItemKind::Frame { at, .. } => Some(at),
            _ => None,
        }
    }

    /// How much of the source the item covers: `duration × speed`.
    pub fn source_span(&self) -> Time {
        Time((self.duration.ticks() as f64 * f64::from(self.speed)).round() as i64)
    }

    /// Timeline length that covers `span` of source at this speed.
    pub fn duration_for_span(&self, span: Time) -> Time {
        Time((span.ticks() as f64 / f64::from(self.speed.max(MIN_SPEED))).round() as i64)
    }

    /// Whether `t` falls inside the item (start inclusive, end exclusive).
    pub fn contains(&self, t: Time) -> bool {
        self.start <= t && t < self.end()
    }

    /// The text this item shows, for text items.
    pub fn text(&self) -> Option<&crate::text::TextSpec> {
        match &self.kind {
            ItemKind::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Source time shown at timeline time `t` (speed and reverse applied;
    /// a held frame always shows its frame).
    pub fn source_time(&self, t: Time) -> Time {
        if let Some(at) = self.held_frame() {
            return at;
        }
        let offset = Time(((t - self.start).ticks() as f64 * f64::from(self.speed)).round() as i64);
        if self.reverse {
            // Last frame of the span first; stay inside the span.
            let last = self.source_in + self.source_span() - Time(1);
            (last - offset).max(self.source_in)
        } else {
            self.source_in + offset
        }
    }
}

/// How long an item for `media` is when first placed.
pub fn default_duration(media: &MediaRef) -> Time {
    match media.kind() {
        MediaKind::Image => IMAGE_DURATION,
        _ if media.info.duration > Time::ZERO => media.info.duration,
        _ => IMAGE_DURATION,
    }
}

/// Longest an item for `media` can be from `source_in`, or `None` if it
/// can be stretched indefinitely (still images).
pub fn max_duration(media: &MediaRef, source_in: Time) -> Option<Time> {
    match media.kind() {
        MediaKind::Image => None,
        _ => Some((media.info.duration - source_in).max(Time::ZERO)),
    }
}

/// Which track kind holds items for a media kind.
pub fn track_kind_for(kind: MediaKind) -> TrackKind {
    match kind {
        MediaKind::Audio => TrackKind::Audio,
        _ => TrackKind::Video,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub kind: TrackKind,
    /// The main track is magnetic: its items always sit back to back from
    /// time zero. A project has at most one, and it is never removed
    /// automatically.
    #[serde(default, skip_serializing_if = "is_false")]
    pub main: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub muted: bool,
    /// Linear gain applied to the track's mix, `0.0..=MAX_GAIN`.
    #[serde(default = "unity", skip_serializing_if = "is_unity")]
    pub gain: f32,
    pub items: Vec<Item>,
    /// Transitions between adjacent items on this track.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<crate::effects::Transition>,
}

impl Track {
    pub fn new(kind: TrackKind) -> Self {
        Track {
            id: TrackId::new(),
            kind,
            main: false,
            locked: false,
            hidden: false,
            muted: false,
            gain: 1.0,
            items: Vec::new(),
            transitions: Vec::new(),
        }
    }

    pub fn new_main() -> Self {
        Track {
            main: true,
            ..Track::new(TrackKind::Video)
        }
    }

    pub fn end(&self) -> Time {
        self.items.iter().map(Item::end).max().unwrap_or(Time::ZERO)
    }

    /// Item playing at `t`, if any.
    pub fn item_at(&self, t: Time) -> Option<&Item> {
        self.items.iter().find(|i| i.contains(t))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub id: MarkerId,
    pub at: Time,
    #[serde(default)]
    pub label: String,
}

/// A whole project, as saved to `project.rwproj`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub id: ProjectId,
    pub name: String,
    pub canvas: Canvas,
    pub media: Vec<MediaRef>,
    /// Bottom-most first. Video tracks draw over the tracks before them.
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub markers: Vec<Marker>,
}

impl Project {
    pub fn new(name: impl Into<String>, canvas: Canvas) -> Self {
        Project {
            version: SCHEMA_VERSION,
            id: ProjectId::new(),
            name: name.into(),
            canvas,
            media: Vec::new(),
            tracks: Vec::new(),
            markers: Vec::new(),
        }
    }

    pub fn media(&self, id: MediaId) -> Option<&MediaRef> {
        self.media.iter().find(|m| m.id == id)
    }

    pub fn media_index(&self, id: MediaId) -> Option<usize> {
        self.media.iter().position(|m| m.id == id)
    }

    /// Whether any timeline item uses this media.
    pub fn is_media_used(&self, id: MediaId) -> bool {
        self.tracks
            .iter()
            .flat_map(|t| &t.items)
            .any(|i| i.media() == Some(id))
    }

    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn main_track(&self) -> Option<&Track> {
        self.tracks.iter().find(|t| t.main)
    }

    /// Finds an item and the index of its track.
    pub fn find_item(&self, id: ItemId) -> Option<(usize, &Item)> {
        self.tracks
            .iter()
            .enumerate()
            .find_map(|(ti, t)| t.items.iter().find(|i| i.id == id).map(|i| (ti, i)))
    }

    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.tracks.iter().flat_map(|t| &t.items)
    }

    /// Length of the timeline: end of the last item.
    pub fn duration(&self) -> Time {
        self.tracks
            .iter()
            .flat_map(|t| &t.items)
            .map(Item::end)
            .max()
            .unwrap_or(Time::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_their_ratio_and_even_sides() {
        for p in AspectPreset::ALL {
            let (w, h) = p.size();
            assert_eq!(w % 2, 0);
            assert_eq!(h % 2, 0);
            let (a, b) = p
                .ratio_label()
                .split_once(':')
                .map(|(a, b)| (a.parse::<u32>().unwrap(), b.parse::<u32>().unwrap()))
                .unwrap();
            assert_eq!(w * b, h * a, "{p:?}");
            assert_eq!(AspectPreset::matching(w, h), Some(p));
        }
    }

    #[test]
    fn custom_canvas_is_clamped_and_even() {
        let c = Canvas::custom(1001, 3);
        assert_eq!((c.width, c.height), (1000, 16));
        assert_eq!(Canvas::custom(100_000, 720).width, Canvas::MAX_SIDE);
    }
}
