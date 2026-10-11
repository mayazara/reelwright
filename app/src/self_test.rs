//! `reelwright --self-test <report>`: checks that a packaged build can do
//! its core work on this machine — load FFmpeg, find a GPU (or software
//! fallback), render, encode, and read the result back — then writes a
//! report and exits non-zero on failure. CI runs it on the packaged exe.

use rw_media::{AudioSettings, Ffmpeg, MediaIo, Mp4Writer, VideoReader, VideoSettings};
use rw_model::{FrameRate, Look, Rgba, Time};
use rw_render::{Compositor, Gpu, Layer, Source};
use std::fmt::Write as _;
use std::path::Path;

pub fn run(report_path: &Path) -> bool {
    let mut report = String::new();
    let ok = checks(&mut report);
    let _ = writeln!(report, "\nresult: {}", if ok { "PASS" } else { "FAIL" });
    let _ = std::fs::write(report_path, &report);
    ok
}

fn checks(r: &mut String) -> bool {
    let _ = writeln!(r, "{} {}", rw_model::app::NAME, rw_model::app::VERSION);
    let _ = writeln!(r, "os: {} {}", std::env::consts::OS, std::env::consts::ARCH);

    // FFmpeg loads (the DLLs next to the exe were found).
    let _ = Ffmpeg::new();
    let encoders = rw_media::encode::available_encoders(rw_media::encode::H264_ENCODERS);
    let _ = writeln!(
        r,
        "ffmpeg: loaded; H.264 encoders: {}",
        if encoders.is_empty() {
            "none (MPEG-4 fallback)".into()
        } else {
            encoders.join(", ")
        }
    );

    // Graphics.
    let gpu = match Gpu::headless() {
        Ok(g) => {
            let _ = writeln!(r, "gpu: {}", g.adapter_name);
            g
        }
        Err(e) => {
            let _ = writeln!(r, "gpu: FAILED {e}");
            return false;
        }
    };

    // Render a frame with a filter, encode one second, decode it back.
    let dir = std::env::temp_dir().join(format!(
        "{}-self-test-{}",
        rw_model::app::ID,
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let out = dir.join("check.mp4");
    let result = (|| -> Result<(), String> {
        let mut comp = Compositor::new(&gpu, (320, 240));
        let look = Look {
            filter: Some(rw_model::Filter {
                id: "warm".into(),
                intensity: 1.0,
            }),
            ..Look::default()
        };
        let layer = Layer {
            source: Source::Solid(Rgba([40, 160, 140, 255])),
            key: 0,
            rotation: 0,
            look,
        };
        let video = VideoSettings {
            width: 320,
            height: 240,
            frame_rate: FrameRate::FPS_30,
            bit_rate: 1_000_000,
            encoder: None,
        };
        let mut w = Mp4Writer::create(&out, &video, Some(&AudioSettings { bit_rate: 128_000 }))
            .map_err(|e| e.to_string())?;
        let encoder = w.encoder_name().to_owned();
        for _ in 0..30 {
            comp.render(Rgba::BLACK, std::slice::from_ref(&layer));
            w.write_video(&comp.read_back().map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            w.write_audio(&[0.0; 3200]).map_err(|e| e.to_string())?;
        }
        w.finish().map_err(|e| e.to_string())?;
        let info = Ffmpeg::new().probe(&out).map_err(|e| e.to_string())?;
        let mut reader = VideoReader::open(&out, None, false).map_err(|e| e.to_string())?;
        let frame = reader
            .frame_at(Time::from_millis(500))
            .map_err(|e| e.to_string())?;
        let mid = ((120 * 320 + 160) * 4) as usize;
        let px = &frame.image.pixels[mid..mid + 3];
        let _ = writeln!(
            r,
            "export: {encoder}, {} decoded, centre pixel {px:?}",
            info.duration
        );
        // Warm filter on teal: still green-blue dominant, not black.
        if px.iter().all(|v| *v < 20) {
            return Err("decoded frame is black".into());
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    match result {
        Ok(()) => true,
        Err(e) => {
            let _ = writeln!(r, "export: FAILED {e}");
            false
        }
    }
}
