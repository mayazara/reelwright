//! Graphics items: stickers and emoji from a pack, and vector shapes.
//!
//! Like text, a graphic has a natural size (a share of the canvas height)
//! that its transform's size scales.

use crate::project::Rgba;
use serde::{Deserialize, Serialize};

/// Natural height of a sticker, as a share of the canvas height.
pub const STICKER_HEIGHT: f32 = 0.25;
/// Natural height of a shape, as a share of the canvas height.
pub const SHAPE_HEIGHT: f32 = 0.3;

/// A picture from a sticker pack: the bundled set or one the user added.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StickerRef {
    /// Pack name (`openmoji` for the bundled emoji).
    pub pack: String,
    /// Picture within the pack (its file name without extension).
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Triangle,
    Star,
    Line,
    Arrow,
    DoubleArrow,
    SpeechBubble,
    Heart,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 9] = [
        ShapeKind::Rectangle,
        ShapeKind::Ellipse,
        ShapeKind::Triangle,
        ShapeKind::Star,
        ShapeKind::Line,
        ShapeKind::Arrow,
        ShapeKind::DoubleArrow,
        ShapeKind::SpeechBubble,
        ShapeKind::Heart,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::Ellipse => "Circle",
            ShapeKind::Triangle => "Triangle",
            ShapeKind::Star => "Star",
            ShapeKind::Line => "Line",
            ShapeKind::Arrow => "Arrow",
            ShapeKind::DoubleArrow => "Double arrow",
            ShapeKind::SpeechBubble => "Speech bubble",
            ShapeKind::Heart => "Heart",
        }
    }

    /// Lines and arrows are drawn with the stroke alone.
    pub fn is_line(self) -> bool {
        matches!(
            self,
            ShapeKind::Line | ShapeKind::Arrow | ShapeKind::DoubleArrow
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShapeStroke {
    pub color: Rgba,
    /// Line width as a share of the shape's height.
    pub width: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeSpec {
    pub kind: ShapeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Rgba>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<ShapeStroke>,
    /// Corner rounding as a share of the shorter side (rectangles, bubbles).
    #[serde(default)]
    pub corner_radius: f32,
    /// Width divided by height.
    pub aspect: f32,
}

impl ShapeSpec {
    /// A shape with the usual look for its kind.
    pub fn new(kind: ShapeKind) -> Self {
        let teal = Rgba([14, 124, 116, 255]);
        let white = Rgba([255, 255, 255, 255]);
        let (fill, stroke, aspect, radius) = match kind {
            ShapeKind::Line | ShapeKind::Arrow | ShapeKind::DoubleArrow => (
                None,
                Some(ShapeStroke {
                    color: white,
                    width: 0.12,
                }),
                4.0,
                0.0,
            ),
            ShapeKind::Rectangle => (Some(teal), None, 1.6, 0.08),
            ShapeKind::SpeechBubble => (
                Some(white),
                Some(ShapeStroke {
                    color: Rgba([18, 20, 24, 255]),
                    width: 0.03,
                }),
                1.5,
                0.25,
            ),
            ShapeKind::Heart => (Some(Rgba([236, 104, 88, 255])), None, 1.1, 0.0),
            ShapeKind::Star => (Some(Rgba([242, 165, 65, 255])), None, 1.05, 0.0),
            _ => (Some(teal), None, 1.0, 0.0),
        };
        ShapeSpec {
            kind,
            fill,
            stroke,
            corner_radius: radius,
            aspect,
        }
    }

    pub fn sanitized(mut self) -> Self {
        let fix = |v: f32, lo: f32, hi: f32, d: f32| {
            if v.is_finite() { v.clamp(lo, hi) } else { d }
        };
        self.aspect = fix(self.aspect, 0.05, 20.0, 1.0);
        self.corner_radius = fix(self.corner_radius, 0.0, 0.5, 0.0);
        if let Some(s) = &mut self.stroke {
            s.width = fix(s.width, 0.0, 0.5, 0.05);
        }
        if self.kind.is_line() && self.stroke.is_none() {
            self.stroke = ShapeSpec::new(self.kind).stroke;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_always_have_a_stroke() {
        let mut s = ShapeSpec::new(ShapeKind::Arrow);
        s.stroke = None;
        s.aspect = f32::NAN;
        let s = s.sanitized();
        assert!(s.stroke.is_some());
        assert_eq!(s.aspect, 1.0);
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<ShapeSpec>(&j).unwrap(), s);
    }
}
