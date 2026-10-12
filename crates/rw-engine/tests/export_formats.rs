//! M5: every export format writes a file that reads back as asked.

mod common;

use rw_cache::ProjectCache;
use rw_engine::export::{ExportFormat, ExportSettings, Quality, Resolution, bit_rate, export};
use rw_engine::import::probe_file;
use rw_media::{Ffmpeg, MediaIo, VideoCodec};
use rw_model::{
    Canvas, FrameRate, Generated, Item, MediaKind, Project, Rgba, Time, Track, TrackKind,
};
use rw_render::Gpu;
use std::sync::atomic::AtomicBool;

fn gpu() -> Option<Gpu> {
    match Gpu::headless() {
        Ok(g) => Some(g),
        Err(e) if std::env::var("RW_REQUIRE_GPU").is_ok_and(|v| v == "1") => panic!("{e}"),
        Err(e) => {
            eprintln!("skipping: {e}");
            None
        }
    }
}

/// Two seconds: a background, a title over it, and a tone.
fn project() -> Project {
    let g = common::fixtures();
    let tone = probe_file(&Ffmpeg::new(), &g.path("tone.wav")).unwrap();
    let mut p = Project::new("formats", Canvas::default());
    let mut main = Track::new_main();
    let mut bg = Item::generated(
        Generated::Solid {
            color: Rgba([30, 90, 120, 255]),
        },
        Time::ZERO,
    );
    bg.duration = Time::from_secs(2);
    main.items.push(bg);
    let mut text = Track::new(TrackKind::Text);
    let mut title = Item::text_item(rw_model::TextSpec::new("Hello"), Time::ZERO);
    title.duration = Time::from_secs(2);
    text.items.push(title);
    let mut audio = Track::new(TrackKind::Audio);
    let mut t = Item::new(&tone, Time::ZERO);
    t.duration = Time::from_secs(2);
    audio.items.push(t);
    p.media.push(tone);
    p.tracks = vec![main, text, audio];
    p
}

fn settings(p: &Project, path: std::path::PathBuf, format: ExportFormat) -> ExportSettings {
    ExportSettings {
        format,
        ..ExportSettings::for_project(p, &path)
    }
}

#[test]
fn mp4_at_a_lower_resolution_and_frame_rate() {
    let Some(gpu) = gpu() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let p = project();
    let size = Resolution::P480.size_for((p.canvas.width, p.canvas.height));
    let fps = FrameRate::new(24, 1);
    let s = ExportSettings {
        width: size.0,
        height: size.1,
        frame_rate: Some(fps),
        video_bit_rate: bit_rate(size, fps, Quality::Smaller, VideoCodec::H264),
        ..settings(
            &p,
            tmp.path().join("small.mp4"),
            ExportFormat::Video {
                codec: VideoCodec::H264,
            },
        )
    };
    let cache = ProjectCache::for_project(tmp.path());
    let r = export(&p, &cache, &gpu, &s, &AtomicBool::new(false), &|_| {}).unwrap();
    assert_eq!(r.frames, 48);
    assert!(r.bytes > 1000);
    let info = Ffmpeg::new().probe(&r.path).unwrap();
    let v = info.video.unwrap();
    assert_eq!((v.width, v.height), (854, 480));
    assert!((v.frame_rate.as_f64() - 24.0).abs() < 0.01);
    assert!(info.audio.is_some());
}

#[test]
fn every_working_codec_writes_video_with_sound() {
    let Some(gpu) = gpu() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let p = project();
    let cache = ProjectCache::for_project(tmp.path());
    let mut done = Vec::new();
    for codec in VideoCodec::ALL {
        let encoders = rw_media::working_encoders(codec);
        let Some(encoder) = encoders.first() else {
            eprintln!("{codec:?}: no working encoder here");
            continue;
        };
        let path = tmp.path().join(format!("out.{}", codec.extension()));
        let s = ExportSettings {
            encoder: Some(encoder.clone()),
            ..settings(&p, path, ExportFormat::Video { codec })
        };
        let r = export(&p, &cache, &gpu, &s, &AtomicBool::new(false), &|_| {})
            .unwrap_or_else(|e| panic!("{codec:?} with {encoder}: {e}"));
        let info = Ffmpeg::new().probe(&r.path).unwrap();
        assert_eq!(info.kind, MediaKind::Video, "{codec:?}");
        assert!(info.audio.is_some(), "{codec:?} has sound");
        assert!(
            (info.duration.as_secs_f64() - 2.0).abs() < 0.1,
            "{codec:?} lasts {}",
            info.duration
        );
        done.push(format!("{codec:?}={encoder}"));
    }
    eprintln!("exported with: {}", done.join(", "));
    assert!(
        done.iter().any(|d| d.starts_with("H264")),
        "H.264 always works"
    );
}

#[test]
fn gif_and_mp3() {
    let Some(gpu) = gpu() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let p = project();
    let cache = ProjectCache::for_project(tmp.path());

    let size = Resolution::P480.size_for((p.canvas.width, p.canvas.height));
    let s = ExportSettings {
        width: size.0,
        height: size.1,
        ..settings(&p, tmp.path().join("a.gif"), ExportFormat::Gif { fps: 15 })
    };
    let r = export(&p, &cache, &gpu, &s, &AtomicBool::new(false), &|_| {}).unwrap();
    assert_eq!(r.frames, 30);
    let info = Ffmpeg::new().probe(&r.path).unwrap();
    let v = info.video.expect("gif reads back as pictures");
    assert_eq!((v.width, v.height), (854, 480));
    assert!(
        (info.duration.as_secs_f64() - 2.0).abs() < 0.15,
        "gif lasts {}",
        info.duration
    );

    let s = settings(&p, tmp.path().join("a.mp3"), ExportFormat::Mp3);
    let r = export(&p, &cache, &gpu, &s, &AtomicBool::new(false), &|_| {}).unwrap();
    let info = Ffmpeg::new().probe(&r.path).unwrap();
    assert_eq!(info.kind, MediaKind::Audio);
    assert!(info.video.is_none());
    assert!(
        (info.duration.as_secs_f64() - 2.0).abs() < 0.1,
        "{}",
        info.duration
    );
}

#[test]
fn range_export_and_cancel() {
    let Some(gpu) = gpu() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let p = project();
    let cache = ProjectCache::for_project(tmp.path());
    let s = ExportSettings {
        range: Some((Time::from_millis(500), Time::from_millis(1500))),
        ..settings(
            &p,
            tmp.path().join("part.mp4"),
            ExportFormat::Video {
                codec: VideoCodec::H264,
            },
        )
    };
    let r = export(&p, &cache, &gpu, &s, &AtomicBool::new(false), &|_| {}).unwrap();
    assert_eq!(r.frames, 30);
    assert_eq!(r.duration, Time::from_secs(1));

    let out = tmp.path().join("cancelled");
    std::fs::create_dir_all(&out).unwrap();
    for format in [ExportFormat::Gif { fps: 10 }, ExportFormat::Mp3] {
        let s = settings(&p, out.join(format!("x.{}", format.extension())), format);
        let e = export(&p, &cache, &gpu, &s, &AtomicBool::new(true), &|_| {}).unwrap_err();
        assert!(matches!(e, rw_engine::export::ExportError::Cancelled));
    }
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0, "nothing left");
}
