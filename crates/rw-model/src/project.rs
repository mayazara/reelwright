//! The project document: canvas, media, tracks and markers.

use crate::ids::{GroupId, ItemId, MarkerId, MediaId, ProjectId, TrackId};
use crate::media::MediaRef;
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
    Media { media: MediaId },
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupId>,
}

impl Item {
    pub fn end(&self) -> Time {
        self.start + self.duration
    }

    pub fn media(&self) -> Option<MediaId> {
        match self.kind {
            ItemKind::Media { media } => Some(media),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub kind: TrackKind,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub muted: bool,
    pub items: Vec<Item>,
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
    /// Bottom-most first; the first video track is the main track.
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
