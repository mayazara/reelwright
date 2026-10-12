//! M4: speed, reverse, freeze frames and transitions land on the right
//! source frames in an export.

mod common;

use rw_cache::ProjectCache;
use rw_edit::History;
use rw_edit::timeline::{Placement, TimelineEdit, TimelineOp};
use rw_engine::export::{ExportSettings, export};
use rw_engine::import::probe_file;
use rw_media::{Ffmpeg, VideoReader};
use rw_model::{Canvas, FrameRate, Item, ItemId, Project, Time, TransitionKind};
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

/// Grey level at the centre, in units of the fixture's step (4 levels per
/// source frame).
fn level(img: &rw_media::RgbaImage) -> f64 {
    let i = (((img.height / 2) * img.width + img.width / 2) * 4) as usize;
    f64::from(img.pixels[i]) / 4.0
}

#[test]
fn speed_reverse_and_freeze_pick_the_right_frames() {
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

    let mut p = Project::new("retime", Canvas::custom(320, 240));
    let m = probe_file(&io, &gray).unwrap();
    p.media = vec![m.clone()];
    p.tracks.push(rw_model::Track::new_main());
    let main = p.tracks[0].id;
    let mut h = History::new();
    let mut run = |p: &mut Project, op| h.execute(p, Box::new(TimelineEdit::new(op))).unwrap();
    let mut add = |p: &mut Project, from: i64, to: i64| -> ItemId {
        let mut item = Item::new(&m, Time::ZERO);
        item.source_in = f(from);
        item.duration = f(to) - f(from);
        let id = item.id;
        run(
            p,
            TimelineOp::Insert {
                placement: Placement::Track(main),
                at: Time::from_secs(99),
                items: vec![item],
            },
        );
        id
    };
    // Source 0..20 at double speed: 10 frames showing 0, 2, 4, ...
    let fast = add(&mut p, 0, 20);
    // Source 30..40 backwards: 39, 38, ... 30.
    let back = add(&mut p, 30, 40);
    // Source 50..55 with frame 52 held for 5 frames.
    let held = add(&mut p, 50, 55);
    let mut run = |p: &mut Project, op| h.execute(p, Box::new(TimelineEdit::new(op))).unwrap();
    run(
        &mut p,
        TimelineOp::SetSpeed {
            items: vec![fast],
            speed: 2.0,
        },
    );
    run(
        &mut p,
        TimelineOp::SetReverse {
            items: vec![back],
            reverse: true,
        },
    );
    run(
        &mut p,
        TimelineOp::Freeze {
            item: held,
            at: f(22),
            duration: f(5),
        },
    );
    assert_eq!(p.duration(), f(30));

    let cache = ProjectCache::for_project(tmp.path());
    let settings = ExportSettings::for_project(&p, &tmp.path().join("retime.mp4"));
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

    let mut r = VideoReader::open(&report.path, None, false).unwrap();
    let mut mismatches = Vec::new();
    for k in 0..30i64 {
        let want = match k {
            0..10 => 2 * k,
            10..20 => 39 - (k - 10),
            20..22 => 50 + (k - 20),
            22..27 => 52,
            _ => 52 + (k - 27),
        };
        let frame = r.frame_at(f(k) + Time::from_millis(2)).unwrap();
        let got = level(&frame.image).round() as i64;
        if got != want {
            mismatches.push(format!("frame {k}: shows source {got}, want {want}"));
        }
    }
    assert!(mismatches.is_empty(), "\n{}", mismatches.join("\n"));
}

#[test]
fn cross_dissolve_blends_across_the_cut() {
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

    let mut p = Project::new("dissolve", Canvas::custom(320, 240));
    let m = probe_file(&io, &gray).unwrap();
    p.media = vec![m.clone()];
    let mut track = rw_model::Track::new_main();
    // Dark frames 0..15, then bright frames 45..60.
    let mut a = Item::new(&m, Time::ZERO);
    a.duration = f(15);
    let mut b = Item::new(&m, Time::ZERO);
    b.source_in = f(45);
    b.duration = f(15);
    let a_id = a.id;
    track.items = vec![a, b];
    p.tracks.push(track);
    rw_edit::timeline::normalize(&mut p.tracks);
    let mut h = History::new();
    h.execute(
        &mut p,
        Box::new(TimelineEdit::new(TimelineOp::AddTransition {
            a: a_id,
            kind: TransitionKind::CrossDissolve,
            duration: f(10),
        })),
    )
    .unwrap();

    let cache = ProjectCache::for_project(tmp.path());
    let settings = ExportSettings::for_project(&p, &tmp.path().join("dissolve.mp4"));
    let report = export(
        &p,
        &cache,
        &gpu,
        &settings,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    let mut r = VideoReader::open(&report.path, None, false).unwrap();
    let mut levels = Vec::new();
    for k in 0..30i64 {
        levels.push(level(
            &r.frame_at(f(k) + Time::from_millis(2)).unwrap().image,
        ));
    }
    // Outside the window (frames 10..20) each clip plays untouched.
    assert!((levels[5] - 5.0).abs() < 0.6, "{levels:?}");
    assert!((levels[25] - 55.0).abs() < 0.6, "{levels:?}");
    // Inside, the picture brightens steadily from the first clip's held
    // last frame towards the second clip's first frame.
    for k in 10..20 {
        assert!(
            levels[k + 1] > levels[k],
            "not rising at frame {k}: {levels:?}"
        );
        assert!(levels[k] > 9.0 && levels[k] < 50.0, "frame {k}: {levels:?}");
    }
}
