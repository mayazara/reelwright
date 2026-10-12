//! Golden-frame tests: every filter, blend mode, colour control, effect and
//! transition and a set of transforms, rendered from a synthetic scene and compared with stored PNGs.
//!
//! References live in `tests/golden/`. Regenerate them (on the software
//! renderer CI uses) with:
//! `RW_GPU_FALLBACK=1 RW_UPDATE_GOLDEN=1 cargo test -p rw-render --test golden`
//! and review the diff before committing.

use rw_media::RgbaImage;
use rw_model::{
    BlendMode, ColorControl, Crop, EffectKind, Filter, Fit, Layout, Look, Rgba, Transform,
    TransitionKind,
};
use rw_render::{Compositor, Gpu, Layer, LayerTransition, Source};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SIZE: (u32, u32) = (128, 72);

fn gpu() -> Option<Gpu> {
    // One device for the whole binary: several software-renderer devices
    // compiling pipelines at once have crashed the driver.
    static GPU: std::sync::OnceLock<Option<Gpu>> = std::sync::OnceLock::new();
    GPU.get_or_init(|| match Gpu::headless() {
        Ok(g) => Some(g),
        Err(e) if std::env::var("RW_REQUIRE_GPU").is_ok_and(|v| v == "1") => panic!("{e}"),
        Err(e) => {
            eprintln!("skipping golden tests: {e}");
            None
        }
    })
    .clone()
}

/// A picture with hue sweep, a grey ramp, skin-like tones and a soft edge:
/// enough variety to show what any adjustment does.
fn base_picture() -> Arc<RgbaImage> {
    let (w, h) = (96u32, 54u32);
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let fx = x as f32 / (w - 1) as f32;
            let fy = y as f32 / (h - 1) as f32;
            let c = if fy < 0.4 {
                // Hue sweep.
                let hsv = rw_hsv(fx * 360.0, 0.85, 0.9);
                [hsv[0], hsv[1], hsv[2]]
            } else if fy < 0.6 {
                let v = (fx * 255.0) as u8;
                [v, v, v]
            } else {
                // Warm skin-like tones fading to a cool shadow.
                let t = fx;
                [
                    (224.0 - 150.0 * t) as u8,
                    (172.0 - 100.0 * t) as u8,
                    (140.0 - 40.0 * t) as u8,
                ]
            };
            px.extend([c[0], c[1], c[2], 255]);
        }
    }
    Arc::new(RgbaImage::new(w, h, px))
}

fn rw_hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    ]
}

fn base_layer() -> Layer {
    Layer::image(base_picture(), 1, 0)
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn compare(name: &str, got: &RgbaImage) -> Result<(), String> {
    rw_render::golden::compare(&golden_dir(), name, got)
}

fn render(gpu: &Gpu, layers: &[Layer]) -> RgbaImage {
    let mut c = Compositor::new(gpu, SIZE);
    c.render(Rgba([24, 26, 30, 255]), layers);
    c.read_back().unwrap()
}

/// One GPU test at a time (see `gpu`).
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn check_all(cases: Vec<(String, Vec<Layer>)>) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(gpu) = gpu() else { return };
    let failures: Vec<String> = cases
        .into_iter()
        .filter_map(|(name, layers)| compare(&name, &render(&gpu, &layers)).err())
        .collect();
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn every_filter() {
    let cases = rw_fx::filters()
        .iter()
        .map(|f| {
            let mut l = base_layer();
            l.look.filter = Some(Filter {
                id: f.id.into(),
                intensity: 1.0,
            });
            (format!("filter-{}", f.id), vec![l])
        })
        .chain(std::iter::once({
            let mut l = base_layer();
            l.look.filter = Some(Filter {
                id: "vivid".into(),
                intensity: 0.5,
            });
            ("filter-vivid-half".to_owned(), vec![l])
        }))
        .collect();
    check_all(cases);
}

#[test]
fn every_blend_mode() {
    // A vertical gradient strip over the base picture, partly transparent
    // at the edges, in each blend mode.
    let cases = BlendMode::ALL
        .iter()
        .map(|mode| {
            let mut top = Layer {
                source: Source::Gradient {
                    from: Rgba([30, 60, 200, 255]),
                    to: Rgba([250, 230, 120, 255]),
                    angle: 90.0,
                },
                key: 0,
                rotation: 0,
                look: Look::default(),
                clip_time: 0.0,
                clip_progress: 0.0,
                transition: None,
            };
            top.look.blend = *mode;
            top.look.transform = Transform {
                center: [0.5, 0.5],
                size: [0.6, 1.0],
                ..Transform::default()
            };
            (
                format!("blend-{mode:?}").to_lowercase(),
                vec![base_layer(), top],
            )
        })
        .collect();
    check_all(cases);
}

#[test]
fn every_colour_control() {
    let mut cases = Vec::new();
    for c in ColorControl::ALL {
        for (suffix, at_end) in [("min", false), ("max", true)] {
            let r = c.range();
            let v = if at_end { *r.end() } else { *r.start() } * 0.75;
            let mut l = base_layer();
            l.look.color.set(c, v);
            cases.push((
                format!("colour-{}-{suffix}", c.label().to_lowercase()),
                vec![l],
            ));
        }
    }
    check_all(cases);
}

#[test]
fn transforms_crops_and_opacity() {
    let with = |f: &dyn Fn(&mut Look)| {
        let mut l = base_layer();
        f(&mut l.look);
        vec![l]
    };
    let cases = vec![
        ("transform-fit".to_owned(), with(&|_| {})),
        (
            "transform-fill".to_owned(),
            with(&|l| l.transform.fit = Fit::Cover),
        ),
        (
            "transform-rotate-30".to_owned(),
            with(&|l| l.transform.rotation = 30.0),
        ),
        (
            "transform-flip-both".to_owned(),
            with(&|l| {
                l.transform.flip_h = true;
                l.transform.flip_v = true;
            }),
        ),
        (
            "transform-pip".to_owned(),
            with(&|l| l.transform = Layout::TopRight.apply(l.transform)),
        ),
        (
            "transform-split".to_owned(),
            with(&|l| l.transform = Layout::SplitLeft.apply(l.transform)),
        ),
        (
            "crop-square".to_owned(),
            with(&|l| l.crop = Crop::to_ratio((96, 54), 1.0)),
        ),
        (
            "crop-corner".to_owned(),
            with(&|l| {
                l.crop = Crop {
                    left: 0.5,
                    top: 0.4,
                    ..Crop::default()
                }
            }),
        ),
        ("opacity-40".to_owned(), with(&|l| l.opacity = 0.4)),
    ];
    check_all(cases);
}

/// A second picture for transitions: the base picture turned upside down
/// and tinted, so either clip is easy to tell apart.
fn other_layer() -> Layer {
    let mut l = base_layer();
    l.key = 2;
    l.look.transform.flip_v = true;
    l.look.color.temperature = -0.8;
    l
}

fn with_effect(kind: EffectKind) -> Layer {
    let mut l = base_layer();
    l.look.effects = vec![rw_fx::effects::new_effect(kind)];
    l.clip_time = 1.25;
    l.clip_progress = 0.5;
    l
}

#[test]
fn every_effect() {
    let cases = EffectKind::ALL
        .iter()
        .map(|kind| {
            let mut l = with_effect(*kind);
            if *kind == EffectKind::BackgroundBlurFill {
                // A tall picture in a wide frame.
                l.look.transform.size = [0.35, 1.0];
                l.look.transform.fit = Fit::Cover;
            }
            (format!("effect-{kind:?}").to_lowercase(), vec![l])
        })
        .chain(std::iter::once({
            // Effects stack in order.
            let mut l = with_effect(EffectKind::Pixelate);
            l.look
                .effects
                .push(rw_fx::effects::new_effect(EffectKind::Vignette));
            ("effect-stacked".to_owned(), vec![l])
        }))
        .collect();
    check_all(cases);
}

fn transition(kind: TransitionKind, progress: f32) -> Layer {
    let mut to = other_layer();
    to.transition = Some(Box::new(LayerTransition {
        from: base_layer(),
        kind,
        progress,
    }));
    to
}

#[test]
fn every_transition() {
    let cases = TransitionKind::ALL
        .iter()
        .map(|kind| {
            (
                format!("transition-{kind:?}").to_lowercase(),
                vec![transition(*kind, 0.4)],
            )
        })
        .collect();
    check_all(cases);
}

fn max_diff(a: &RgbaImage, b: &RgbaImage) -> u8 {
    a.pixels
        .iter()
        .zip(&b.pixels)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

/// Every transition starts on the outgoing clip and ends on the incoming
/// one, so cuts into and out of the window don't jump.
#[test]
fn transitions_start_and_end_on_their_clips() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(gpu) = gpu() else { return };
    let from = render(&gpu, &[base_layer()]);
    let to = render(&gpu, &[other_layer()]);
    let mut bad = Vec::new();
    for kind in TransitionKind::ALL {
        let start = max_diff(&render(&gpu, &[transition(kind, 0.0)]), &from);
        let end = max_diff(&render(&gpu, &[transition(kind, 1.0)]), &to);
        if start > 2 || end > 2 {
            bad.push(format!("{kind:?}: start off by {start}, end off by {end}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Drawing a clip on its own and merging it gives the same picture as
/// drawing it straight onto the canvas (blend mode, opacity, placement).
#[test]
fn drawing_alone_then_merging_matches_direct_drawing() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(gpu) = gpu() else { return };
    for mode in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Screen] {
        let mut top = other_layer();
        top.look.blend = mode;
        top.look.opacity = 0.6;
        top.look.transform = Layout::TopRight.apply(top.look.transform);
        let direct = render(&gpu, &[base_layer(), top.clone()]);
        // A barely-there effect forces the separate path.
        top.look.effects = vec![rw_model::Effect {
            kind: EffectKind::Invert,
            intensity: 1e-4,
            params: Vec::new(),
        }];
        let merged = render(&gpu, &[base_layer(), top]);
        let d = max_diff(&direct, &merged);
        assert!(d <= 2, "{mode:?}: differs by {d}");
    }
}
