//! M4 acceptance: 4× speed plays smoothly with the sound's pitch unchanged.

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

/// Output that records what it plays, faster than real time.
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

/// Strongest frequency of the left channel, searched 1 Hz at a time
/// between `lo` and `hi` (Goertzel filters).
fn dominant(samples: &[f32], lo: f64, hi: f64) -> f64 {
    let left: Vec<f64> = samples
        .iter()
        .step_by(CHANNELS)
        .map(|v| f64::from(*v))
        .collect();
    let power = |f: f64| {
        let w = std::f64::consts::TAU * f / f64::from(SAMPLE_RATE);
        let (mut s1, mut s2) = (0.0, 0.0);
        for &x in &left {
            let s = x + 2.0 * w.cos() * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2
    };
    let mut best = (lo, f64::MIN);
    let mut f = lo;
    while f <= hi {
        let p = power(f);
        if p > best.1 {
            best = (f, p);
        }
        f += 1.0;
    }
    best.0
}

/// Plays a 3 s 440 Hz tone at 4× through the player and returns the
/// strongest frequency heard.
fn pitch_at_4x(keep_pitch: bool) -> Option<f64> {
    let g = common::fixtures();
    let path = g.path("tone.wav");
    if !path.exists() {
        return None;
    }
    let tmp = tempfile::tempdir().unwrap();
    let tone = probe_file(&Ffmpeg::new(), &path).unwrap();
    let mut p = Project::new("p", Canvas::custom(64, 64));
    let mut audio = Track::new(TrackKind::Audio);
    let mut item = Item::new(&tone, Time::ZERO);
    item.speed = 4.0;
    item.keep_pitch = keep_pitch;
    item.duration = Time::from_millis(750);
    audio.items.push(item);
    p.media.push(tone);
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
    player.play(Time::ZERO, (64, 64), false);
    // The 0.75 s clip, played back at 4× real time, plus slack.
    std::thread::sleep(Duration::from_millis(400));
    player.pause();
    let s = samples.lock().unwrap();
    // Skip the first 50 ms; analyse the next half second.
    let from = SAMPLE_RATE as usize / 20 * CHANNELS;
    let to = (from + SAMPLE_RATE as usize / 2 * CHANNELS).min(s.len());
    assert!(
        to > from + SAMPLE_RATE as usize / 4 * CHANNELS,
        "captured {} samples",
        s.len()
    );
    Some(dominant(&s[from..to], 200.0, 2200.0))
}

#[test]
fn four_times_speed_keeps_pitch() {
    let Some(f) = pitch_at_4x(true) else { return };
    assert!((f - 440.0).abs() < 6.0, "pitch kept: heard {f} Hz");
}

#[test]
fn four_times_speed_raises_pitch_when_asked() {
    let Some(f) = pitch_at_4x(false) else { return };
    assert!(
        (f - 1760.0).abs() < 10.0,
        "pitch follows speed: heard {f} Hz"
    );
}

/// 1080p clips at 4× played in real time for `RW_SOAK_SECS` (default 4 s).
/// `RW_PERF=1` enforces the < 1 % dropped-frame target (release build).
#[test]
fn four_times_speed_plays_smoothly() {
    let g = common::fixtures();
    let clip = g.path("h264_1080p30_aac.mp4");
    if !clip.exists() {
        return;
    }
    let secs: u64 = std::env::var("RW_SOAK_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let tmp = tempfile::tempdir().unwrap();
    let v = probe_file(&Ffmpeg::new(), &clip).unwrap();
    let mut p = Project::new("fast", Canvas::default());
    let mut main = Track::new_main();
    // Back-to-back copies of the 2 s clip, each 0.5 s at 4×.
    let mut at = Time::ZERO;
    while at < Time::from_secs(secs as i64 + 1) {
        let mut i = Item::new(&v, at);
        i.speed = 4.0;
        i.duration = i.duration_for_span(i.duration);
        at += i.duration;
        main.items.push(i);
    }
    p.media.push(v);
    p.tracks.push(main);

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
        "4x for {secs}s: shown {} dropped {} ({:.2}%), max lag {}, underrun samples {}",
        stats.shown,
        stats.dropped,
        drop_rate * 100.0,
        stats.max_lag,
        stats.underrun_samples,
    );
    assert!(
        (end.as_secs_f64() - secs as f64).abs() < 0.5,
        "clock {end} after {secs}s"
    );
    if std::env::var_os("RW_PERF").is_some() {
        assert!(
            stats.max_lag < FrameRate::FPS_30.frame_duration(),
            "{stats:?}"
        );
        assert!(drop_rate < 0.01, "dropped {:.2}%", drop_rate * 100.0);
    }
}
