//! Looks survive export: placement, generated backgrounds and fades
//! (picture and sound).

mod common;

use rw_cache::ProjectCache;
use rw_engine::export::{ExportSettings, export};
use rw_engine::import::probe_file;
use rw_media::decode::{CHANNELS, SAMPLE_RATE};
use rw_media::{AudioReader, Ffmpeg, VideoReader};
use rw_model::{
    Canvas, Fit, FrameRate, Generated, Item, Project, Rgba, Time, Track, TrackKind, Transform,
};
use rw_render::Gpu;
use std::sync::atomic::AtomicBool;

fn gpu() -> Option<Gpu> {
    match Gpu::headless() {
        Ok(g) => Some(g),
        Err(e) if std::env::var("RW_REQUIRE_GPU").is_ok_and(|v| v == "1") => panic!("{e}"),
        Err(_) => None,
    }
}

fn px(img: &rw_media::RgbaImage, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * img.width + x) * 4) as usize;
    [img.pixels[i], img.pixels[i + 1], img.pixels[i + 2]]
}

fn rms(s: &[f32]) -> f32 {
    (s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32).sqrt()
}

#[test]
fn pip_background_and_fades_export() {
    let Some(gpu) = gpu() else { return };
    let g = common::fixtures();
    let gray = g.path("frames_gray_h264.mp4");
    if !gray.exists() {
        return;
    }
    let io = Ffmpeg::new();
    let tmp = tempfile::tempdir().unwrap();
    let fps = FrameRate::FPS_30;
    let gray_m = probe_file(&io, &gray).unwrap();
    let tone = probe_file(&io, &g.path("tone.wav")).unwrap();
    let mut p = Project::new("look", Canvas::custom(320, 240));

    let mut main = Track::new_main();
    let mut bg = Item::generated(
        Generated::Solid {
            color: Rgba([220, 20, 20, 255]),
        },
        Time::ZERO,
    );
    bg.duration = Time::from_secs(1);
    main.items.push(bg);

    let mut over = Track::new(TrackKind::Video);
    let mut pip = Item::new(&gray_m, Time::ZERO);
    pip.duration = Time::from_secs(1);
    pip.look.transform = Transform {
        center: [0.75, 0.75],
        size: [0.5, 0.5],
        fit: Fit::Stretch,
        ..Transform::default()
    };
    pip.look.fade_in = Time::from_millis(500);
    over.items.push(pip);

    let mut audio = Track::new(TrackKind::Audio);
    let mut t = Item::new(&tone, Time::ZERO);
    t.duration = Time::from_secs(1);
    t.look.fade_in = Time::from_millis(500);
    audio.items.push(t);

    p.media = vec![gray_m, tone];
    p.tracks = vec![main, over, audio];

    let out = tmp.path().join("look.mp4");
    let settings = ExportSettings::for_project(&p, &out);
    export(
        &p,
        &ProjectCache::for_project(tmp.path()),
        &gpu,
        &settings,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();

    let mut r = VideoReader::open(&out, None, false).unwrap();
    let red = |c: [u8; 3]| c[0] > 180 && c[1] < 60 && c[2] < 60;
    for k in [0i64, 7, 20, 29] {
        let f = r
            .frame_at(fps.frame_to_time(k) + Time::from_millis(2))
            .unwrap();
        // Outside the picture-in-picture box: the red background.
        assert!(
            red(px(&f.image, 40, 40)),
            "frame {k} background {:?}",
            px(&f.image, 40, 40)
        );
        let inside = px(&f.image, 240, 180);
        match k {
            0 => assert!(red(inside), "fully faded out at the start: {inside:?}"),
            20 | 29 => {
                // Grey frame k: level 4k, equal channels.
                assert!(
                    inside[0].abs_diff(inside[2]) < 12,
                    "frame {k} grey {inside:?}"
                );
                assert!(
                    (i64::from(inside[1]) - 4 * k).abs() < 10,
                    "frame {k} level {inside:?}"
                );
            }
            _ => assert!(
                !red(inside) && inside[0] > inside[2] + 20,
                "frame {k} mid-fade mix {inside:?}"
            ),
        }
    }

    let mut a = AudioReader::open(&out).unwrap();
    let mut buf = vec![0.0f32; SAMPLE_RATE as usize * CHANNELS];
    let n = a.read(&mut buf).unwrap();
    let window = |at: f32| {
        let s = (at * SAMPLE_RATE as f32) as usize * CHANNELS;
        rms(&buf[s..(s + 2400 * CHANNELS).min(n)])
    };
    let (start, mid, full) = (window(0.02), window(0.25), window(0.7));
    assert!(
        start < full * 0.15,
        "fade-in starts quiet: {start} vs {full}"
    );
    assert!(
        (mid / full - 0.5).abs() < 0.12,
        "halfway through the fade ≈ half level: {mid} vs {full}"
    );
}
