//! M2 acceptance: the exported cut list matches the timeline frame-accurately.

mod common;

use rw_cache::ProjectCache;
use rw_edit::History;
use rw_edit::timeline::{Placement, TimelineEdit, TimelineOp};
use rw_engine::export::{ExportSettings, export};
use rw_engine::import::probe_file;
use rw_media::decode::{CHANNELS, SAMPLE_RATE};
use rw_media::{AudioReader, Ffmpeg, MediaIo, VideoReader};
use rw_model::{Canvas, FrameRate, Item, Project, Time, TrackKind};
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

fn gray_index(img: &rw_media::RgbaImage, x: u32, y: u32) -> i64 {
    let i = ((y * img.width + x) * 4) as usize;
    (f64::from(img.pixels[i]) / 4.0).round() as i64
}

fn is_gray(img: &rw_media::RgbaImage, x: u32, y: u32) -> bool {
    let i = ((y * img.width + x) * 4) as usize;
    let p = &img.pixels[i..i + 3];
    p[0].abs_diff(p[1]) < 12 && p[1].abs_diff(p[2]) < 12
}

#[test]
fn exported_cuts_match_timeline() {
    let Some(gpu) = gpu() else { return };
    let g = common::fixtures();
    let gray = g.path("frames_gray_h264.mp4");
    if !gray.exists() {
        eprintln!("skipping: no H.264 encoder for the fixture");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let io = Ffmpeg::new();
    let fps = FrameRate::FPS_30;
    let f = |n: i64| fps.frame_to_time(n);

    let mut p = Project::new("golden", Canvas::custom(320, 240));
    let gray_m = probe_file(&io, &gray).unwrap();
    let still = probe_file(&io, &g.path("still.png")).unwrap();
    let beep = probe_file(&io, &g.path("beep_aac.m4a")).unwrap();
    p.media = vec![gray_m.clone(), still.clone(), beep.clone()];
    p.tracks.push(rw_model::Track::new_main());
    let main = p.tracks[0].id;
    let mut h = History::new();
    let mut run = |p: &mut Project, op| h.execute(p, Box::new(TimelineEdit::new(op))).unwrap();

    // Main track: source frames 10..25, then 40..50, then 0..5.
    for (from, to) in [(10, 25), (40, 50), (0, 5)] {
        let mut item = Item::new(&gray_m, Time::ZERO);
        item.source_in = f(from);
        item.duration = f(to) - f(from);
        run(
            &mut p,
            TimelineOp::Insert {
                placement: Placement::Track(main),
                at: Time::from_secs(99),
                items: vec![item],
            },
        );
    }
    // Overlay: the 16:9 still over frames 6..12. It letterboxes inside the
    // 4:3 canvas, so the top rows still show the main track.
    let mut over = Item::new(&still, Time::ZERO);
    over.duration = f(12) - f(6);
    run(
        &mut p,
        TimelineOp::Insert {
            placement: Placement::NewTrack {
                kind: TrackKind::Video,
                index: 1,
            },
            at: f(6),
            items: vec![over],
        },
    );
    // Audio: beep (at 1.0 s in the file) placed so it sounds at 0.5 s.
    let mut b = Item::new(&beep, Time::ZERO);
    b.source_in = Time::from_millis(500);
    b.duration = Time::from_millis(900);
    run(
        &mut p,
        TimelineOp::Insert {
            placement: Placement::NewTrack {
                kind: TrackKind::Audio,
                index: 9,
            },
            at: Time::ZERO,
            items: vec![b],
        },
    );
    assert_eq!(p.duration(), f(30));

    let cache = ProjectCache::for_project(tmp.path());
    let settings = ExportSettings::for_project(&p, &tmp.path().join("golden.mp4"));
    let report = export(
        &p,
        &cache,
        &gpu,
        &settings,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    assert_eq!(report.frames, 30);

    let info = io.probe(&report.path).unwrap();
    assert!(
        (info.duration.as_secs_f64() - 1.0).abs() < 0.05,
        "duration {}",
        info.duration
    );
    let mut r = VideoReader::open(&report.path, None, false).unwrap();
    let mut mismatches = Vec::new();
    for k in 0..30i64 {
        let want = match k {
            0..15 => 10 + k,
            15..25 => 40 + (k - 15),
            _ => k - 25,
        };
        let frame = r.frame_at(f(k) + Time::from_millis(2)).unwrap();
        let img = &frame.image;
        // Top rows are main-track picture in every frame.
        let got = gray_index(img, 160, 4);
        if got != want {
            mismatches.push(format!(
                "frame {k}: top shows source frame {got}, want {want}"
            ));
        }
        let overlay_visible = !is_gray(img, 160, 120);
        if overlay_visible != (6..12).contains(&k) {
            mismatches.push(format!("frame {k}: overlay visible = {overlay_visible}"));
        }
    }
    assert!(mismatches.is_empty(), "\n{}", mismatches.join("\n"));

    let mut a = AudioReader::open(&report.path).unwrap();
    let mut buf = vec![0.0f32; SAMPLE_RATE as usize * CHANNELS];
    let n = a.read(&mut buf).unwrap();
    let onset = buf[..n]
        .chunks(CHANNELS)
        .position(|s| s[0].abs() > 0.2)
        .expect("beep present");
    let want = SAMPLE_RATE as usize / 2;
    assert!(
        onset.abs_diff(want) <= 48,
        "audio onset at sample {onset}, want {want}"
    );
}

#[test]
fn cancelled_export_leaves_nothing() {
    let Some(gpu) = gpu() else { return };
    let g = common::fixtures();
    let io = Ffmpeg::new();
    let tmp = tempfile::tempdir().unwrap();
    let m = probe_file(&io, &g.path("still.png")).unwrap();
    let mut p = Project::new("c", Canvas::custom(64, 64));
    let mut t = rw_model::Track::new_main();
    t.items.push(Item::new(&m, Time::ZERO));
    p.media.push(m);
    p.tracks.push(t);
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let settings = ExportSettings::for_project(&p, &out.join("x.mp4"));
    let err = export(
        &p,
        &ProjectCache::for_project(tmp.path()),
        &gpu,
        &settings,
        &AtomicBool::new(true),
        &|_| {},
    )
    .unwrap_err();
    assert!(matches!(err, rw_engine::export::ExportError::Cancelled));
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
}
