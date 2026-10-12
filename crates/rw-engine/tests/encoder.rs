//! MP4 writer round trip: what is written decodes back frame-accurately.

use rw_media::decode::{CHANNELS, SAMPLE_RATE};
use rw_media::{
    AudioReader, AudioSettings, Ffmpeg, MediaIo, Mp4Writer, RgbaImage, VideoReader, VideoSettings,
};
use rw_model::{FrameRate, Time};

fn gray(w: u32, h: u32, level: u8) -> RgbaImage {
    RgbaImage::new(w, h, [level, level, level, 255].repeat((w * h) as usize))
}

fn index_of(img: &RgbaImage) -> i64 {
    let mid = ((img.height / 2 * img.width + img.width / 2) * 4) as usize;
    (f64::from(img.pixels[mid]) / 4.0).round() as i64
}

fn round_trip(encoder: Option<&str>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.mp4");
    let fps = FrameRate::new(30000, 1001);
    let video = VideoSettings {
        width: 320,
        height: 240,
        frame_rate: fps,
        bit_rate: 4_000_000,
        encoder: encoder.map(str::to_owned),
        codec: Default::default(),
    };
    let mut w =
        Mp4Writer::create(&path, &video, Some(&AudioSettings { bit_rate: 192_000 })).unwrap();
    let frames = 60;
    let samples_per_frame = f64::from(SAMPLE_RATE) / fps.as_f64();
    let mut written = 0usize;
    for n in 0..frames {
        w.write_video(&gray(320, 240, (n * 4) as u8)).unwrap();
        // Audio for this frame: a beep during frame 30 onwards for 0.1 s.
        let until = (((n + 1) as f64) * samples_per_frame).round() as usize;
        let mut chunk = Vec::new();
        for s in written..until {
            let t = s as f64 / f64::from(SAMPLE_RATE);
            let beep = t >= fps.frame_to_time(30).as_secs_f64()
                && t < fps.frame_to_time(30).as_secs_f64() + 0.1;
            let v = if beep {
                0.8 * (2.0 * std::f64::consts::PI * 1000.0 * t).sin() as f32
            } else {
                0.0
            };
            chunk.extend([v; CHANNELS]);
        }
        written = until;
        w.write_audio(&chunk).unwrap();
    }
    let name = w.encoder_name().to_owned();
    let out = w.finish().unwrap();
    assert!(!dir.path().join("out.mp4.part").exists());

    let info = Ffmpeg::new().probe(&out).unwrap();
    let v = info.video.unwrap();
    assert_eq!((v.width, v.height), (320, 240), "{name}");
    assert_eq!(
        v.frame_rate.as_f64().mul_add(1000.0, 0.0).round(),
        29970.0,
        "{name}"
    );
    let expected = fps.frame_to_time(frames);
    assert!(
        (info.duration - expected).ticks().abs() < Time::from_millis(60).ticks(),
        "{name}: {} vs {expected}",
        info.duration
    );

    let mut r = VideoReader::open(&out, None, false).unwrap();
    for n in [0, 1, 29, 30, 59, 10, 45] {
        let f = r
            .frame_at(fps.frame_to_time(n) + Time::from_millis(1))
            .unwrap();
        assert_eq!(index_of(&f.image), n, "{name}: frame {n}");
    }

    let mut a = AudioReader::open(&out).unwrap();
    let mut buf = vec![0.0f32; SAMPLE_RATE as usize * 2 * CHANNELS];
    let n = a.read(&mut buf).unwrap();
    let onset = buf[..n]
        .chunks(CHANNELS)
        .position(|f| f[0].abs() > 0.2)
        .unwrap();
    let want = (fps.frame_to_time(30).as_secs_f64() * f64::from(SAMPLE_RATE)) as i64;
    assert!(
        (onset as i64 - want).abs() <= 48,
        "{name}: audio onset {onset}, want {want}"
    );
}

#[test]
fn round_trip_default_h264_encoder() {
    round_trip(None);
}

#[test]
fn round_trip_builtin_fallback_encoder() {
    round_trip(Some("mpeg4"));
}

#[test]
fn odd_sizes_and_wrong_frames_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let s = VideoSettings {
        width: 321,
        height: 240,
        frame_rate: FrameRate::FPS_30,
        bit_rate: 1_000_000,
        encoder: None,
        codec: Default::default(),
    };
    assert!(Mp4Writer::create(&dir.path().join("a.mp4"), &s, None).is_err());
    let s = VideoSettings { width: 320, ..s };
    let mut w = Mp4Writer::create(&dir.path().join("b.mp4"), &s, None).unwrap();
    assert!(w.write_video(&gray(16, 16, 0)).is_err());
    w.abort();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
