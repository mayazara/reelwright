//! The effect and transition catalogue: names, parameters and defaults.
//! The shaders that draw them live in `rw-render`.

use rw_model::{Effect, EffectKind, TransitionKind};

/// One adjustable value of an effect besides its intensity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Param {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

const fn param(name: &'static str, min: f32, max: f32, default: f32) -> Param {
    Param {
        name,
        min,
        max,
        default,
    }
}

/// How an effect is presented and set up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectInfo {
    pub kind: EffectKind,
    pub name: &'static str,
    /// Intensity when first added.
    pub intensity: f32,
    /// Extra values, in the order stored in [`Effect::params`] (at most 8).
    pub params: &'static [Param],
}

const CHROMA_KEY: &[Param] = &[
    param("Key red", 0.0, 1.0, 0.0),
    param("Key green", 0.0, 1.0, 1.0),
    param("Key blue", 0.0, 1.0, 0.0),
    param("Softness", 0.0, 1.0, 0.2),
    param("Spill removal", 0.0, 1.0, 0.5),
];

const SLOW_ZOOM: &[Param] = &[
    // Where the zoom ends up, as a share of the frame from its centre.
    param("Pan across", -1.0, 1.0, 0.0),
    param("Pan down", -1.0, 1.0, 0.0),
    // 1 zooms in, 0 zooms out.
    param("Zoom in", 0.0, 1.0, 1.0),
];

const VIGNETTE: &[Param] = &[param("Size", 0.0, 1.0, 0.5)];

const fn info(kind: EffectKind, name: &'static str, intensity: f32) -> EffectInfo {
    EffectInfo {
        kind,
        name,
        intensity,
        params: &[],
    }
}

/// Every effect, in library order.
pub const EFFECTS: [EffectInfo; 16] = [
    info(EffectKind::Blur, "Blur", 0.5),
    info(EffectKind::BackgroundBlurFill, "Blurred backdrop", 1.0),
    info(EffectKind::Glitch, "Glitch", 0.5),
    info(EffectKind::RgbSplit, "Colour split", 0.5),
    info(EffectKind::Vhs, "Tape", 0.7),
    info(EffectKind::FilmGrain, "Grain", 0.5),
    EffectInfo {
        params: VIGNETTE,
        ..info(EffectKind::Vignette, "Vignette", 0.6)
    },
    info(EffectKind::Pixelate, "Pixelate", 0.4),
    EffectInfo {
        params: SLOW_ZOOM,
        ..info(EffectKind::SlowZoom, "Slow zoom", 0.5)
    },
    info(EffectKind::Shake, "Shake", 0.4),
    info(EffectKind::Haze, "Haze", 0.5),
    info(EffectKind::Sharpen, "Sharpen", 0.5),
    info(EffectKind::BlackAndWhite, "Black and white", 1.0),
    info(EffectKind::Sepia, "Sepia", 1.0),
    info(EffectKind::Invert, "Invert", 1.0),
    EffectInfo {
        params: CHROMA_KEY,
        ..info(EffectKind::ChromaKey, "Green screen", 0.4)
    },
];

pub fn effect_info(kind: EffectKind) -> &'static EffectInfo {
    EFFECTS
        .iter()
        .find(|e| e.kind == kind)
        .unwrap_or(&EFFECTS[0])
}

/// A new effect with default settings.
pub fn new_effect(kind: EffectKind) -> Effect {
    let info = effect_info(kind);
    Effect {
        kind,
        intensity: info.intensity,
        params: info.params.iter().map(|p| p.default).collect(),
    }
}

/// Values of every parameter: stored ones clamped to range, defaults for
/// missing or invalid ones.
pub fn resolved_params(effect: &Effect) -> [f32; 8] {
    let mut out = [0.0; 8];
    for (i, p) in effect_info(effect.kind).params.iter().enumerate().take(8) {
        out[i] = effect
            .params
            .get(i)
            .copied()
            .filter(|v| v.is_finite())
            .map_or(p.default, |v| v.clamp(p.min, p.max));
    }
    out
}

/// Transitions in library order.
pub const TRANSITIONS: [TransitionKind; 20] = TransitionKind::ALL;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_effect_has_one_entry_and_sane_params() {
        for kind in EffectKind::ALL {
            assert_eq!(EFFECTS.iter().filter(|e| e.kind == kind).count(), 1);
            let info = effect_info(kind);
            assert!(info.params.len() <= 8);
            for p in info.params {
                assert!(p.min <= p.default && p.default <= p.max, "{}", p.name);
            }
        }
        let mut e = new_effect(EffectKind::ChromaKey);
        assert_eq!(resolved_params(&e)[..3], [0.0, 1.0, 0.0]);
        e.params = vec![f32::NAN, 7.0];
        assert_eq!(resolved_params(&e)[..4], [0.0, 1.0, 0.0, 0.2]);
    }
}
