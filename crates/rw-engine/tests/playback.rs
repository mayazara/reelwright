//! Playback clock, A/V sync and a soak run on a multi-track 1080p project.

mod common;

use rw_audio::output::{AudioOutput, NullFactory, NullOutput, OutputFactory, Pull};
use rw_audio::{CHANNELS, SAMPLE_RATE};
use rw_cache::ProjectCache;
use rw_engine::import::probe_file;
use rw_engine::playback::Player;
use rw_media::Ffmpeg;
use rw_model::{Canvas, FrameRate, Item, Project, Time, Track, TrackKind};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn gray_index(img: &rw_media::RgbaImage) -> i64 {
    let mid = ((img.height / 2 * img.width + img.width / 2) * 4) as usize;
    (f64::from(img.pixels[mid]) / 4.0).round() as i64
}

#[test]
fn frames_follow_the_audio_clock() {
    let g = common::fixtures();
    let path = g.path("frames_gray_h264.mp4");
    if !path.exists() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let m = probe_file(&Ffmpeg::new(), &path).unwrap();
    let mut p = Project::new("p", Canvas::custom(320, 240));
    let mut main = Track::new_main();
    main.items.push(Item::new(&m, Time::ZERO));
    p.media.push(m);
    p.tracks.push(main);
    let fps = FrameRate::FPS_30;

    let mut player = Player::new(
        Arc::new(NullFactory::default()),
        ProjectCache::for_project(tmp.path()),
        Arc::new(p),
    );
    let from = fps.frame_to_time(5);
    player.play(from, (320, 240), false);
    let started = Instant::now();
    let mut last_t = Time(-1);
    let mut checked = 0;
    while started.elapsed() < Duration::from_millis(1200) {
        let clock = player.position();
        if let Some(f) = player.take_frame() {
            let after = player.position();
            assert!(f.t <= after, "frame {} shown before its time {after}", f.t);
            assert!(f.t > last_t, "frames go forward");
            assert!(
                clock - f.t < fps.frame_duration() + Time::from_millis(20),
                "lag {}",
                clock - f.t
            );
            assert_eq!(
                gray_index(f.layers[0].as_image().unwrap()),
                fps.time_to_frame(f.t),
                "picture matches its time"
            );
            last_t = f.t;
            checked += 1;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    let pos = player.pause();
    // Every frame shown was checked above; slow debug CI runners show fewer.
    assert!(checked > 5, "only {checked} frames shown");
    // The clock advanced roughly in real time from the start point.
    let advanced = (pos - from).as_secs_f64();
    assert!(
        (advanced - 1.2).abs() < 0.25,
        "clock advanced {advanced} s in 1.2 s"
    );
    let stats = player.stats();
    assert!(stats.max_lag < fps.frame_duration(), "{stats:?}");
}

/// Output that records what it plays, at a multiple of real time.
struct Capture {
    samples: Arc<Mutex<Vec<f32>>>,
    speed: f64,
}

struct CaptureOutput(#[allow(dead_code)] NullOutput);

impl AudioOutput for CaptureOutput {
    fn latency(&self) -> Duration {
        Duration::ZERO
    }
    fn name(&self) -> String {
        "capture".into()
    }
}

impl OutputFactory for Capture {
    fn open(&self, mut pull: Pull) -> Result<Box<dyn AudioOutput>, String> {
        let samples = self.samples.clone();
        Ok(Box::new(CaptureOutput(NullOutput::start(
            Box::new(move |buf: &mut [f32]| {
                pull(buf);
                samples.lock().unwrap().extend_from_slice(buf);
            }),
            self.speed,
        ))))
    }
}

#[test]
fn sound_plays_at_the_right_sample() {
    let g = common::fixtures();
    let tmp = tempfile::tempdir().unwrap();
    let beep = probe_file(&Ffmpeg::new(), &g.path("beep_44k.wav")).unwrap();
    let mut p = Project::new("p", Canvas::custom(64, 64));
    let mut audio = Track::new(TrackKind::Audio);
    let mut item = Item::new(&beep, Time::from_millis(700)); // beep at 1.7 s
    item.gain = 0.5;
    audio.items.push(item);
    p.media.push(beep);
    p.tracks.push(Track::new_main());
    p.tracks.push(audio);

    let samples = Arc::new(Mutex::new(Vec::new()));
    let factory = Capture {
        samples: samples.clone(),
        speed: 4.0,
    };
    let mut player = Player::new(
        Arc::new(factory),
        ProjectCache::for_project(tmp.path()),
        Arc::new(p),
    );
    player.play(Time::from_millis(1500), (64, 64), false);
    std::thread::sleep(Duration::from_millis(250)); // ≈ 1 s of audio at 4x
    player.pause();
    let s = samples.lock().unwrap();
    let onset = s
        .chunks(CHANNELS)
        .position(|f| f[0].abs() > 0.1)
        .expect("beep heard");
    let want = SAMPLE_RATE as usize / 5; // 1.7 s - 1.5 s
    assert!(onset.abs_diff(want) <= 48, "onset at {onset}, want {want}");
    let peak = s.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    // Compare with the file's own peak: the item's gain is 0.5.
    let mut r = rw_media::AudioReader::open(&g.path("beep_44k.wav")).unwrap();
    let mut src = vec![0.0f32; 3 * SAMPLE_RATE as usize * CHANNELS];
    let n = r.read(&mut src).unwrap();
    let src_peak = src[..n].iter().fold(0.0f32, |a, v| a.max(v.abs()));
    assert!(
        (peak / src_peak - 0.5).abs() < 0.02,
        "gain applied: {peak} vs source {src_peak}"
    );
}

/// 3 video tracks + 2 audio tracks of 1080p H.264, played in real time.
/// `RW_SOAK_SECS` sets the length (default 5 s; the M2 acceptance run uses
/// 600). `RW_PERF=1` enforces the < 1 % dropped-frame target, which needs a
/// release build on a machine like the plan's reference laptop.
#[test]
fn soak_multitrack_1080p() {
    let g = common::fixtures();
    let clip = g.path("h264_1080p30_aac.mp4");
    if !clip.exists() {
        return;
    }
    let secs: u64 = std::env::var("RW_SOAK_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let tmp = tempfile::tempdir().unwrap();
    let io = Ffmpeg::new();
    let v = probe_file(&io, &clip).unwrap();
    let a = probe_file(&io, &g.path("tone.wav")).unwrap();
    let mut p = Project::new("soak", Canvas::default());
    // Fill each track with back-to-back copies of the 2 s clip.
    let fill = |media: &rw_model::MediaRef, track: &mut Track, total: Time| {
        let mut at = Time::ZERO;
        while at < total {
            let mut i = Item::new(media, at);
            i.duration = i.duration.min(total - at);
            at += i.duration;
            track.items.push(i);
        }
    };
    let total = Time::from_secs(secs as i64 + 1);
    let mut tracks = vec![
        Track::new_main(),
        Track::new(TrackKind::Video),
        Track::new(TrackKind::Video),
    ];
    for t in &mut tracks {
        fill(&v, t, total);
    }
    for _ in 0..2 {
        let mut t = Track::new(TrackKind::Audio);
        fill(&a, &mut t, total);
        tracks.push(t);
    }
    p.media = vec![v, a];
    p.tracks = tracks;

    let mut player = Player::new(
        Arc::new(NullFactory::default()),
        ProjectCache::for_project(tmp.path()),
        Arc::new(p),
    );
    player.play(Time::ZERO, (960, 540), true);
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        player.take_frame();
        std::thread::sleep(Duration::from_millis(8));
    }
    let end = player.pause();
    let stats = player.stats();
    let expected = (end.as_secs_f64() * 30.0) as u64;
    let drop_rate = stats.dropped as f64 / expected.max(1) as f64;
    eprintln!(
        "soak {secs}s: shown {} dropped {} ({:.2}%), max lag {}, underrun samples {}, clock {}",
        stats.shown,
        stats.dropped,
        drop_rate * 100.0,
        stats.max_lag,
        stats.underrun_samples,
        end
    );
    // Sync never drifts: whatever is shown is less than a frame behind.
    if stats.shown > 0 && std::env::var_os("RW_PERF").is_some() {
        assert!(
            stats.max_lag < FrameRate::FPS_30.frame_duration(),
            "{stats:?}"
        );
        assert!(drop_rate < 0.01, "dropped {:.2}%", drop_rate * 100.0);
    }
    assert!(
        (end.as_secs_f64() - secs as f64).abs() < 0.5,
        "clock {end} after {secs}s"
    );
}

/// Plays a beep through the real sound card. Run with
/// `cargo test -p rw-engine --test playback device -- --ignored --nocapture`.
#[test]
#[ignore = "needs a sound device and makes a sound"]
fn device_output_plays_in_real_time() {
    let g = common::fixtures();
    let tmp = tempfile::tempdir().unwrap();
    let beep = probe_file(&Ffmpeg::new(), &g.path("beep_44k.wav")).unwrap();
    let mut p = Project::new("p", Canvas::custom(64, 64));
    let mut audio = Track::new(TrackKind::Audio);
    audio.items.push(Item::new(&beep, Time::ZERO));
    p.media.push(beep);
    p.tracks.push(Track::new_main());
    p.tracks.push(audio);
    let mut player = Player::new(
        Arc::new(rw_audio::device::DeviceFactory),
        ProjectCache::for_project(tmp.path()),
        Arc::new(p),
    );
    player.play(Time::from_millis(500), (64, 64), false);
    let start = Instant::now();
    std::thread::sleep(Duration::from_millis(1500));
    let pos = player.position();
    let elapsed = start.elapsed().as_secs_f64();
    let name = player.output_name().to_owned();
    let stats = player.stats();
    player.pause();
    eprintln!(
        "device: {name}; clock advanced {:.3} s in {elapsed:.3} s; underruns {}",
        (pos - Time::from_millis(500)).as_secs_f64(),
        stats.underrun_samples
    );
    assert_ne!(name, "No audio device", "device failed to open");
    let advanced = (pos - Time::from_millis(500)).as_secs_f64();
    assert!(
        (advanced - elapsed).abs() < 0.25,
        "clock {advanced} vs wall {elapsed}"
    );
}
