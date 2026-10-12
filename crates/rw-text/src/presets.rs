//! Ready-made text styles offered in the Text library.

use rw_model::{
    Outline, Rgba, Shadow, TextAlign, TextAnimation, TextBehaviour, TextBox, TextMotion, TextSpec,
    TextStyle, Time,
};

/// A starting point for a text item.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub spec: TextSpec,
    /// Where it sits, as shares of the canvas.
    pub center: [f32; 2],
    pub duration: Time,
}

const INK: Rgba = Rgba([18, 20, 24, 255]);
const TEAL: Rgba = Rgba([14, 124, 116, 235]);
const AMBER: Rgba = Rgba([242, 165, 65, 255]);
const CREAM: Rgba = Rgba([246, 238, 222, 255]);

fn soft_shadow() -> Option<Shadow> {
    Some(Shadow {
        offset: [0.03, 0.05],
        blur: 0.1,
        color: Rgba([0, 0, 0, 150]),
    })
}

fn spec(text: &str, style: TextStyle, enter: TextMotion, exit: TextMotion) -> TextSpec {
    TextSpec {
        text: text.into(),
        style,
        enter: TextAnimation::new(enter),
        exit: TextAnimation::new(exit),
        behaviour: TextBehaviour::Static,
    }
}

fn style(font: &str, size: f32, weight: u16) -> TextStyle {
    TextStyle {
        font: font.into(),
        size,
        weight,
        shadow: soft_shadow(),
        ..TextStyle::default()
    }
}

/// Every preset, in library order.
pub fn all() -> Vec<Preset> {
    let five = Time::from_secs(5);
    let mut out = vec![
        Preset {
            name: "Title",
            spec: spec(
                "Your title here",
                style("Montserrat", 0.11, 800),
                TextMotion::Pop,
                TextMotion::Fade,
            ),
            center: [0.5, 0.5],
            duration: five,
        },
        Preset {
            name: "Subtitle",
            spec: spec(
                "A line to set the scene",
                TextStyle {
                    italic: true,
                    color: CREAM,
                    ..style("Lora", 0.05, 400)
                },
                TextMotion::Fade,
                TextMotion::Fade,
            ),
            center: [0.5, 0.62],
            duration: five,
        },
        Preset {
            name: "Lower third",
            spec: spec(
                "Name Surname\nRole or place",
                TextStyle {
                    align: TextAlign::Left,
                    shadow: None,
                    background: Some(TextBox {
                        color: TEAL,
                        padding: 0.45,
                        radius: 0.2,
                    }),
                    line_spacing: 1.25,
                    ..style("DM Sans", 0.045, 600)
                },
                TextMotion::SlideRight,
                TextMotion::SlideRight,
            ),
            center: [0.25, 0.8],
            duration: five,
        },
        Preset {
            name: "Quote card",
            spec: spec(
                "\u{201C}Say something worth\nremembering.\u{201D}\n\u{2014} Someone",
                TextStyle {
                    italic: true,
                    line_spacing: 1.35,
                    shadow: None,
                    background: Some(TextBox {
                        color: Rgba([18, 20, 24, 210]),
                        padding: 0.8,
                        radius: 0.4,
                    }),
                    ..style("Playfair Display", 0.06, 500)
                },
                TextMotion::Fade,
                TextMotion::Fade,
            ),
            center: [0.5, 0.5],
            duration: five,
        },
        Preset {
            name: "Big statement",
            spec: spec(
                "MAKE IT LOUD",
                TextStyle {
                    color: AMBER,
                    outline: Some(Outline {
                        width: 0.06,
                        color: INK,
                    }),
                    ..style("Anton", 0.16, 400)
                },
                TextMotion::Zoom,
                TextMotion::Fade,
            ),
            center: [0.5, 0.5],
            duration: five,
        },
        Preset {
            name: "Typewriter",
            spec: TextSpec {
                enter: TextAnimation {
                    motion: TextMotion::Typewriter,
                    duration: Time::from_millis(1500),
                },
                ..spec(
                    "Typing it out, one letter at a time",
                    style("Space Mono", 0.045, 400),
                    TextMotion::Typewriter,
                    TextMotion::Fade,
                )
            },
            center: [0.5, 0.5],
            duration: five,
        },
        Preset {
            name: "Word by word",
            spec: TextSpec {
                enter: TextAnimation {
                    motion: TextMotion::WordByWord,
                    duration: Time::from_millis(1500),
                },
                ..spec(
                    "One word at a time",
                    style("Poppins", 0.08, 700),
                    TextMotion::WordByWord,
                    TextMotion::Fade,
                )
            },
            center: [0.5, 0.5],
            duration: five,
        },
        Preset {
            name: "Handwritten note",
            spec: spec(
                "a little note",
                style("Caveat", 0.1, 700),
                TextMotion::Bounce,
                TextMotion::Fade,
            ),
            center: [0.5, 0.3],
            duration: five,
        },
        Preset {
            name: "Countdown",
            spec: TextSpec {
                behaviour: TextBehaviour::Countdown,
                ..spec(
                    "5",
                    style("Bebas Neue", 0.3, 400),
                    TextMotion::None,
                    TextMotion::None,
                )
            },
            center: [0.5, 0.5],
            duration: five,
        },
        Preset {
            name: "End credits",
            spec: TextSpec {
                behaviour: TextBehaviour::Roll,
                ..spec(
                    "Directed by\nSomeone Wonderful\n\nEdited by\nYou\n\nMusic\nA Friend\n\nThanks for watching",
                    TextStyle {
                        line_spacing: 1.6,
                        shadow: None,
                        ..style("Nunito", 0.045, 500)
                    },
                    TextMotion::None,
                    TextMotion::None,
                )
            },
            center: [0.5, 0.5],
            duration: Time::from_secs(10),
        },
    ];
    for p in &mut out {
        p.spec = p.spec.clone().sanitized(p.duration);
    }
    out
}

/// Plain white text, for "Add text".
pub fn plain() -> Preset {
    Preset {
        name: "Plain text",
        spec: spec(
            "Your text",
            style("Inter", 0.07, 600),
            TextMotion::Fade,
            TextMotion::Fade,
        ),
        center: [0.5, 0.5],
        duration: Time::from_secs(5),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_use_bundled_fonts_and_render_fully() {
        let all = all();
        assert!(all.len() >= 10);
        let (bundled, _) = crate::font_families();
        for p in all.iter().chain([&plain()]) {
            assert!(
                bundled.contains(&p.spec.style.font) || p.spec.style.font == "Inter",
                "{}: font {} isn't bundled",
                p.name,
                p.spec.style.font
            );
            let text = p.spec.text_at(Time::ZERO, p.duration);
            let (_, img) = crate::render(&text, &p.spec.style, 1080.0, None);
            assert!(img.all_glyphs_found, "{}", p.name);
            assert!(img.width > 50, "{}", p.name);
        }
    }
}
