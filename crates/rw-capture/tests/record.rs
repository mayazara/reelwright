//! Recording sessions with synthetic sources: files, pauses, retakes and
//! sound kept in step with pictures.

use rw_capture::clock::{SystemTime, TimeSource};
use rw_capture::sources::{TestPattern, test_tone};
use rw_capture::{FileKind, Options, Recorder, Sources};
use rw_media::{AudioReader, Ffmpeg, MediaIo};
use std::sync::Arc;
use std::time::Duration;

fn options(dir: &std::path::Path, name: &str) -> Options {
    Options {
        folder: dir.to_path_buf(),
        name: name.into(),
        fps: 30,
        encoder: None,
    }
}

#[test]
fn screen_camera_and_sound_with_a_pause() {
    let tmp = tempfile::tempdir().unwrap();
    let time: Arc<dyn TimeSource> = Arc::new(SystemTime::new());
    let sources = Sources {
        screen: Some(Box::new(TestPattern::new((640, 360), 30))),
        camera: Some(Box::new(TestPattern::new((320, 240), 30))),
        audio: vec![test_tone(time.clone(), 0.0)],
    };
    let rec = Recorder::start(sources, options(tmp.path(), "Take"), time).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    rec.pause();
    std::thread::sleep(Duration::from_millis(1000));
    rec.resume();
    std::thread::sleep(Duration::from_millis(1500));
    let elapsed = rec.elapsed();
    let (files, problem) = rec.finish();
    assert!(problem.is_none(), "{problem:?}");
    assert!(
        (elapsed.as_secs_f64() - 3.0).abs() < 0.2,
        "elapsed {elapsed:?}"
    );
    let kinds: Vec<FileKind> = files.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [FileKind::Screen, FileKind::Camera]);

    let io = Ffmpeg::new();
    let screen = io.probe(&files[0].path).unwrap();
    let camera = io.probe(&files[1].path).unwrap();
    // The paused second isn't in the files.
    for (name, info) in [("screen", &screen), ("camera", &camera)] {
        assert!(
            (info.duration.as_secs_f64() - 3.0).abs() < 0.25,
            "{name} lasts {}",
            info.duration
        );
    }
    assert!(screen.audio.is_some(), "sound goes with the screen");
    assert!(camera.audio.is_none());
    let v = screen.video.unwrap();
    assert_eq!((v.width, v.height), (640, 360));
}

/// Clicks every second of the sound card's own count, with the card 2 %
/// fast (exaggerated so 5 s shows what 30 min of a real card's drift does:
/// uncorrected, the clicks would slide 3 frames): in the file they must
/// stay where the clock says, within 2 frames.
#[test]
fn sound_stays_in_step_with_a_fast_sound_card() {
    let tmp = tempfile::tempdir().unwrap();
    let time: Arc<dyn TimeSource> = Arc::new(SystemTime::new());
    let sources = Sources {
        screen: Some(Box::new(TestPattern::new((320, 240), 30))),
        camera: None,
        audio: vec![test_tone(time.clone(), 20_000.0)],
    };
    let rec = Recorder::start(sources, options(tmp.path(), "Sync"), time).unwrap();
    std::thread::sleep(Duration::from_millis(5200));
    let (files, problem) = rec.finish();
    assert!(problem.is_none(), "{problem:?}");
    let mut r = AudioReader::open(&files[0].path).unwrap();
    let mut buf = vec![0.0f32; 48_000 * 2 * 6];
    let n = r.read(&mut buf).unwrap();
    // Click onsets (left channel), in samples.
    let mut onsets = Vec::new();
    let mut last = -48_000i64;
    for (i, f) in buf[..n].chunks(2).enumerate() {
        if f[0] > 0.5 && i as i64 - last > 24_000 {
            onsets.push(i as i64);
            last = i as i64;
        }
    }
    assert!(onsets.len() >= 4, "clicks found at {onsets:?}");
    // The card's second k is captured at k / 1.02 s of real time.
    let frame = 48_000 / 30;
    for (k, at) in onsets.iter().enumerate().skip(1) {
        let want = (k as f64 * 48_000.0 / 1.02).round() as i64;
        let first = onsets[0];
        assert!(
            ((at - first) - want).abs() <= 2 * frame,
            "click {k} at {} samples after the first, want {want}",
            at - first
        );
    }
}

#[test]
fn sound_only_and_retake() {
    let tmp = tempfile::tempdir().unwrap();
    let time: Arc<dyn TimeSource> = Arc::new(SystemTime::new());
    let rec = Recorder::start(
        Sources {
            screen: None,
            camera: None,
            audio: vec![test_tone(time.clone(), 0.0)],
        },
        options(tmp.path(), "Voice"),
        time.clone(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    let (files, problem) = rec.finish();
    assert!(problem.is_none(), "{problem:?}");
    assert_eq!(files[0].kind, FileKind::Sound);
    let info = Ffmpeg::new().probe(&files[0].path).unwrap();
    assert!(info.video.is_none() && info.audio.is_some());
    assert!(
        (info.duration.as_secs_f64() - 1.2).abs() < 0.25,
        "{}",
        info.duration
    );

    // A retake leaves nothing behind.
    let rec = Recorder::start(
        Sources {
            screen: Some(Box::new(TestPattern::new((320, 240), 30))),
            camera: None,
            audio: Vec::new(),
        },
        options(tmp.path(), "Retake"),
        time,
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    rec.discard();
    let left: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("Retake"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}
