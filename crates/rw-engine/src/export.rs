//! Rendering the timeline to a file: every frame, full quality.

use crate::frames::FrameSource;
use rw_audio::{CHANNELS, Mixer, time_to_sample};
use rw_cache::ProjectCache;
use rw_media::{AudioSettings, MediaError, Mp4Writer, VideoSettings};
use rw_model::{Project, Time};
use rw_render::{Compositor, Gpu, RenderError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq)]
pub struct ExportSettings {
    pub path: PathBuf,
    /// Output size; must be even. Usually the canvas size.
    pub width: u32,
    pub height: u32,
    pub video_bit_rate: usize,
    pub audio_bit_rate: usize,
    /// Encoder name, or `None` for the first that works.
    pub encoder: Option<String>,
    /// Timeline range to export; `None` is the whole project.
    pub range: Option<(Time, Time)>,
}

impl ExportSettings {
    /// Full-canvas export with bit rates suited to the size.
    pub fn for_project(project: &Project, path: &Path) -> Self {
        let c = project.canvas;
        let pixels = u64::from(c.width) * u64::from(c.height);
        // About 4 Mbit/s per megapixel at 30 fps (≈ 8 Mbit/s for 1080p).
        let rate = (pixels as f64 * 4.0 * c.frame_rate.as_f64() / 30.0) as usize;
        ExportSettings {
            path: path.to_path_buf(),
            width: c.width,
            height: c.height,
            video_bit_rate: rate.max(500_000),
            audio_bit_rate: 192_000,
            encoder: None,
            range: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("the timeline is empty")]
    Empty,
    #[error("export cancelled")]
    Cancelled,
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Render(#[from] RenderError),
}

/// What happened, for the "export finished" message.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportReport {
    pub path: PathBuf,
    pub frames: i64,
    pub duration: Time,
    pub encoder: String,
}

/// Renders `project` to an MP4. `progress` receives `0.0..=1.0`. On error
/// or cancellation no partial file is left behind.
pub fn export(
    project: &Project,
    cache: &ProjectCache,
    gpu: &Gpu,
    settings: &ExportSettings,
    cancel: &AtomicBool,
    progress: &dyn Fn(f32),
) -> Result<ExportReport, ExportError> {
    let (start, end) = settings.range.unwrap_or((Time::ZERO, project.duration()));
    if end <= start {
        return Err(ExportError::Empty);
    }
    let fps = project.canvas.frame_rate;
    // Frames whose start time falls inside [start, end).
    let mut first = fps.time_to_frame(start);
    if fps.frame_to_time(first) < start {
        first += 1;
    }
    let mut last = fps.time_to_frame(end);
    if fps.frame_to_time(last) >= end {
        last -= 1;
    }
    let frames = (last - first + 1).max(1);

    let size = (settings.width, settings.height);
    let mut writer = Mp4Writer::create(
        &settings.path,
        &VideoSettings {
            width: size.0,
            height: size.1,
            frame_rate: fps,
            bit_rate: settings.video_bit_rate,
            encoder: settings.encoder.clone(),
        },
        Some(&AudioSettings {
            bit_rate: settings.audio_bit_rate,
        }),
    )?;
    let mut source = FrameSource::new(cache.clone(), false);
    let mut compositor = Compositor::new(gpu, size);
    let mut mixer = Mixer::new();
    let mut audio = Vec::new();
    let audio_start = time_to_sample(start);

    let result = (|| -> Result<(), ExportError> {
        for i in 0..frames {
            if cancel.load(Ordering::Relaxed) {
                return Err(ExportError::Cancelled);
            }
            let t = fps.frame_to_time(first + i);
            let layers = source.layers_at(project, t, size);
            compositor.render(project.canvas.background, &layers);
            writer.write_video(&compositor.read_back()?)?;

            // Audio covering exactly this frame's span, in whole samples, so
            // rounding never accumulates.
            let s0 = time_to_sample(fps.frame_to_time(first + i).max(start)) - audio_start;
            let s1 = time_to_sample(fps.frame_to_time(first + i + 1).min(end)) - audio_start;
            if s1 > s0 {
                audio.resize((s1 - s0) as usize * CHANNELS, 0.0);
                mixer.mix(project, audio_start + s0, &mut audio);
                for s in audio.iter_mut() {
                    *s = s.clamp(-1.0, 1.0);
                }
                writer.write_audio(&audio)?;
            }
            progress((i + 1) as f32 / frames as f32);
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            let encoder = writer.encoder_name().to_owned();
            let path = writer.finish()?;
            Ok(ExportReport {
                path,
                frames,
                duration: end - start,
                encoder,
            })
        }
        Err(e) => {
            writer.abort();
            Err(e)
        }
    }
}
