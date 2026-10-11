//! Compositor output checks on a headless GPU.

use rw_media::RgbaImage;
use rw_model::Rgba;
use rw_render::{Compositor, Gpu, Layer};
use std::sync::Arc;

/// Headless GPU, or `None` (test skipped) when the machine has none.
/// `RW_REQUIRE_GPU=1` turns a missing GPU into a failure.
fn gpu() -> Option<Gpu> {
    match Gpu::headless() {
        Ok(g) => Some(g),
        Err(e) if std::env::var("RW_REQUIRE_GPU").is_ok_and(|v| v == "1") => panic!("{e}"),
        Err(e) => {
            eprintln!("skipping GPU test: {e}");
            None
        }
    }
}

fn solid(w: u32, h: u32, c: [u8; 4]) -> Arc<RgbaImage> {
    Arc::new(RgbaImage::new(w, h, c.repeat((w * h) as usize)))
}

fn px(img: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * img.width + x) * 4) as usize;
    img.pixels[i..i + 4].try_into().unwrap()
}

fn close(a: [u8; 4], b: [u8; 4], tol: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tol)
}

fn layer(image: Arc<RgbaImage>, key: u64) -> Layer {
    Layer {
        image,
        key,
        rotation: 0,
        opacity: 1.0,
    }
}

#[test]
fn background_and_letterbox() {
    let Some(gpu) = gpu() else { return };
    let mut c = Compositor::new(&gpu, (160, 90));
    let bg = Rgba([20, 40, 60, 255]);
    c.render(bg, &[]);
    let out = c.read_back().unwrap();
    assert!(
        close(px(&out, 80, 45), [20, 40, 60, 255], 1),
        "{:?}",
        px(&out, 80, 45)
    );

    // A 4:3 red picture in a 16:9 frame: pillarboxed, bars show background.
    c.render(bg, &[layer(solid(40, 30, [255, 0, 0, 255]), 1)]);
    let out = c.read_back().unwrap();
    assert!(close(px(&out, 80, 45), [255, 0, 0, 255], 1));
    assert!(
        close(px(&out, 5, 45), [20, 40, 60, 255], 1),
        "left bar {:?}",
        px(&out, 5, 45)
    );
    assert!(close(px(&out, 154, 45), [20, 40, 60, 255], 1), "right bar");
    // Picture spans 120 px wide: columns 20..140.
    assert!(close(px(&out, 22, 45), [255, 0, 0, 255], 1));
}

#[test]
fn opacity_blends_in_linear_light() {
    let Some(gpu) = gpu() else { return };
    let mut c = Compositor::new(&gpu, (64, 64));
    let mut top = layer(solid(8, 8, [0, 0, 255, 255]), 2);
    top.opacity = 0.5;
    c.render(Rgba::BLACK, &[layer(solid(8, 8, [255, 0, 0, 255]), 1), top]);
    let p = px(&c.read_back().unwrap(), 32, 32);
    // 50 % linear light encodes to 188 in sRGB (not 128).
    assert!(close(p, [188, 0, 188, 255], 2), "{p:?}");
}

#[test]
fn transparent_pixels_show_what_is_below() {
    let Some(gpu) = gpu() else { return };
    let mut c = Compositor::new(&gpu, (32, 32));
    c.render(
        Rgba([0, 255, 0, 255]),
        &[layer(solid(4, 4, [255, 0, 0, 0]), 1)],
    );
    assert!(close(
        px(&c.read_back().unwrap(), 16, 16),
        [0, 255, 0, 255],
        1
    ));
}

#[test]
fn rotation_turns_the_picture() {
    let Some(gpu) = gpu() else { return };
    // 40x20 picture: left half red, right half green.
    let mut pixels = Vec::new();
    for _y in 0..20 {
        for x in 0..40 {
            pixels.extend(if x < 20 {
                [255, 0, 0, 255]
            } else {
                [0, 255, 0, 255]
            });
        }
    }
    let img = Arc::new(RgbaImage::new(40, 20, pixels));
    let mut c = Compositor::new(&gpu, (100, 200));
    let mut l = layer(img.clone(), 1);
    l.rotation = 90;
    c.render(Rgba::BLACK, &[l.clone()]);
    let out = c.read_back().unwrap();
    // Rotated clockwise: the left (red) edge becomes the top.
    assert!(
        close(px(&out, 50, 40), [255, 0, 0, 255], 2),
        "top {:?}",
        px(&out, 50, 40)
    );
    assert!(
        close(px(&out, 50, 160), [0, 255, 0, 255], 2),
        "bottom {:?}",
        px(&out, 50, 160)
    );
    l.rotation = 270;
    l.key = 2;
    c.render(Rgba::BLACK, &[l]);
    let out = c.read_back().unwrap();
    assert!(close(px(&out, 50, 40), [0, 255, 0, 255], 2));
    // The CPU rotation used for thumbnails agrees with the shader.
    assert_eq!(img.rotated(90).pixels[..4], [255, 0, 0, 255]);
}

#[test]
fn same_key_skips_upload_and_resize_works() {
    let Some(gpu) = gpu() else { return };
    let mut c = Compositor::new(&gpu, (16, 16));
    c.render(Rgba::BLACK, &[layer(solid(4, 4, [255, 0, 0, 255]), 7)]);
    // Same key, different pixels: the cached upload is reused.
    c.render(Rgba::BLACK, &[layer(solid(4, 4, [0, 0, 255, 255]), 7)]);
    assert!(close(
        px(&c.read_back().unwrap(), 8, 8),
        [255, 0, 0, 255],
        1
    ));
    c.render(Rgba::BLACK, &[layer(solid(4, 4, [0, 0, 255, 255]), 8)]);
    assert!(close(
        px(&c.read_back().unwrap(), 8, 8),
        [0, 0, 255, 255],
        1
    ));
    c.resize((64, 36));
    c.render(Rgba::BLACK, &[layer(solid(4, 4, [0, 0, 255, 255]), 8)]);
    let out = c.read_back().unwrap();
    assert_eq!((out.width, out.height), (64, 36));
}
