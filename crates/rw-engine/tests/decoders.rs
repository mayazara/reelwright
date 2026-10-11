//! Frame-accurate video seeking and sample-accurate audio.

mod common;

use rw_media::decode::{CHANNELS, SAMPLE_RATE};
use rw_media::{AudioReader, VideoReader};
use rw_model::{FrameRate, Time};

/// Frame index encoded in the grey fixture's luma.
fn frame_index(img: &rw_media::RgbaImage) -> i64 {
    let mid = ((img.height / 2 * img.width + img.width / 2) * 4) as usize;
    (f64::from(img.pixels[mid]) / 4.0).round() as i64
}

#[test]
fn video_frames_match_time_in_any_order() {
    let g = common::fixtures();
    let path = g.path("frames_gray_h264.mp4");
    if !path.exists() {
        eprintln!("skipping: no H.264 encoder");
        return;
    }
    let mut r = VideoReader::open(&path, None, false).unwrap();
    let fps = FrameRate::FPS_30;
    // Sequential, then jumps backwards and forwards, then mid-frame times.
    let mut frames: Vec<i64> = (0..60).collect();
    frames.extend([59, 0, 45, 3, 30, 29, 58, 1, 15, 14, 16]);
    for n in frames {
        for offset in [Time::ZERO, Time(fps.frame_duration().ticks() / 2)] {
            let t = fps.frame_to_time(n) + offset;
            let f = r.frame_at(t).unwrap();
            assert_eq!(
                frame_index(&f.image),
                n,
                "at {t} (frame {n}), pts {}",
                f.pts
            );
            assert!(
                f.covers(t),
                "frame {n}: pts {} dur {} doesn't cover {t}",
                f.pts,
                f.duration
            );
        }
    }
    // Past the end holds the last frame.
    let last = r.frame_at(Time::from_secs(5)).unwrap();
    assert_eq!(frame_index(&last.image), 59);
}

#[test]
fn video_scales_to_requested_size() {
    let g = common::fixtures();
    let path = g.path("h264_1080p30_aac.mp4");
    if !path.exists() {
        return;
    }
    let mut r = VideoReader::open(&path, Some((480, 270)), false).unwrap();
    let f = r.frame_at(Time::from_millis(500)).unwrap();
    assert_eq!((f.image.width, f.image.height), (480, 270));
    r.set_out_size((192, 108));
    let f = r.frame_at(Time::from_millis(600)).unwrap();
    assert_eq!((f.image.width, f.image.height), (192, 108));
}

#[test]
fn still_image_is_one_frame_forever() {
    let g = common::fixtures();
    let mut r = VideoReader::open(&g.path("still.png"), Some((160, 90)), true).unwrap();
    let a = r.frame_at(Time::ZERO).unwrap();
    let b = r.frame_at(Time::from_secs(100)).unwrap();
    assert_eq!(a.image, b.image);
    assert!(b.covers(Time::from_secs(100)));
}

/// Index of the first sample louder than the threshold.
fn onset(samples: &[f32]) -> Option<usize> {
    samples.chunks(CHANNELS).position(|f| f[0].abs() > 0.2)
}

fn read_secs(r: &mut AudioReader, secs: f64) -> Vec<f32> {
    let mut buf = vec![0.0; (secs * f64::from(SAMPLE_RATE)) as usize * CHANNELS];
    let n = r.read(&mut buf).unwrap();
    buf.truncate(n);
    buf
}

#[test]
fn audio_is_resampled_and_sample_accurate_after_seek() {
    let g = common::fixtures();
    // Tolerance: 1 ms covers resampler and AAC priming filter delay.
    let tol = (SAMPLE_RATE / 1000) as i64;
    for name in ["beep_44k.wav", "beep_aac.m4a"] {
        let mut r = AudioReader::open(&g.path(name)).unwrap();
        let all = read_secs(&mut r, 3.0);
        let len = all.len() / CHANNELS;
        assert!(
            (len as i64 - 3 * i64::from(SAMPLE_RATE)).abs() < 2048,
            "{name}: {len} samples"
        );
        let on = onset(&all).unwrap() as i64;
        assert!(
            (on - i64::from(SAMPLE_RATE)).abs() <= tol,
            "{name}: onset at {on}"
        );

        for seek_ms in [900i64, 500, 0, 950] {
            r.seek(Time::from_millis(seek_ms));
            assert_eq!(r.position(), Time::from_millis(seek_ms));
            let chunk = read_secs(&mut r, 0.5);
            let want = (1000 - seek_ms) * i64::from(SAMPLE_RATE) / 1000;
            match onset(&chunk) {
                Some(on) => assert!(
                    (on as i64 - want).abs() <= tol,
                    "{name} seek {seek_ms}: onset {on}, want {want}"
                ),
                // The beep starts at or after the end of this chunk.
                None => assert!(
                    want >= (chunk.len() / CHANNELS) as i64 - tol,
                    "{name} seek {seek_ms}: no onset, want {want}"
                ),
            }
        }
        // Position tracks what was read.
        r.seek(Time::from_millis(100));
        read_secs(&mut r, 0.25);
        assert_eq!(r.position(), Time::from_millis(350));
    }
}
