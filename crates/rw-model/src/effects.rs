//! Effects on a clip and transitions between clips.
//!
//! The model stores only what was chosen; names, parameter ranges and
//! defaults live in `rw-fx`, which owns the shaders.

use crate::ids::ItemId;
use crate::time::Time;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A visual effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    Blur,
    /// Blurred, enlarged copy of the clip behind it (for vertical video in a
    /// wide frame).
    BackgroundBlurFill,
    Glitch,
    RgbSplit,
    Vhs,
    FilmGrain,
    Vignette,
    Pixelate,
    /// Ken Burns: zoom and pan over the clip's length.
    SlowZoom,
    Shake,
    Haze,
    Sharpen,
    BlackAndWhite,
    Sepia,
    Invert,
    ChromaKey,
}

impl EffectKind {
    pub const ALL: [EffectKind; 16] = [
        EffectKind::Blur,
        EffectKind::BackgroundBlurFill,
        EffectKind::Glitch,
        EffectKind::RgbSplit,
        EffectKind::Vhs,
        EffectKind::FilmGrain,
        EffectKind::Vignette,
        EffectKind::Pixelate,
        EffectKind::SlowZoom,
        EffectKind::Shake,
        EffectKind::Haze,
        EffectKind::Sharpen,
        EffectKind::BlackAndWhite,
        EffectKind::Sepia,
        EffectKind::Invert,
        EffectKind::ChromaKey,
    ];
}

/// One effect applied to a clip. `params` are interpreted per kind (see
/// `rw_fx::effects`); missing values take their defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub kind: EffectKind,
    /// `0.0..=1.0`.
    pub intensity: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<f32>,
}

/// How one clip turns into the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    CrossDissolve,
    FadeBlack,
    FadeWhite,
    WipeLeft,
    WipeRight,
    WipeUp,
    WipeDown,
    SlideLeft,
    SlideRight,
    SlideUp,
    SlideDown,
    Push,
    ZoomIn,
    ZoomOut,
    Spin,
    CircleReveal,
    Glitch,
    LightLeak,
    BlurDissolve,
    PageTurn,
}

impl TransitionKind {
    pub const ALL: [TransitionKind; 20] = [
        TransitionKind::CrossDissolve,
        TransitionKind::FadeBlack,
        TransitionKind::FadeWhite,
        TransitionKind::WipeLeft,
        TransitionKind::WipeRight,
        TransitionKind::WipeUp,
        TransitionKind::WipeDown,
        TransitionKind::SlideLeft,
        TransitionKind::SlideRight,
        TransitionKind::SlideUp,
        TransitionKind::SlideDown,
        TransitionKind::Push,
        TransitionKind::ZoomIn,
        TransitionKind::ZoomOut,
        TransitionKind::Spin,
        TransitionKind::CircleReveal,
        TransitionKind::Glitch,
        TransitionKind::LightLeak,
        TransitionKind::BlurDissolve,
        TransitionKind::PageTurn,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TransitionKind::CrossDissolve => "Cross dissolve",
            TransitionKind::FadeBlack => "Fade through black",
            TransitionKind::FadeWhite => "Fade through white",
            TransitionKind::WipeLeft => "Wipe left",
            TransitionKind::WipeRight => "Wipe right",
            TransitionKind::WipeUp => "Wipe up",
            TransitionKind::WipeDown => "Wipe down",
            TransitionKind::SlideLeft => "Slide left",
            TransitionKind::SlideRight => "Slide right",
            TransitionKind::SlideUp => "Slide up",
            TransitionKind::SlideDown => "Slide down",
            TransitionKind::Push => "Push",
            TransitionKind::ZoomIn => "Zoom in",
            TransitionKind::ZoomOut => "Zoom out",
            TransitionKind::Spin => "Spin",
            TransitionKind::CircleReveal => "Circle reveal",
            TransitionKind::Glitch => "Glitch",
            TransitionKind::LightLeak => "Light leak",
            TransitionKind::BlurDissolve => "Blur dissolve",
            TransitionKind::PageTurn => "Page turn",
        }
    }

    /// Index used by the transition shader.
    pub fn index(self) -> u32 {
        Self::ALL.iter().position(|k| *k == self).unwrap_or(0) as u32
    }
}

/// Shortest and longest transition.
pub const MIN_TRANSITION: Time = Time::from_millis(100);
pub const MAX_TRANSITION: Time = Time::from_secs(3);
pub const DEFAULT_TRANSITION: Time = Time::from_millis(1000);

/// Identifies a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TransitionId(pub Uuid);

impl TransitionId {
    pub fn new() -> Self {
        TransitionId(Uuid::new_v4())
    }
}

impl Default for TransitionId {
    fn default() -> Self {
        Self::new()
    }
}

/// A transition across the cut between item `a` and the item `b` that
/// starts where `a` ends, on the same track. It is centred on the cut and
/// doesn't change the timeline's length: `a` keeps playing (or holds its
/// last frame) into the second half; `b` starts early (or holds its first
/// frame) in the first half.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub id: TransitionId,
    pub a: ItemId,
    pub b: ItemId,
    pub kind: TransitionKind,
    pub duration: Time,
}

impl Transition {
    /// Timeline window `[start, end)` around the cut at `cut`.
    pub fn window(&self, cut: Time) -> (Time, Time) {
        let half = Time(self.duration.ticks() / 2);
        (cut - half, cut - half + self.duration)
    }

    /// Progress `0..=1` at `t` within the window around `cut`.
    pub fn progress(&self, cut: Time, t: Time) -> f32 {
        let (s, e) = self.window(cut);
        ((t - s).as_secs_f64() / (e - s).as_secs_f64().max(1e-9)).clamp(0.0, 1.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_window_is_centred() {
        let t = Transition {
            id: TransitionId::new(),
            a: ItemId::new(),
            b: ItemId::new(),
            kind: TransitionKind::CrossDissolve,
            duration: Time::from_secs(1),
        };
        let cut = Time::from_secs(5);
        assert_eq!(
            t.window(cut),
            (Time::from_millis(4500), Time::from_millis(5500))
        );
        assert_eq!(t.progress(cut, cut), 0.5);
        assert_eq!(t.progress(cut, Time::ZERO), 0.0);
        assert!(TransitionKind::ALL.len() >= 18 && EffectKind::ALL.len() >= 15);
    }
}
