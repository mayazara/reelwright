//! M3 acceptance: a property change shows in the preview in under 50 ms.
//!
//! Measures what the UI does when a slider moves while paused: evaluate the
//! frame (decoders already warm), composite at preview size, and wait for
//! the GPU (read back). The 50 ms target is enforced with `RW_PERF=1`
//! (release build); otherwise the timing is printed.

mod common;

use rw_cache::ProjectCache;
use rw_engine::frames::FrameSource;
use rw_engine::import::probe_file;
use rw_media::Ffmpeg;
use rw_model::{Canvas, Filter, Item, Layout, Project, Time, Track, TrackKind};
use rw_render::{Compositor, Gpu};
use std::time::{Duration, Instant};

#[test]
fn property_change_reaches_preview_quickly() {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) if std::env::var("RW_REQUIRE_GPU").is_ok_and(|v| v == "1") => panic!("{e}"),
        Err(_) => return,
    };
    let g = common::fixtures();
    let clip = g.path("h264_1080p30_aac.mp4");
    if !clip.exists() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let m = probe_file(&Ffmpeg::new(), &clip).unwrap();
    let mut p = Project::new("p", Canvas::default());
    let mut tracks = vec![
        Track::new_main(),
        Track::new(TrackKind::Video),
        Track::new(TrackKind::Video),
    ];
    for (i, t) in tracks.iter_mut().enumerate() {
        let mut item = Item::new(&m, Time::ZERO);
        if i == 1 {
            item.look.transform = Layout::TopRight.apply(item.look.transform);
        }
        if i == 2 {
            item.look.transform = Layout::BottomLeft.apply(item.look.transform);
            item.look.filter = Some(Filter {
                id: "ember".into(),
                intensity: 1.0,
            });
        }
        t.items.push(item);
    }
    p.media.push(m);
    p.tracks = tracks;

    let size = (960, 540);
    let t = Time::from_secs(1);
    let mut source = FrameSource::new(ProjectCache::for_project(tmp.path()), true);
    let mut comp = Compositor::new(&gpu, size);
    // Warm up: open decoders, upload textures, compile pipelines.
    for _ in 0..3 {
        let layers = source.layers_at(&p, t, size);
        comp.render(p.canvas.background, &layers);
        comp.read_back().unwrap();
    }
    let target = p.tracks[0].items[0].id;
    let mut worst = Duration::ZERO;
    let mut total = Duration::ZERO;
    let n = 30;
    for i in 0..n {
        // As if the saturation slider moved one step.
        for item in p.tracks.iter_mut().flat_map(|t| t.items.iter_mut()) {
            if item.id == target {
                item.look.color.saturation = (i as f32 / n as f32) * 2.0 - 1.0;
            }
        }
        let start = Instant::now();
        let layers = source.layers_at(&p, t, size);
        comp.render(p.canvas.background, &layers);
        comp.read_back().unwrap();
        let d = start.elapsed();
        worst = worst.max(d);
        total += d;
    }
    eprintln!(
        "property change → preview: mean {:?}, worst {:?}",
        total / n,
        worst
    );
    if std::env::var("RW_PERF").is_ok_and(|v| v == "1") {
        assert!(worst < Duration::from_millis(50), "worst {worst:?}");
    }
}
