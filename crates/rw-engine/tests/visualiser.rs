//! M6: audio visualisers move with the project's sound, in every style.

mod common;

use rw_cache::ProjectCache;
use rw_engine::frames::FrameSource;
use rw_engine::import::probe_file;
use rw_media::Ffmpeg;
use rw_model::{
    Canvas, Generated, Item, Project, Rgba, Time, Track, TrackKind, VisualiserSpec,
    VisualiserStyle,
};
use rw_render::{Compositor, Gpu};
use std::path::{Path, PathBuf};

const SIZE: (u32, u32) = (480, 270);

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// A dark background, a visualiser over it, and (with `sound`) a 440 Hz
/// tone for the first two seconds.
fn project(style: VisualiserStyle, sound: bool) -> Project {
    let mut p = Project::new("vis", Canvas::custom(SIZE.0, SIZE.1));
    let mut main = Track::new_main();
    let mut bg = Item::generated(
        Generated::Solid {
            color: Rgba([20, 24, 34, 255]),
        },
        Time::ZERO,
    );
    bg.duration = Time::from_secs(4);
    main.items.push(bg);
    let mut over = Track::new(TrackKind::Video);
    let mut v = Item::visualiser_item(VisualiserSpec::new(style), Time::ZERO);
    v.duration = Time::from_secs(4);
    v.look.transform.size = [2.0, 2.0];
    over.items.push(v);
    p.tracks = vec![main, over];
    if sound {
        let tone = probe_file(&Ffmpeg::new(), &common::fixtures().path("tone.wav")).unwrap();
        let mut a = Track::new(TrackKind::Audio);
        let mut i = Item::new(&tone, Time::ZERO);
        i.duration = Time::from_secs(2);
        a.items.push(i);
        p.media.push(tone);
        p.tracks.push(a);
    }
    p
}

fn render(gpu: &Gpu, p: &Project, at: Time) -> rw_media::RgbaImage {
    let tmp = tempfile::tempdir().unwrap();
    let mut source = FrameSource::new(ProjectCache::for_project(tmp.path()), false);
    let layers = source.layers_at(p, at, SIZE);
    let mut c = Compositor::new(gpu, SIZE);
    c.render(p.canvas.background, &layers);
    c.read_back().unwrap()
}

fn lit(img: &rw_media::RgbaImage) -> usize {
    // The amber of the visualiser, not the dark background.
    img.pixels.chunks(4).filter(|p| p[0] > 150 && p[2] < 120).count()
}

#[test]
fn reacts_to_sound_and_matches_goldens() {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) if std::env::var("RW_REQUIRE_GPU").is_ok_and(|v| v == "1") => panic!("{e}"),
        Err(e) => {
            eprintln!("skipping: {e}");
            return;
        }
    };
    let mut failures = Vec::new();
    for style in VisualiserStyle::ALL {
        let p = project(style, true);
        let loud = render(&gpu, &p, Time::from_secs(1));
        let quiet = render(&gpu, &p, Time::from_secs(3));
        assert!(
            lit(&loud) > lit(&quiet) + 50,
            "{style:?}: {} lit with sound, {} without",
            lit(&loud),
            lit(&quiet)
        );
        let name = format!("visualiser-{style:?}").to_lowercase();
        if let Err(e) = rw_render::golden::compare(&golden_dir(), &name, &loud) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
