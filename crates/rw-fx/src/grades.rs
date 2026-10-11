//! The bundled filter looks, defined as colour functions. The `.cube`
//! files in `assets/luts/` are generated from these by
//! `cargo run -p rw-fx --example make_luts`; the app ships the files.

type Rgb = [f32; 3];

fn luma(c: Rgb) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn saturate(c: Rgb, amount: f32) -> Rgb {
    let l = luma(c);
    mix([l, l, l], c, amount)
}

fn contrast(c: Rgb, amount: f32) -> Rgb {
    c.map(|v| (v - 0.5) * amount + 0.5)
}

/// Smooth S-curve with adjustable strength.
fn s_curve(c: Rgb, strength: f32) -> Rgb {
    c.map(|v| {
        let s = v * v * (3.0 - 2.0 * v);
        v + (s - v) * strength
    })
}

/// Lifts blacks and lowers whites (faded film look).
fn fade(c: Rgb, lift: f32, cap: f32) -> Rgb {
    c.map(|v| lift + v * (cap - lift))
}

fn tint(c: Rgb, rgb: Rgb) -> Rgb {
    [c[0] * rgb[0], c[1] * rgb[1], c[2] * rgb[2]]
}

/// Colours shadows and highlights separately (split toning).
fn split_tone(c: Rgb, shadow: Rgb, highlight: Rgb, amount: f32) -> Rgb {
    let l = luma(c);
    let ws = (1.0 - l).powi(2) * amount;
    let wh = l.powi(2) * amount;
    [
        c[0] + (shadow[0] - 0.5) * ws + (highlight[0] - 0.5) * wh,
        c[1] + (shadow[1] - 0.5) * ws + (highlight[1] - 0.5) * wh,
        c[2] + (shadow[2] - 0.5) * ws + (highlight[2] - 0.5) * wh,
    ]
}

fn mono(c: Rgb, weights: Rgb) -> Rgb {
    let l = c[0] * weights[0] + c[1] * weights[1] + c[2] * weights[2];
    [l, l, l]
}

/// A bundled look: file stem, display name, and the colour function.
pub struct Grade {
    pub id: &'static str,
    pub name: &'static str,
    pub apply: fn(Rgb) -> Rgb,
}

pub const GRADES: &[Grade] = &[
    Grade {
        id: "mono",
        name: "Mono",
        apply: |c| mono(c, [0.2126, 0.7152, 0.0722]),
    },
    Grade {
        id: "mono-punch",
        name: "Mono punch",
        apply: |c| s_curve(mono(c, [0.3, 0.6, 0.1]), 0.9),
    },
    Grade {
        id: "silver",
        name: "Silver",
        apply: |c| {
            tint(
                fade(mono(c, [0.25, 0.65, 0.1]), 0.04, 0.97),
                [0.97, 1.0, 1.04],
            )
        },
    },
    Grade {
        id: "noir",
        name: "Noir",
        apply: |c| contrast(mono(c, [0.35, 0.55, 0.1]), 1.45),
    },
    Grade {
        id: "sepia",
        name: "Sepia",
        apply: |c| {
            let l = luma(c);
            [l * 1.07 + 0.03, l * 0.91 + 0.02, l * 0.72]
        },
    },
    Grade {
        id: "warm",
        name: "Warm",
        apply: |c| tint(c, [1.08, 1.01, 0.88]),
    },
    Grade {
        id: "cool",
        name: "Cool",
        apply: |c| tint(c, [0.9, 1.0, 1.1]),
    },
    Grade {
        id: "golden-hour",
        name: "Golden hour",
        apply: |c| s_curve(tint(saturate(c, 1.1), [1.12, 1.02, 0.82]), 0.25),
    },
    Grade {
        id: "blue-hour",
        name: "Blue hour",
        apply: |c| {
            split_tone(
                tint(c, [0.88, 0.95, 1.12]),
                [0.35, 0.45, 0.75],
                [0.55, 0.55, 0.6],
                0.4,
            )
        },
    },
    Grade {
        id: "harbor",
        name: "Harbor",
        apply: |c| {
            s_curve(
                split_tone(c, [0.3, 0.55, 0.6], [0.75, 0.55, 0.35], 0.55),
                0.35,
            )
        },
    },
    Grade {
        id: "ember",
        name: "Ember",
        apply: |c| {
            contrast(
                split_tone(saturate(c, 1.15), [0.45, 0.35, 0.3], [0.85, 0.55, 0.3], 0.6),
                1.08,
            )
        },
    },
    Grade {
        id: "faded-film",
        name: "Faded film",
        apply: |c| fade(saturate(c, 0.82), 0.08, 0.94),
    },
    Grade {
        id: "vintage",
        name: "Vintage",
        apply: |c| fade(tint(saturate(c, 0.7), [1.07, 1.0, 0.84]), 0.07, 0.95),
    },
    Grade {
        id: "retro",
        name: "Retro",
        apply: |c| {
            fade(
                split_tone(saturate(c, 0.85), [0.4, 0.55, 0.42], [0.72, 0.6, 0.42], 0.5),
                0.05,
                0.96,
            )
        },
    },
    Grade {
        id: "pastel",
        name: "Pastel",
        apply: |c| fade(saturate(contrast(c, 0.8), 0.75), 0.12, 1.0),
    },
    Grade {
        id: "vivid",
        name: "Vivid",
        apply: |c| s_curve(saturate(c, 1.4), 0.4),
    },
    Grade {
        id: "muted",
        name: "Muted",
        apply: |c| saturate(c, 0.55),
    },
    Grade {
        id: "matte",
        name: "Matte",
        apply: |c| fade(contrast(c, 0.85), 0.1, 0.92),
    },
    Grade {
        id: "forest",
        name: "Forest",
        apply: |c| tint(saturate(c, 1.1), [0.95, 1.06, 0.92]),
    },
    Grade {
        id: "desert",
        name: "Desert",
        apply: |c| fade(tint(saturate(c, 0.8), [1.1, 1.0, 0.8]), 0.03, 0.98),
    },
    Grade {
        id: "arctic",
        name: "Arctic",
        apply: |c| fade(tint(saturate(c, 0.85), [0.9, 1.0, 1.08]), 0.05, 1.0),
    },
    Grade {
        id: "neon",
        name: "Neon",
        apply: |c| {
            s_curve(
                split_tone(saturate(c, 1.5), [0.55, 0.3, 0.75], [0.35, 0.75, 0.7], 0.6),
                0.3,
            )
        },
    },
    Grade {
        id: "moonlight",
        name: "Moonlight",
        apply: |c| tint(contrast(saturate(c, 0.5), 1.1), [0.82, 0.9, 1.12]),
    },
    Grade {
        id: "rose",
        name: "Rosé",
        apply: |c| fade(tint(c, [1.06, 0.96, 1.0]), 0.04, 1.0),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_unique_and_outputs_in_range() {
        let ids: HashSet<_> = GRADES.iter().map(|g| g.id).collect();
        assert_eq!(ids.len(), GRADES.len());
        assert!(GRADES.len() >= 20, "plan asks for 20+ filters");
        for g in GRADES {
            for c in [
                [0.0, 0.0, 0.0],
                [1.0, 1.0, 1.0],
                [0.8, 0.2, 0.4],
                [0.1, 0.9, 0.6],
            ] {
                let out = (g.apply)(c);
                assert!(out.iter().all(|v| v.is_finite()), "{}", g.id);
            }
        }
    }
}
