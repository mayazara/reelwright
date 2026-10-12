//! How a clip looks: placement, crop, blending, colour and fades.
//!
//! Every field defaults to "no change", so projects saved before these
//! existed load unchanged and files stay small.

use crate::project::Rgba;
use crate::time::Time;
use serde::{Deserialize, Serialize};

/// How the (cropped) picture fills its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Whole picture visible inside the box (letterboxed).
    #[default]
    Contain,
    /// Box filled; the overflow is cut off at the box edge.
    Cover,
    /// Picture stretched to the box's shape.
    Stretch,
}

impl Fit {
    pub const ALL: [Fit; 3] = [Fit::Contain, Fit::Cover, Fit::Stretch];

    pub fn label(self) -> &'static str {
        match self {
            Fit::Contain => "Fit",
            Fit::Cover => "Fill",
            Fit::Stretch => "Stretch",
        }
    }
}

/// Placement on the canvas. The box is in canvas fractions: `center`
/// `(0.5, 0.5)` and `size` `(1, 1)` cover the whole canvas.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform {
    pub center: [f32; 2],
    pub size: [f32; 2],
    /// Clockwise, degrees.
    pub rotation: f32,
    pub flip_h: bool,
    pub flip_v: bool,
    pub fit: Fit,
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            center: [0.5, 0.5],
            size: [1.0, 1.0],
            rotation: 0.0,
            flip_h: false,
            flip_v: false,
            fit: Fit::Contain,
        }
    }
}

impl Transform {
    pub fn is_identity(&self) -> bool {
        *self == Transform::default()
    }

    /// Uniform scale relative to full canvas (the larger box side).
    pub fn scale(&self) -> f32 {
        self.size[0].max(self.size[1])
    }

    /// Keeps values in sensible ranges: boxes from 1 % to 1000 % of the
    /// canvas, rotation in `[0, 360)`.
    pub fn sanitized(mut self) -> Self {
        for s in &mut self.size {
            *s = if s.is_finite() {
                s.clamp(0.01, 10.0)
            } else {
                1.0
            };
        }
        for c in &mut self.center {
            *c = if c.is_finite() {
                c.clamp(-5.0, 6.0)
            } else {
                0.5
            };
        }
        self.rotation = if self.rotation.is_finite() {
            self.rotation.rem_euclid(360.0)
        } else {
            0.0
        };
        self
    }
}

/// Picture-in-picture and split-screen layouts offered by the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layout {
    Full,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    SplitLeft,
    SplitRight,
    SplitTop,
    SplitBottom,
}

impl Layout {
    pub const ALL: [Layout; 9] = [
        Layout::Full,
        Layout::TopLeft,
        Layout::TopRight,
        Layout::BottomLeft,
        Layout::BottomRight,
        Layout::SplitLeft,
        Layout::SplitRight,
        Layout::SplitTop,
        Layout::SplitBottom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Layout::Full => "Full frame",
            Layout::TopLeft => "Corner: top left",
            Layout::TopRight => "Corner: top right",
            Layout::BottomLeft => "Corner: bottom left",
            Layout::BottomRight => "Corner: bottom right",
            Layout::SplitLeft => "Split: left half",
            Layout::SplitRight => "Split: right half",
            Layout::SplitTop => "Split: top half",
            Layout::SplitBottom => "Split: bottom half",
        }
    }

    /// The transform for this layout, keeping rotation and flips.
    pub fn apply(self, t: Transform) -> Transform {
        const PIP: f32 = 0.32;
        const MARGIN: f32 = 0.04;
        let corner = |x: f32, y: f32| ([x, y], [PIP, PIP], Fit::Contain);
        let near = MARGIN + PIP / 2.0;
        let far = 1.0 - near;
        let (center, size, fit) = match self {
            Layout::Full => ([0.5, 0.5], [1.0, 1.0], Fit::Contain),
            Layout::TopLeft => corner(near, near),
            Layout::TopRight => corner(far, near),
            Layout::BottomLeft => corner(near, far),
            Layout::BottomRight => corner(far, far),
            Layout::SplitLeft => ([0.25, 0.5], [0.5, 1.0], Fit::Cover),
            Layout::SplitRight => ([0.75, 0.5], [0.5, 1.0], Fit::Cover),
            Layout::SplitTop => ([0.5, 0.25], [1.0, 0.5], Fit::Cover),
            Layout::SplitBottom => ([0.5, 0.75], [1.0, 0.5], Fit::Cover),
        };
        Transform {
            center,
            size,
            fit,
            ..t
        }
    }
}

/// Fractions of the source cut off each side.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Crop {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Crop {
    /// Smallest share of the source that must stay visible on each axis.
    pub const MIN_KEEP: f32 = 0.02;

    pub fn is_none(&self) -> bool {
        *self == Crop::default()
    }

    pub fn sanitized(mut self) -> Self {
        let fix = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0 - Self::MIN_KEEP)
            } else {
                0.0
            }
        };
        self.left = fix(self.left);
        self.right = fix(self.right);
        self.top = fix(self.top);
        self.bottom = fix(self.bottom);
        let excess_x = self.left + self.right - (1.0 - Self::MIN_KEEP);
        if excess_x > 0.0 {
            self.right -= excess_x;
        }
        let excess_y = self.top + self.bottom - (1.0 - Self::MIN_KEEP);
        if excess_y > 0.0 {
            self.bottom -= excess_y;
        }
        self
    }

    /// Width and height fractions left after cropping.
    pub fn kept(&self) -> (f32, f32) {
        (1.0 - self.left - self.right, 1.0 - self.top - self.bottom)
    }

    /// Largest centred crop of a `src` picture with aspect `ratio`
    /// (width / height).
    pub fn to_ratio(src: (u32, u32), ratio: f32) -> Crop {
        let src_ratio = src.0.max(1) as f32 / src.1.max(1) as f32;
        if ratio > src_ratio {
            let keep = src_ratio / ratio;
            let cut = (1.0 - keep) / 2.0;
            Crop {
                top: cut,
                bottom: cut,
                ..Crop::default()
            }
        } else {
            let keep = ratio / src_ratio;
            let cut = (1.0 - keep) / 2.0;
            Crop {
                left: cut,
                right: cut,
                ..Crop::default()
            }
        }
    }
}

/// How a clip combines with what's beneath it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    Add,
    Difference,
}

impl BlendMode {
    pub const ALL: [BlendMode; 10] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::Add,
        BlendMode::Difference,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
            BlendMode::Darken => "Darken",
            BlendMode::Lighten => "Lighten",
            BlendMode::ColorDodge => "Colour dodge",
            BlendMode::ColorBurn => "Colour burn",
            BlendMode::Add => "Add",
            BlendMode::Difference => "Difference",
        }
    }

    /// Index used by the compositor shader.
    pub fn index(self) -> u32 {
        Self::ALL.iter().position(|b| *b == self).unwrap_or(0) as u32
    }
}

/// Colour corrections. Each is `0.0` for "no change" and spans `-1..=1`,
/// except exposure, which is in stops (`-3..=3`).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorAdjust {
    pub exposure: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub vibrance: f32,
    pub temperature: f32,
    pub tint: f32,
    pub highlights: f32,
    pub shadows: f32,
}

/// One adjustable control, for building sliders and tests generically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorControl {
    Exposure,
    Brightness,
    Contrast,
    Saturation,
    Vibrance,
    Temperature,
    Tint,
    Highlights,
    Shadows,
}

impl ColorControl {
    pub const ALL: [ColorControl; 9] = [
        ColorControl::Exposure,
        ColorControl::Brightness,
        ColorControl::Contrast,
        ColorControl::Saturation,
        ColorControl::Vibrance,
        ColorControl::Temperature,
        ColorControl::Tint,
        ColorControl::Highlights,
        ColorControl::Shadows,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ColorControl::Exposure => "Exposure",
            ColorControl::Brightness => "Brightness",
            ColorControl::Contrast => "Contrast",
            ColorControl::Saturation => "Saturation",
            ColorControl::Vibrance => "Vibrance",
            ColorControl::Temperature => "Temperature",
            ColorControl::Tint => "Tint",
            ColorControl::Highlights => "Highlights",
            ColorControl::Shadows => "Shadows",
        }
    }

    pub fn range(self) -> std::ops::RangeInclusive<f32> {
        match self {
            ColorControl::Exposure => -3.0..=3.0,
            _ => -1.0..=1.0,
        }
    }
}

impl ColorAdjust {
    pub fn is_neutral(&self) -> bool {
        *self == ColorAdjust::default()
    }

    pub fn get(&self, c: ColorControl) -> f32 {
        match c {
            ColorControl::Exposure => self.exposure,
            ColorControl::Brightness => self.brightness,
            ColorControl::Contrast => self.contrast,
            ColorControl::Saturation => self.saturation,
            ColorControl::Vibrance => self.vibrance,
            ColorControl::Temperature => self.temperature,
            ColorControl::Tint => self.tint,
            ColorControl::Highlights => self.highlights,
            ColorControl::Shadows => self.shadows,
        }
    }

    pub fn set(&mut self, c: ColorControl, v: f32) {
        let r = c.range();
        let v = if v.is_finite() {
            v.clamp(*r.start(), *r.end())
        } else {
            0.0
        };
        match c {
            ColorControl::Exposure => self.exposure = v,
            ColorControl::Brightness => self.brightness = v,
            ColorControl::Contrast => self.contrast = v,
            ColorControl::Saturation => self.saturation = v,
            ColorControl::Vibrance => self.vibrance = v,
            ColorControl::Temperature => self.temperature = v,
            ColorControl::Tint => self.tint = v,
            ColorControl::Highlights => self.highlights = v,
            ColorControl::Shadows => self.shadows = v,
        }
    }

    pub fn sanitized(mut self) -> Self {
        for c in ColorControl::ALL {
            let v = self.get(c);
            self.set(c, v);
        }
        self
    }
}

/// A look-up-table filter, by name, mixed in at `intensity`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    /// File stem of a bundled `.cube` (e.g. `"ember"`).
    pub id: String,
    /// `0.0..=1.0`.
    pub intensity: f32,
}

/// Everything about how a clip looks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    pub transform: Transform,
    pub crop: Crop,
    /// `0.0..=1.0`.
    pub opacity: f32,
    pub blend: BlendMode,
    pub color: ColorAdjust,
    pub filter: Option<Filter>,
    /// Applied in order, before colour and filter.
    pub effects: Vec<crate::effects::Effect>,
    /// Fades apply to both picture and sound.
    pub fade_in: Time,
    pub fade_out: Time,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            transform: Transform::default(),
            crop: Crop::default(),
            opacity: 1.0,
            blend: BlendMode::Normal,
            color: ColorAdjust::default(),
            filter: None,
            effects: Vec::new(),
            fade_in: Time::ZERO,
            fade_out: Time::ZERO,
        }
    }
}

impl Look {
    pub fn is_default(&self) -> bool {
        *self == Look::default()
    }

    /// Fade multiplier at `offset` into a clip of `duration`.
    pub fn fade_at(&self, offset: Time, duration: Time) -> f32 {
        let ramp = |pos: Time, len: Time| {
            if len <= Time::ZERO {
                1.0
            } else {
                (pos.as_secs_f64() / len.as_secs_f64()).clamp(0.0, 1.0) as f32
            }
        };
        let fin = ramp(offset, self.fade_in);
        let fout = ramp(duration - offset, self.fade_out);
        fin.min(fout)
    }

    /// Keeps values in range; fades together never exceed `duration`.
    pub fn sanitized(mut self, duration: Time) -> Self {
        self.transform = self.transform.sanitized();
        self.crop = self.crop.sanitized();
        self.opacity = if self.opacity.is_finite() {
            self.opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.color = self.color.sanitized();
        for e in &mut self.effects {
            e.intensity = if e.intensity.is_finite() {
                e.intensity.clamp(0.0, 1.0)
            } else {
                1.0
            };
            for p in &mut e.params {
                if !p.is_finite() {
                    *p = 0.0;
                }
            }
        }
        if let Some(f) = &mut self.filter {
            f.intensity = if f.intensity.is_finite() {
                f.intensity.clamp(0.0, 1.0)
            } else {
                1.0
            };
        }
        self.fade_in = self.fade_in.max(Time::ZERO).min(duration);
        self.fade_out = self.fade_out.max(Time::ZERO).min(duration - self.fade_in);
        self
    }
}

/// A generated picture with no media file.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "fill", rename_all = "snake_case")]
pub enum Generated {
    Solid {
        color: Rgba,
    },
    /// Linear gradient from `from` to `to`; `angle` in degrees, 0 = left to
    /// right, 90 = top to bottom.
    Gradient {
        from: Rgba,
        to: Rgba,
        angle: f32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_identity() {
        assert!(Transform::default().is_identity());
        assert!(Look::default().is_default());
        let json = serde_json::to_string(&Look::default()).unwrap();
        let back: Look = serde_json::from_str("{}").unwrap();
        assert_eq!(back, Look::default(), "{json}");
    }

    #[test]
    fn fades_ramp_and_never_overlap() {
        let mut l = Look {
            fade_in: Time::from_secs(1),
            fade_out: Time::from_secs(2),
            ..Look::default()
        };
        let d = Time::from_secs(10);
        assert_eq!(l.fade_at(Time::ZERO, d), 0.0);
        assert_eq!(l.fade_at(Time::from_millis(500), d), 0.5);
        assert_eq!(l.fade_at(Time::from_secs(5), d), 1.0);
        assert_eq!(l.fade_at(Time::from_secs(9), d), 0.5);
        l.fade_in = Time::from_secs(8);
        let l = l.sanitized(d);
        assert_eq!(l.fade_in + l.fade_out, d);
    }

    #[test]
    fn crop_sanitizing_and_ratio() {
        let c = Crop {
            left: 0.7,
            right: 0.7,
            top: -1.0,
            bottom: f32::NAN,
        }
        .sanitized();
        assert!(c.kept().0 >= Crop::MIN_KEEP - 1e-6);
        assert_eq!((c.top, c.bottom), (0.0, 0.0));
        // 16:9 source cropped to 1:1 keeps the middle 9/16 of the width.
        let sq = Crop::to_ratio((1920, 1080), 1.0);
        assert!((sq.kept().0 - 0.5625).abs() < 1e-4 && sq.top == 0.0);
        // 16:9 source to 21:9 cuts top and bottom.
        let wide = Crop::to_ratio((1920, 1080), 21.0 / 9.0);
        assert!(wide.left == 0.0 && wide.top > 0.0);
    }

    #[test]
    fn layouts_and_sanitizing() {
        let t = Transform {
            rotation: 30.0,
            flip_h: true,
            ..Transform::default()
        };
        let pip = Layout::BottomRight.apply(t);
        assert_eq!(pip.rotation, 30.0);
        assert!(pip.flip_h && pip.center[0] > 0.5 && pip.size[0] < 0.5);
        assert_eq!(Layout::SplitLeft.apply(t).fit, Fit::Cover);
        let wild = Transform {
            rotation: -90.0,
            size: [0.0, f32::INFINITY],
            ..t
        }
        .sanitized();
        assert_eq!(wild.rotation, 270.0);
        assert_eq!(wild.size, [0.01, 1.0]);
    }

    #[test]
    fn color_controls_clamp() {
        let mut c = ColorAdjust::default();
        c.set(ColorControl::Exposure, 9.0);
        c.set(ColorControl::Contrast, -2.0);
        assert_eq!((c.exposure, c.contrast), (3.0, -1.0));
        assert!(!c.is_neutral());
        assert_eq!(BlendMode::Difference.index(), 9);
    }
}
