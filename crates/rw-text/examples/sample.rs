//! Renders sample lines to a PNG for eyeballing:
//! `cargo run -p rw-text --example sample -- out.png`

use rw_model::{Outline, Rgba, TextBox, TextStyle};

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "text-sample.png".into());
    let lines: Vec<(&str, TextStyle)> = vec![
        ("Reelwright Title", TextStyle::default()),
        (
            "مرحبا بالعالم",
            TextStyle {
                font: "Noto Sans Arabic".into(),
                ..TextStyle::default()
            },
        ),
        (
            "नमस्ते दुनिया",
            TextStyle {
                shadow: None,
                ..TextStyle::default()
            },
        ),
        (
            "你好，世界 こんにちは 안녕하세요",
            TextStyle {
                weight: 400,
                ..TextStyle::default()
            },
        ),
        (
            "Party time 🎬🎉👍🏽",
            TextStyle {
                outline: Some(Outline {
                    width: 0.08,
                    color: Rgba([10, 10, 10, 255]),
                }),
                ..TextStyle::default()
            },
        ),
        (
            "Lower third\nwith a box",
            TextStyle {
                font: "Lora".into(),
                align: rw_model::TextAlign::Left,
                shadow: None,
                background: Some(TextBox {
                    color: Rgba([14, 124, 116, 230]),
                    padding: 0.4,
                    radius: 0.25,
                }),
                ..TextStyle::default()
            },
        ),
    ];
    let imgs: Vec<_> = lines
        .iter()
        .map(|(t, s)| rw_text::render(t, s, 720.0, None).1)
        .collect();
    let w = imgs.iter().map(|i| i.width).max().unwrap_or(1) + 20;
    let h: u32 = imgs.iter().map(|i| i.height + 10).sum::<u32>() + 10;
    let mut sheet = image::RgbaImage::from_pixel(w, h, image::Rgba([60, 64, 80, 255]));
    let mut y = 10;
    for img in &imgs {
        let src = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone()).unwrap();
        image::imageops::overlay(&mut sheet, &src, 10, i64::from(y));
        y += img.height + 10;
    }
    sheet.save(&out).unwrap();
    println!("wrote {out} ({w}x{h})");
}
