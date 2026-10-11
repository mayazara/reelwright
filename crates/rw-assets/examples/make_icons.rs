//! Renders the app icon (the same shapes as `assets/branding/logo.svg`) to
//! `assets/branding/icon-256.png` and a multi-size `assets/branding/reelwright.ico`.
//! Run from the repository root: `cargo run -p rw-assets --example make_icons`.

use std::io::Write;
use std::path::Path;

/// Shapes in the logo's 64-unit coordinate space.
fn coverage(x: f32, y: f32) -> [f32; 4] {
    let teal = [0x0E as f32, 0x7C as f32, 0x74 as f32];
    let cream = [0xF4 as f32, 0xF1 as f32, 0xEA as f32];
    let amber = [0xF2 as f32, 0xA5 as f32, 0x41 as f32];
    let mut out = [0.0, 0.0, 0.0, 0.0];
    let mut paint = |c: [f32; 3]| out = [c[0], c[1], c[2], 255.0];
    // Rounded square: x,y in 2..62, radius 14.
    let (qx, qy) = (
        (x - 32.0).abs() - (30.0 - 14.0),
        (y - 32.0).abs() - (30.0 - 14.0),
    );
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - 14.0;
    if outside > 0.0 {
        return out;
    }
    paint(teal);
    // Reel ring: centre (30,34), radius 17, stroke 4.
    let d = ((x - 30.0).powi(2) + (y - 34.0).powi(2)).sqrt();
    if (d - 17.0).abs() <= 2.0 {
        paint(cream);
    }
    for (cx, cy) in [(30.0, 24.0), (21.3, 39.0), (38.7, 39.0)] {
        if ((x - cx) * (x - cx) + (y - cy) * (y - cy)).sqrt() <= 3.2 {
            paint(cream);
        }
    }
    // Play triangle (44,8)-(56,14)-(44,20).
    if (44.0..=56.0).contains(&x) && (y - 14.0).abs() <= (56.0 - x) / 2.0 {
        paint(amber);
    }
    out
}

/// RGBA pixels at `size`, 4×4 supersampled.
fn render(size: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    let n = 4;
    for py in 0..size {
        for pxl in 0..size {
            let mut acc = [0.0f32; 4];
            for sy in 0..n {
                for sx in 0..n {
                    let x = (pxl as f32 + (sx as f32 + 0.5) / n as f32) * 64.0 / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / n as f32) * 64.0 / size as f32;
                    let c = coverage(x, y);
                    // Premultiply while averaging.
                    for k in 0..3 {
                        acc[k] += c[k] * c[3] / 255.0;
                    }
                    acc[3] += c[3];
                }
            }
            let s = (n * n) as f32;
            let a = acc[3] / s;
            let un = |v: f32| {
                if a > 0.0 {
                    (v / s * 255.0 / a).round().clamp(0.0, 255.0) as u8
                } else {
                    0
                }
            };
            px.extend([un(acc[0]), un(acc[1]), un(acc[2]), a.round() as u8]);
        }
    }
    px
}

fn png(size: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new(&mut out);
    image::ImageEncoder::write_image(
        enc,
        &render(size),
        size,
        size,
        image::ExtendedColorType::Rgba8,
    )
    .expect("encode png");
    out
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/branding");
    std::fs::write(dir.join("icon-256.png"), png(256)).expect("write png");
    // ICO with PNG-compressed entries (supported since Windows Vista).
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let images: Vec<Vec<u8>> = sizes.iter().map(|s| png(*s)).collect();
    let mut ico = Vec::new();
    ico.write_all(&[0, 0, 1, 0]).unwrap();
    ico.write_all(&(sizes.len() as u16).to_le_bytes()).unwrap();
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, data) in sizes.iter().zip(&images) {
        let dim = if *s >= 256 { 0 } else { *s as u8 };
        ico.write_all(&[dim, dim, 0, 0]).unwrap();
        ico.write_all(&1u16.to_le_bytes()).unwrap();
        ico.write_all(&32u16.to_le_bytes()).unwrap();
        ico.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
        ico.write_all(&offset.to_le_bytes()).unwrap();
        offset += data.len() as u32;
    }
    for data in &images {
        ico.write_all(data).unwrap();
    }
    std::fs::write(dir.join("reelwright.ico"), ico).expect("write ico");
    println!("wrote icon-256.png and reelwright.ico");
}
