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
    Layer::image(image, key, 0)
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
    top.look.opacity = 0.5;
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

// ---- M3: placement, crop, blend, colour, filters, generated sources ----

use rw_model::{BlendMode, Crop, Filter, Fit, Transform};
use rw_render::Source;

/// 40x20 picture: left half red, right half green.
fn red_green() -> Arc<RgbaImage> {
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
    Arc::new(RgbaImage::new(40, 20, pixels))
}

fn render(gpu: &Gpu, size: (u32, u32), bg: Rgba, layers: &[Layer]) -> RgbaImage {
    let mut c = Compositor::new(gpu, size);
    c.render(bg, layers);
    c.read_back().unwrap()
}

#[test]
fn box_placement_and_flip() {
    let Some(gpu) = gpu() else { return };
    let mut l = layer(red_green(), 1);
    // Bottom-right quarter of a 200x100 canvas, stretched to the box.
    l.look.transform = Transform {
        center: [0.75, 0.75],
        size: [0.5, 0.5],
        fit: Fit::Stretch,
        ..Transform::default()
    };
    let out = render(&gpu, (200, 100), Rgba::BLACK, &[l.clone()]);
    assert!(
        close(px(&out, 50, 25), [0, 0, 0, 255], 1),
        "outside the box"
    );
    assert!(
        close(px(&out, 120, 75), [255, 0, 0, 255], 2),
        "left half of box is red"
    );
    assert!(close(px(&out, 180, 75), [0, 255, 0, 255], 2));
    l.look.transform.flip_h = true;
    let out = render(&gpu, (200, 100), Rgba::BLACK, &[l]);
    assert!(close(px(&out, 120, 75), [0, 255, 0, 255], 2), "flipped");
}

#[test]
fn fill_is_clipped_to_its_box_and_crop_selects_area() {
    let Some(gpu) = gpu() else { return };
    // A 2:1 picture filling a square box in the left half: overflow cut off.
    let mut l = layer(red_green(), 1);
    l.look.transform = Transform {
        center: [0.25, 0.5],
        size: [0.5, 1.0],
        fit: Fit::Cover,
        ..Transform::default()
    };
    let out = render(&gpu, (200, 100), Rgba([0, 0, 255, 255]), &[l]);
    assert!(
        close(px(&out, 150, 50), [0, 0, 255, 255], 1),
        "right half untouched"
    );
    assert!(close(px(&out, 10, 50), [255, 0, 0, 255], 2));
    assert!(close(px(&out, 90, 50), [0, 255, 0, 255], 2));
    // Cropping away the right half leaves only red.
    let mut l = layer(red_green(), 1);
    l.look.crop = Crop {
        right: 0.5,
        ..Crop::default()
    };
    let out = render(&gpu, (100, 100), Rgba::BLACK, &[l]);
    assert!(close(px(&out, 50, 50), [255, 0, 0, 255], 2));
    assert!(
        close(px(&out, 95, 50), [255, 0, 0, 255], 2),
        "square crop fills the square canvas"
    );
}

#[test]
fn blend_mode_identities() {
    let Some(gpu) = gpu() else { return };
    let base = layer(solid(4, 4, [200, 120, 40, 255]), 1);
    let check = |mode: BlendMode, top: [u8; 4], want: [u8; 4]| {
        let mut t = layer(solid(4, 4, top), 2);
        t.look.blend = mode;
        let out = render(&gpu, (16, 16), Rgba::BLACK, &[base.clone(), t]);
        let got = px(&out, 8, 8);
        assert!(close(got, want, 2), "{mode:?}: {got:?} want {want:?}");
    };
    check(
        BlendMode::Multiply,
        [255, 255, 255, 255],
        [200, 120, 40, 255],
    );
    check(BlendMode::Screen, [0, 0, 0, 255], [200, 120, 40, 255]);
    check(BlendMode::Difference, [200, 120, 40, 255], [0, 0, 0, 255]);
    check(BlendMode::Darken, [100, 200, 0, 255], [100, 120, 0, 255]);
    check(BlendMode::Lighten, [100, 200, 0, 255], [200, 200, 40, 255]);
    check(BlendMode::Add, [100, 100, 100, 255], [255, 220, 140, 255]);
    check(
        BlendMode::Multiply,
        [128, 128, 128, 255],
        [100, 60, 20, 255],
    );
    check(BlendMode::Normal, [9, 9, 9, 255], [9, 9, 9, 255]);
}

#[test]
fn colour_controls_and_filters() {
    let Some(gpu) = gpu() else { return };
    let grey = |p: [u8; 4]| p[0].abs_diff(p[1]) <= 2 && p[1].abs_diff(p[2]) <= 2;
    let mut l = layer(solid(4, 4, [220, 60, 30, 255]), 1);
    l.look.color.saturation = -1.0;
    assert!(
        grey(px(&render(&gpu, (8, 8), Rgba::BLACK, &[l]), 4, 4)),
        "desaturated"
    );

    let mut l = layer(solid(4, 4, [220, 60, 30, 255]), 1);
    l.look.filter = Some(Filter {
        id: "mono".into(),
        intensity: 1.0,
    });
    let p = px(&render(&gpu, (8, 8), Rgba::BLACK, &[l.clone()]), 4, 4);
    assert!(grey(p), "mono filter: {p:?}");
    l.look.filter = Some(Filter {
        id: "mono".into(),
        intensity: 0.0,
    });
    assert!(
        close(
            px(&render(&gpu, (8, 8), Rgba::BLACK, &[l]), 4, 4),
            [220, 60, 30, 255],
            2
        ),
        "zero intensity is a no-op"
    );

    // +1 stop doubles linear light: sRGB 100 (lin 0.127) → 0.254 → sRGB 137.
    let mut l = layer(solid(4, 4, [100, 100, 100, 255]), 1);
    l.look.color.exposure = 1.0;
    assert!(close(
        px(&render(&gpu, (8, 8), Rgba::BLACK, &[l]), 4, 4),
        [137, 137, 137, 255],
        2
    ));
}

#[test]
fn solid_and_gradient_sources() {
    let Some(gpu) = gpu() else { return };
    let solid_layer = Layer {
        source: Source::Solid(Rgba([10, 200, 90, 255])),
        key: 0,
        rotation: 0,
        look: Default::default(),
    };
    assert!(close(
        px(&render(&gpu, (32, 16), Rgba::BLACK, &[solid_layer]), 30, 2),
        [10, 200, 90, 255],
        1
    ));
    let grad = Layer {
        source: Source::Gradient {
            from: Rgba([0, 0, 0, 255]),
            to: Rgba([255, 255, 255, 255]),
            angle: 0.0,
        },
        key: 0,
        rotation: 0,
        look: Default::default(),
    };
    let out = render(&gpu, (100, 10), Rgba::BLACK, &[grad]);
    let (l, m, r) = (px(&out, 1, 5)[0], px(&out, 50, 5)[0], px(&out, 98, 5)[0]);
    assert!(l < 10 && r > 245 && (120..136).contains(&m), "{l} {m} {r}");
}
