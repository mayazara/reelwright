//! Theme tokens and their mapping onto egui's visuals.
//!
//! All colours used by the UI come from [`Tokens`]; widgets never hard-code
//! colours. The palette is the app's own: a deep teal accent with an amber
//! highlight, in light and dark variants.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke};
use std::sync::Arc;

/// Light or dark appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    Light,
    #[default]
    Dark,
}

impl ThemeMode {
    pub fn toggled(self) -> Self {
        match self {
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Light,
        }
    }
}

/// Semantic colour and shape tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    /// Window background behind every panel.
    pub canvas: Color32,
    /// Panel surfaces.
    pub surface: Color32,
    /// Raised surfaces: cards, popups, the inactive widget fill.
    pub surface_raised: Color32,
    /// Hovered widget fill.
    pub surface_hover: Color32,
    /// Hairline separators and widget outlines.
    pub border: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    /// Primary brand accent: selection, primary buttons, playhead.
    pub accent: Color32,
    /// Text drawn on top of `accent`.
    pub on_accent: Color32,
    /// Secondary highlight: markers, warnings that are not errors.
    pub highlight: Color32,
    pub danger: Color32,
    /// Preview area behind the video canvas.
    pub preview_backdrop: Color32,
    pub radius: u8,
}

impl Tokens {
    pub const fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => DARK,
            ThemeMode::Light => LIGHT,
        }
    }
}

const DARK: Tokens = Tokens {
    canvas: Color32::from_rgb(0x14, 0x17, 0x19),
    surface: Color32::from_rgb(0x1C, 0x20, 0x23),
    surface_raised: Color32::from_rgb(0x26, 0x2B, 0x2F),
    surface_hover: Color32::from_rgb(0x31, 0x37, 0x3C),
    border: Color32::from_rgb(0x36, 0x3D, 0x42),
    text: Color32::from_rgb(0xE8, 0xE6, 0xE1),
    text_muted: Color32::from_rgb(0x9A, 0xA2, 0xA6),
    accent: Color32::from_rgb(0x2B, 0xB3, 0xA6),
    on_accent: Color32::from_rgb(0x08, 0x1C, 0x1A),
    highlight: Color32::from_rgb(0xF2, 0xA5, 0x41),
    danger: Color32::from_rgb(0xE5, 0x5B, 0x4F),
    preview_backdrop: Color32::from_rgb(0x0C, 0x0E, 0x0F),
    radius: 6,
};

const LIGHT: Tokens = Tokens {
    canvas: Color32::from_rgb(0xEC, 0xEA, 0xE5),
    surface: Color32::from_rgb(0xF7, 0xF5, 0xF1),
    surface_raised: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    surface_hover: Color32::from_rgb(0xE4, 0xE1, 0xDA),
    border: Color32::from_rgb(0xD3, 0xCF, 0xC6),
    text: Color32::from_rgb(0x1E, 0x23, 0x26),
    text_muted: Color32::from_rgb(0x5E, 0x67, 0x6C),
    accent: Color32::from_rgb(0x0E, 0x7C, 0x74),
    on_accent: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    highlight: Color32::from_rgb(0xC9, 0x7B, 0x12),
    danger: Color32::from_rgb(0xB8, 0x3A, 0x2F),
    preview_backdrop: Color32::from_rgb(0x2A, 0x2E, 0x31),
    radius: 6,
};

/// Builds egui visuals from the tokens.
pub fn visuals(mode: ThemeMode) -> egui::Visuals {
    let t = Tokens::for_mode(mode);
    let mut v = match mode {
        ThemeMode::Dark => egui::Visuals::dark(),
        ThemeMode::Light => egui::Visuals::light(),
    };
    let radius = CornerRadius::same(t.radius);
    v.override_text_color = Some(t.text);
    v.window_fill = t.surface_raised;
    v.panel_fill = t.surface;
    v.faint_bg_color = t.surface_raised;
    v.extreme_bg_color = t.canvas;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.window_corner_radius = radius;
    v.menu_corner_radius = radius;
    v.hyperlink_color = t.accent;
    v.warn_fg_color = t.highlight;
    v.error_fg_color = t.danger;
    v.selection.bg_fill = t.accent;
    v.selection.stroke = Stroke::new(1.0, t.on_accent);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.text_muted);
    for (state, fill) in [
        (&mut w.inactive, t.surface_raised),
        (&mut w.hovered, t.surface_hover),
        (&mut w.active, t.surface_hover),
        (&mut w.open, t.surface_hover),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.corner_radius = radius;
        state.fg_stroke = Stroke::new(1.0, t.text);
    }
    w.inactive.bg_stroke = Stroke::new(1.0, t.border);
    w.hovered.bg_stroke = Stroke::new(1.0, t.accent);
    w.active.bg_stroke = Stroke::new(1.0, t.accent);
    v
}

/// Applies the theme to a context.
pub fn apply(ctx: &egui::Context, mode: ThemeMode) {
    ctx.set_visuals(visuals(mode));
}

const INTER_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/ui/Inter-Regular.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/ui/Inter-SemiBold.ttf");

/// Name of the semi-bold font family used for headings.
pub const HEADING_FAMILY: &str = "ui-semibold";

/// Font set using the bundled Inter faces for every family.
pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::empty();
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(INTER_REGULAR)),
    );
    fonts.font_data.insert(
        "inter-semibold".into(),
        Arc::new(FontData::from_static(INTER_SEMIBOLD)),
    );
    fonts
        .families
        .insert(FontFamily::Proportional, vec!["inter".into()]);
    fonts
        .families
        .insert(FontFamily::Monospace, vec!["inter".into()]);
    fonts.families.insert(
        FontFamily::Name(HEADING_FAMILY.into()),
        vec!["inter-semibold".into(), "inter".into()],
    );
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance of an sRGB colour.
    fn luminance(c: Color32) -> f64 {
        let lin = |v: u8| {
            let s = f64::from(v) / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn text_meets_wcag_aa_on_surfaces() {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let t = Tokens::for_mode(mode);
            for bg in [t.canvas, t.surface, t.surface_raised] {
                assert!(contrast(t.text, bg) >= 4.5, "{mode:?} text");
                assert!(contrast(t.text_muted, bg) >= 4.5, "{mode:?} muted");
            }
            assert!(contrast(t.on_accent, t.accent) >= 4.5, "{mode:?} accent");
        }
    }

    #[test]
    fn toggle_round_trips() {
        assert_eq!(ThemeMode::Dark.toggled().toggled(), ThemeMode::Dark);
    }
}
