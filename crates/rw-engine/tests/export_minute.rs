//! M5 acceptance: a 1080p export completes with every H.264 encoder that
//! works here, hardware and software, and on the software-only path.
//!
//! The project is `RW_EXPORT_SECONDS` long (default 4; the acceptance run
//! uses 60): 1080p30 video clips with a title over them and sound.

mod common;

use rw_cache::ProjectCache;
use rw_engine::export::{ExportFormat, ExportSettings, export};
use rw_engine::import::probe_file;
use rw_media::{Ffmpeg, MediaIo, VideoCodec};
use rw_model::{Canvas, Item, Project, TextSpec, Time, Track, TrackKind};
use rw_render::Gpu;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

/// Software H.264 encoders (no GPU or OS media service involved).
const SOFTWARE: &[&str] = &["libopenh264", "libx264", "mpeg4"];

fn project(secs: i64) -> Option<Project> {
    let g = common::fixtures();
    let clip = g.path("h264_1080p30_aac.mp4");
    if !clip.exists() {
        return None;
    }
    let io = Ffmpeg::new();
    let v = probe_file(&io, &clip).unwrap();
    let mut p = Project::new("minute", Canvas::default());
    let mut main = Track::new_main();
    let mut at = Time::ZERO;
    let total = Time::from_secs(secs);
    while at < total {
        let mut i = Item::new(&v, at);
        i.duration = i.duration.min(total - at);
        at += i.duration;
        main.items.push(i);
    }
    let mut text = Track::new(TrackKind::Text);
    let mut title = Item::text_item(TextSpec::new("One minute in 1080p"), Time::ZERO);
    title.duration = total;
    text.items.push(title);
    p.media.push(v);
    p.tracks = vec![main, text];
    Some(p)
}

#[test]
fn every_h264_encoder_exports_1080p() {
    let secs: i64 = std::env::var("RW_EXPORT_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let Some(p) = project(secs) else {
        eprintln!("skipping: no 1080p fixture");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let cache = ProjectCache::for_project(tmp.path());
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) if std::env::var("RW_REQUIRE_GPU").is_ok_and(|v| v == "1") => panic!("{e}"),
        Err(e) => {
            eprintln!("skipping: {e}");
            return;
        }
    };
    let encoders = rw_media::working_encoders(VideoCodec::H264);
    assert!(
        encoders.iter().any(|e| SOFTWARE.contains(&e.as_str())),
        "a software encoder is always available: {encoders:?}"
    );
    let mut report = Vec::new();
    for encoder in &encoders {
        let path = tmp.path().join(format!("{encoder}.mp4"));
        let s = ExportSettings {
            encoder: Some(encoder.clone()),
            format: ExportFormat::Video {
                codec: VideoCodec::H264,
            },
            ..ExportSettings::for_project(&p, &path)
        };
        let started = Instant::now();
        let r = export(&p, &cache, &gpu, &s, &AtomicBool::new(false), &|_| {})
            .unwrap_or_else(|e| panic!("{encoder}: {e}"));
        let took = started.elapsed();
        let info = Ffmpeg::new().probe(&r.path).unwrap();
        let v = info.video.expect("video stream");
        assert_eq!((v.width, v.height), (1920, 1080), "{encoder}");
        assert!(
            (info.duration.as_secs_f64() - secs as f64).abs() < 0.1,
            "{encoder}: {}",
            info.duration
        );
        assert_eq!(r.frames, secs * 30);
        let kind = if SOFTWARE.contains(&encoder.as_str()) {
            "software"
        } else {
            "hardware/OS"
        };
        report.push(format!(
            "{encoder} ({kind}): {secs} s of 1080p30 in {:.1} s, {:.1} MB",
            took.as_secs_f64(),
            r.bytes as f64 / 1e6
        ));
    }
    eprintln!("GPU: {}\n{}", gpu.adapter_name, report.join("\n"));
}
