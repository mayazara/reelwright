//! Text items: titles, lower thirds, credits.
//!
//! Sizes are relative so a title looks the same on any canvas: the font
//! size is a share of the canvas height, and outline, shadow and box sizes
//! are shares of the font size. Layout and drawing live in `rw-text`.

use crate::project::Rgba;
use crate::time::Time;
use serde::{Deserialize, Serialize};

/// Bundled family used when a project names a font this machine lacks.
pub const DEFAULT_FONT: &str = "Montserrat";

/// Smallest and largest font size, as a share of the canvas height.
pub const MIN_TEXT_SIZE: f32 = 0.01;
pub const MAX_TEXT_SIZE: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

impl TextAlign {
    pub const ALL: [TextAlign; 3] = [TextAlign::Left, TextAlign::Center, TextAlign::Right];

    pub fn label(self) -> &'static str {
        match self {
            TextAlign::Left => "Left",
            TextAlign::Center => "Centre",
            TextAlign::Right => "Right",
        }
    }
}

/// A line drawn around each letter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Outline {
    /// Share of the font size.
    pub width: f32,
    pub color: Rgba,
}

/// A soft copy of the text behind it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    /// Offset across and down, as shares of the font size.
    pub offset: [f32; 2],
    /// Blur radius, share of the font size.
    pub blur: f32,
    pub color: Rgba,
}

/// A filled, optionally rounded box behind the text.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextBox {
    pub color: Rgba,
    /// Space between text and box edge, share of the font size.
    pub padding: f32,
    /// Corner radius, share of the font size.
    pub radius: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    /// Font family name.
    pub font: String,
    /// Share of the canvas height.
    pub size: f32,
    /// 100 (thin) to 900 (black).
    pub weight: u16,
    #[serde(default)]
    pub italic: bool,
    pub color: Rgba,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<Outline>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<Shadow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<TextBox>,
    #[serde(default)]
    pub align: TextAlign,
    /// Extra space between letters, share of the font size.
    #[serde(default)]
    pub letter_spacing: f32,
    /// Line height as a multiple of the font size.
    #[serde(default = "default_line_spacing")]
    pub line_spacing: f32,
}

fn default_line_spacing() -> f32 {
    1.2
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            font: DEFAULT_FONT.into(),
            size: 0.08,
            weight: 700,
            italic: false,
            color: Rgba([255, 255, 255, 255]),
            outline: None,
            shadow: Some(Shadow {
                offset: [0.04, 0.06],
                blur: 0.08,
                color: Rgba([0, 0, 0, 140]),
            }),
            background: None,
            align: TextAlign::Center,
            letter_spacing: 0.0,
            line_spacing: default_line_spacing(),
        }
    }
}

impl TextStyle {
    /// Values clamped to their ranges; NaN replaced by defaults.
    pub fn sanitized(mut self) -> Self {
        let fix = |v: f32, lo: f32, hi: f32, d: f32| {
            if v.is_finite() { v.clamp(lo, hi) } else { d }
        };
        self.size = fix(self.size, MIN_TEXT_SIZE, MAX_TEXT_SIZE, 0.08);
        self.weight = self.weight.clamp(100, 900);
        self.letter_spacing = fix(self.letter_spacing, -0.2, 1.0, 0.0);
        self.line_spacing = fix(self.line_spacing, 0.6, 3.0, 1.2);
        if let Some(o) = &mut self.outline {
            o.width = fix(o.width, 0.0, 0.3, 0.05);
        }
        if let Some(s) = &mut self.shadow {
            s.offset = s.offset.map(|v| fix(v, -1.0, 1.0, 0.0));
            s.blur = fix(s.blur, 0.0, 1.0, 0.0);
        }
        if let Some(b) = &mut self.background {
            b.padding = fix(b.padding, 0.0, 2.0, 0.3);
            b.radius = fix(b.radius, 0.0, 2.0, 0.0);
        }
        if self.font.trim().is_empty() {
            self.font = DEFAULT_FONT.into();
        }
        self
    }
}

/// How text comes on or goes off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextMotion {
    #[default]
    None,
    Fade,
    Pop,
    SlideLeft,
    SlideRight,
    SlideUp,
    SlideDown,
    Typewriter,
    WordByWord,
    Bounce,
    Zoom,
}

impl TextMotion {
    pub const ALL: [TextMotion; 11] = [
        TextMotion::None,
        TextMotion::Fade,
        TextMotion::Pop,
        TextMotion::SlideLeft,
        TextMotion::SlideRight,
        TextMotion::SlideUp,
        TextMotion::SlideDown,
        TextMotion::Typewriter,
        TextMotion::WordByWord,
        TextMotion::Bounce,
        TextMotion::Zoom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TextMotion::None => "None",
            TextMotion::Fade => "Fade",
            TextMotion::Pop => "Pop",
            TextMotion::SlideLeft => "Slide from right",
            TextMotion::SlideRight => "Slide from left",
            TextMotion::SlideUp => "Slide up",
            TextMotion::SlideDown => "Slide down",
            TextMotion::Typewriter => "Typewriter",
            TextMotion::WordByWord => "Word by word",
            TextMotion::Bounce => "Bounce",
            TextMotion::Zoom => "Zoom",
        }
    }
}

/// An entrance or exit and how long it takes.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct TextAnimation {
    pub motion: TextMotion,
    pub duration: Time,
}

impl TextAnimation {
    pub const DEFAULT_DURATION: Time = Time::from_millis(600);

    pub fn new(motion: TextMotion) -> Self {
        TextAnimation {
            motion,
            duration: Self::DEFAULT_DURATION,
        }
    }
}

/// What the text shows over time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextBehaviour {
    /// The same text throughout.
    #[default]
    Static,
    /// The text rolls up through the frame over the item's length (end
    /// credits).
    Roll,
    /// Whole seconds left until the item ends, replacing the text.
    Countdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSpec {
    pub text: String,
    pub style: TextStyle,
    #[serde(default)]
    pub enter: TextAnimation,
    #[serde(default)]
    pub exit: TextAnimation,
    #[serde(default)]
    pub behaviour: TextBehaviour,
}

impl TextSpec {
    pub fn new(text: impl Into<String>) -> Self {
        TextSpec {
            text: text.into(),
            style: TextStyle::default(),
            enter: TextAnimation::new(TextMotion::Fade),
            exit: TextAnimation::new(TextMotion::Fade),
            behaviour: TextBehaviour::Static,
        }
    }

    /// The text to show `local` into an item of `duration`.
    pub fn text_at(&self, local: Time, duration: Time) -> String {
        match self.behaviour {
            TextBehaviour::Countdown => {
                let left = (duration - local).max(Time::ZERO).as_secs_f64();
                format!("{}", left.ceil() as i64)
            }
            _ => self.text.clone(),
        }
    }

    pub fn sanitized(mut self, duration: Time) -> Self {
        self.style = self.style.sanitized();
        // Entrance and exit fit the item together.
        let half = Time(duration.ticks() / 2).max(Time::ZERO);
        self.enter.duration = self.enter.duration.clamp(Time::ZERO, half);
        self.exit.duration = self.exit.duration.clamp(Time::ZERO, half);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_shows_whole_seconds_left() {
        let mut t = TextSpec::new("ignored");
        t.behaviour = TextBehaviour::Countdown;
        let d = Time::from_secs(5);
        assert_eq!(t.text_at(Time::ZERO, d), "5");
        assert_eq!(t.text_at(Time::from_millis(4100), d), "1");
        assert_eq!(t.text_at(d, d), "0");
        t.behaviour = TextBehaviour::Static;
        assert_eq!(t.text_at(Time::ZERO, d), "ignored");
    }

    #[test]
    fn sanitizing_clamps_and_fits_animations() {
        let mut t = TextSpec::new("x");
        t.style.size = f32::NAN;
        t.style.weight = 5000;
        t.enter.duration = Time::from_secs(9);
        let t = t.sanitized(Time::from_secs(2));
        assert_eq!(t.style.size, 0.08);
        assert_eq!(t.style.weight, 900);
        assert_eq!(t.enter.duration, Time::from_secs(1));
    }

    #[test]
    fn json_round_trip() {
        let t = TextSpec::new("Hello مرحبا नमस्ते 你好 🎬");
        let s = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<TextSpec>(&s).unwrap(), t);
    }
}
