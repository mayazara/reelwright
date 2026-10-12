//! M5 acceptance: text in Arabic, Hindi, CJK and emoji renders correctly.
//!
//! Arabic and Devanagari use the bundled Noto fonts. CJK and emoji come
//! from the computer's fonts (too large to bundle); set
//! `RW_REQUIRE_SCRIPT_FONTS=1` where those are installed (CI) to fail
//! instead of skipping when they're missing. `RW_FONT_DIRS` adds folders.

use rw_model::{Rgba, TextStyle};

fn style() -> TextStyle {
    TextStyle {
        shadow: None,
        weight: 400,
        ..TextStyle::default()
    }
}

fn render(text: &str) -> std::sync::Arc<rw_text::TextImage> {
    rw_text::render(text, &style(), 1080.0, None).1
}

fn inked(img: &rw_text::TextImage) -> usize {
    img.pixels.chunks(4).filter(|p| p[3] > 128).count()
}

fn require_script_fonts() -> bool {
    std::env::var("RW_REQUIRE_SCRIPT_FONTS").is_ok_and(|v| v == "1")
}

#[test]
fn arabic_joins_and_has_every_glyph() {
    let word = "مرحبا";
    let img = render(word);
    assert!(img.all_glyphs_found, "missing Arabic glyphs");
    assert!(inked(&img) > 500);
    // Joined letters are narrower than the same letters standing alone.
    let alone: u32 = word.chars().map(|c| render(&c.to_string()).width).sum();
    assert!(
        img.width + 40 < alone,
        "not joined: {} vs {} separately",
        img.width,
        alone
    );
    // Right to left: the sentence renders with all glyphs too.
    assert!(render("مرحبا بالعالم").all_glyphs_found);
}

#[test]
fn devanagari_forms_conjuncts() {
    let img = render("नमस्ते दुनिया");
    assert!(img.all_glyphs_found, "missing Devanagari glyphs");
    assert!(inked(&img) > 500);
    // स्ते is four characters shaped into fewer glyphs.
    let conjunct = render("स्ते");
    assert!(
        conjunct.glyphs < "स्ते".chars().count(),
        "{} glyphs",
        conjunct.glyphs
    );
}

#[test]
fn cjk_renders_from_system_fonts() {
    let img = render("你好，世界 こんにちは 안녕하세요");
    if !img.all_glyphs_found {
        assert!(!require_script_fonts(), "no CJK font on this computer");
        eprintln!("skipping: no CJK font installed");
        return;
    }
    assert!(inked(&img) > 2000);
}

#[test]
fn emoji_render_in_colour() {
    let img = render("🎬🎉👍🏽");
    if !img.all_glyphs_found {
        assert!(!require_script_fonts(), "no emoji font on this computer");
        eprintln!("skipping: no emoji font installed");
        return;
    }
    // Colour emoji have saturated pixels; white text alone has none.
    let colourful = img
        .pixels
        .chunks(4)
        .filter(|p| {
            p[3] > 200 && {
                let max = p[0].max(p[1]).max(p[2]);
                let min = p[0].min(p[1]).min(p[2]);
                max - min > 80
            }
        })
        .count();
    assert!(colourful > 200, "only {colourful} coloured pixels");
}

#[test]
fn outline_box_and_shadow_grow_the_image() {
    let plain = render("Title");
    let mut s = style();
    s.outline = Some(rw_model::Outline {
        width: 0.1,
        color: Rgba([0, 0, 0, 255]),
    });
    s.background = Some(rw_model::TextBox {
        color: Rgba([20, 20, 20, 200]),
        padding: 0.5,
        radius: 0.3,
    });
    let styled = rw_text::render("Title", &s, 1080.0, None).1;
    assert!(styled.width > plain.width && styled.height > plain.height);
    // The box's corner is rounded: transparent; its edge midpoint opaque.
    let px = |img: &rw_text::TextImage, x: u32, y: u32| {
        img.pixels[((y * img.width + x) * 4 + 3) as usize]
    };
    assert_eq!(px(&styled, 0, 0), 0);
    assert!(px(&styled, styled.width / 2, 4) > 150);
}

#[test]
fn typing_on_keeps_the_layout() {
    let full = rw_text::render("Hello world", &style(), 1080.0, None).1;
    let part = rw_text::render("Hello world", &style(), 1080.0, Some(5)).1;
    assert_eq!((full.width, full.height), (part.width, part.height));
    assert!(inked(&part) < inked(&full));
    assert_eq!(part.drawn_chars, 5);
}
