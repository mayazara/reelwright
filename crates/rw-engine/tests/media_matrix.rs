//! Probe, thumbnail and proxy every fixture in the media matrix.

mod common;

use rw_media::{Ffmpeg, MediaIo, PROXY_HEIGHT, needs_proxy, thumbnail_time};
use rw_model::MediaKind;
use std::sync::atomic::AtomicBool;

#[test]
fn probe_matches_expectations() {
    let g = common::fixtures();
    let io = Ffmpeg::new();
    let mut failures = Vec::new();
    for f in &g.available {
        let info = match io.probe(&g.path(f.name)) {
            Ok(i) => i,
            Err(e) => {
                failures.push(format!("{}: probe failed: {e}", f.name));
                continue;
            }
        };
        let e = f.expect;
        let mut check = |ok: bool, what: String| {
            if !ok {
                failures.push(format!("{}: {what}", f.name));
            }
        };
        check(
            info.kind == e.kind,
            format!("kind {:?} != {:?}", info.kind, e.kind),
        );
        check(
            info.audio.is_some() == e.audio,
            format!("audio present = {}", info.audio.is_some()),
        );
        match (&info.video, e.video) {
            (Some(v), Some((w, h))) => {
                check(
                    (v.width, v.height) == (w, h),
                    format!("size {}x{}", v.width, v.height),
                );
                check(
                    v.has_alpha == e.alpha,
                    format!("alpha = {} ({})", v.has_alpha, v.pixel_format),
                );
                check(
                    v.rotation == e.rotation,
                    format!("rotation = {}", v.rotation),
                );
                check(
                    v.frame_rate.num > 0 && v.frame_rate.den > 0,
                    "frame rate".into(),
                );
            }
            (None, None) => {}
            (v, _) => check(false, format!("video = {v:?}")),
        }
        if e.kind == MediaKind::Image {
            check(info.duration.ticks() == 0, "image has duration".into());
        } else {
            check(
                info.duration.as_secs_f64() > 0.5,
                format!("duration {}", info.duration),
            );
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn thumbnails_fit_and_respect_rotation() {
    let g = common::fixtures();
    let io = Ffmpeg::new();
    for f in g
        .available
        .iter()
        .filter(|f| f.expect.video.is_some() && f.expect.kind != MediaKind::Audio)
    {
        let path = g.path(f.name);
        let info = io.probe(&path).unwrap();
        let img = io
            .thumbnail(&path, thumbnail_time(&info), 320)
            .unwrap_or_else(|e| panic!("{}: {e}", f.name));
        assert!(img.width.max(img.height) <= 320, "{}: {img:?}", f.name);
        let (dw, dh) = info.video.unwrap().display_size();
        let want = f64::from(dw) / f64::from(dh);
        let got = f64::from(img.width) / f64::from(img.height);
        assert!(
            (want - got).abs() / want < 0.02,
            "{}: aspect {got} vs {want}",
            f.name
        );
        // testsrc2 is colourful: a black or flat thumbnail means a decode bug.
        // A few fixtures are flat on purpose.
        let distinct: std::collections::BTreeSet<_> =
            img.pixels.chunks(4).map(|p| [p[0], p[1], p[2]]).collect();
        let flat_by_design = ["still_alpha.png", "frames_gray_h264.mp4"];
        if !flat_by_design.contains(&f.name) {
            assert!(distinct.len() > 8, "{}: thumbnail looks blank", f.name);
        }
    }
}

#[test]
fn proxies_are_small_intra_frame_files() {
    let g = common::fixtures();
    let io = Ffmpeg::new();
    let out = tempfile::tempdir().unwrap();
    let mut made = 0;
    for f in &g.available {
        let path = g.path(f.name);
        let info = io.probe(&path).unwrap();
        if !needs_proxy(&info) {
            continue;
        }
        let proxy = out.path().join(format!("{}.mkv", f.name));
        io.make_proxy(
            &path,
            &proxy,
            PROXY_HEIGHT,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap_or_else(|e| panic!("{}: {e}", f.name));
        let p = io.probe(&proxy).unwrap();
        let v = p.video.unwrap();
        assert_eq!(v.codec, "mjpeg");
        assert!(v.height <= PROXY_HEIGHT, "{}: {}", f.name, v.height);
        assert!(
            (p.duration.as_secs_f64() - info.duration.as_secs_f64()).abs() < 0.15,
            "{}: proxy {} vs source {}",
            f.name,
            p.duration,
            info.duration
        );
        made += 1;
    }
    assert!(
        made >= 1,
        "expected at least the 4K fixture to need a proxy"
    );
}

#[test]
fn cancelled_proxy_leaves_no_file() {
    let g = common::fixtures();
    let io = Ffmpeg::new();
    let src = g.path("h264_4k30.mp4");
    if !src.exists() {
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let proxy = out.path().join("p.mkv");
    let err = io
        .make_proxy(&src, &proxy, PROXY_HEIGHT, &AtomicBool::new(true), &|_| {})
        .unwrap_err();
    assert!(matches!(err, rw_media::MediaError::Cancelled));
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
}

#[test]
fn garbage_file_is_an_error_not_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("fake.mp4");
    std::fs::write(&p, b"definitely not a video").unwrap();
    assert!(Ffmpeg::new().probe(&p).is_err());
    assert!(
        Ffmpeg::new()
            .probe(&dir.path().join("missing.mp4"))
            .is_err()
    );
}
