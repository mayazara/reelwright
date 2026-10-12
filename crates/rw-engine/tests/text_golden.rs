//! Golden frames for every text preset and motion, every shape and some
//! stickers, drawn through the engine and the compositor like preview and
//! export. Presets use bundled fonts only, so
//! frames match on every computer.
//!
//! Regenerate with
//! `RW_GPU_FALLBACK=1 RW_UPDATE_GOLDEN=1 cargo test -p rw-engine --test text_golden`.

use rw_cache::ProjectCache;
use rw_engine::frames::FrameSource;
use rw_model::{Canvas, Generated, Item, Project, Rgba, Time, Track, TrackKind};
use rw_render::{Compositor, Gpu};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SIZE: (u32, u32) = (480, 270);

fn gpu() -> Option<Gpu> {
    static GPU: std::sync::OnceLock<Option<Gpu>> = std::sync::OnceLock::new();
    GPU.get_or_init(|| match Gpu::headless() {
        Ok(g) => Some(g),
        Err(e) if std::env::var("RW_REQUIRE_GPU").is_ok_and(|v| v == "1") => panic!("{e}"),
        Err(e) => {
            eprintln!("skipping: {e}");
            None
        }
    })
    .clone()
}

/// One GPU test at a time (software drivers dislike parallel devices).
static SERIAL: Mutex<()> = Mutex::new(());

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// A gradient background with `text` over it on a text track, drawn at `at`.
fn frame(gpu: &Gpu, text: Item, at: Time) -> rw_media::RgbaImage {
    frame_on(gpu, text, at, TrackKind::Text)
}

/// A gradient background with `item` over it on a track of `kind`.
fn frame_on(gpu: &Gpu, item: Item, at: Time, kind: TrackKind) -> rw_media::RgbaImage {
    let mut p = Project::new("text", Canvas::custom(SIZE.0, SIZE.1));
    let mut main = Track::new_main();
    let mut bg = Item::generated(
        Generated::Gradient {
            from: Rgba([40, 60, 110, 255]),
            to: Rgba([150, 80, 60, 255]),
            angle: 30.0,
        },
        Time::ZERO,
    );
    bg.duration = Time::from_secs(20);
    main.items.push(bg);
    let mut track = Track::new(kind);
    track.items.push(item);
    p.tracks = vec![main, track];
    let tmp = tempfile::tempdir().unwrap();
    let mut source = FrameSource::new(ProjectCache::for_project(tmp.path()), false);
    let layers = source.layers_at(&p, at, SIZE);
    let mut c = Compositor::new(gpu, SIZE);
    c.render(p.canvas.background, &layers);
    c.read_back().unwrap()
}

fn preset_item(p: &rw_text::presets::Preset) -> Item {
    let mut item = Item::text_item(p.spec.clone(), Time::ZERO);
    item.duration = p.duration;
    item.look.transform.center = p.center;
    item
}

/// When to sample each preset: at rest, or mid-motion where the motion is
/// the point.
fn sample_time(name: &str, duration: Time) -> Time {
    match name {
        "Typewriter" | "Word by word" => Time::from_millis(750),
        "Countdown" => Time::from_millis(2300),
        "End credits" => Time(duration.ticks() / 2),
        _ => Time::from_millis(2500),
    }
}

#[test]
fn every_text_preset() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(gpu) = gpu() else { return };
    let mut failures = Vec::new();
    for p in rw_text::presets::all() {
        let at = sample_time(p.name, p.duration);
        let img = frame(&gpu, preset_item(&p), at);
        let name = format!("text-{}", p.name.to_lowercase().replace(' ', "-"));
        if let Err(e) = rw_render::golden::compare(&golden_dir(), &name, &img) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Entrances and exits, mid-way, on one preset.
#[test]
fn every_text_motion() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(gpu) = gpu() else { return };
    let base = rw_text::presets::plain();
    let mut failures = Vec::new();
    for motion in rw_model::TextMotion::ALL {
        let mut item = preset_item(&base);
        let mut spec = item.text().unwrap().clone();
        spec.text = "Moving words".into();
        spec.enter = rw_model::TextAnimation {
            motion,
            duration: Time::from_secs(1),
        };
        item.kind = rw_model::ItemKind::Text {
            text: Box::new(spec),
        };
        let img = frame(&gpu, item, Time::from_millis(400));
        let name = format!("text-motion-{motion:?}").to_lowercase();
        if let Err(e) = rw_render::golden::compare(&golden_dir(), &name, &img) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Every shape, and a few stickers, placed off-centre and enlarged.
#[test]
fn shapes_and_stickers() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(gpu) = gpu() else { return };
    let mut failures = Vec::new();
    let mut cases: Vec<(String, Item)> = rw_model::ShapeKind::ALL
        .iter()
        .map(|k| {
            (
                format!("shape-{k:?}").to_lowercase(),
                Item::shape_item(rw_model::ShapeSpec::new(*k), Time::ZERO),
            )
        })
        .collect();
    for id in ["1F600", "1F389", "2764", "1F44D"] {
        let s = rw_model::StickerRef {
            pack: "openmoji".into(),
            id: id.into(),
        };
        cases.push((
            format!("sticker-{id}").to_lowercase(),
            Item::sticker_item(s, Time::ZERO),
        ));
    }
    for (name, mut item) in cases {
        item.look.transform.center = [0.4, 0.45];
        item.look.transform.size = [1.3, 1.3];
        let img = frame_on(&gpu, item, Time::from_secs(1), TrackKind::Video);
        if let Err(e) = rw_render::golden::compare(&golden_dir(), &name, &img) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
