//! Rendering the timeline to a file: every frame, full quality.
//!
//! Video goes to MP4 (H.264, HEVC or AV1 with AAC sound) or WebM (VP9 with
//! Opus); GIFs are quantised per frame; MP3 is the sound alone.

use crate::frames::FrameSource;
use crate::gif::GifWriter;
use rw_audio::{CHANNELS, Mixer, SAMPLE_RATE, time_to_sample};
use rw_cache::ProjectCache;
use rw_media::{AudioSettings, AudioWriter, MediaError, Mp4Writer, VideoCodec, VideoSettings};
use rw_model::{FrameRate, Project, Time};
use rw_render::{Compositor, Gpu, RenderError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// What kind of file to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Video {
        codec: VideoCodec,
    },
    /// Looping animation without sound, at `fps` frames a second.
    Gif {
        fps: u32,
    },
    /// Sound only.
    Mp3,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Video { codec } => codec.extension(),
            ExportFormat::Gif { .. } => "gif",
            ExportFormat::Mp3 => "mp3",
        }
    }
}

/// Frame height presets (the short side, for upright canvases).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    P480,
    P720,
    P1080,
    P2160,
}

impl Resolution {
    pub const ALL: [Resolution; 4] = [
        Resolution::P480,
        Resolution::P720,
        Resolution::P1080,
        Resolution::P2160,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Resolution::P480 => "480p",
            Resolution::P720 => "720p",
            Resolution::P1080 => "1080p",
            Resolution::P2160 => "4K",
        }
    }

    pub fn lines(self) -> u32 {
        match self {
            Resolution::P480 => 480,
            Resolution::P720 => 720,
            Resolution::P1080 => 1080,
            Resolution::P2160 => 2160,
        }
    }

    /// Output size for a canvas of `canvas` pixels: the short side becomes
    /// this many lines, the shape stays, both sides even.
    pub fn size_for(self, canvas: (u32, u32)) -> (u32, u32) {
        let (w, h) = (canvas.0.max(2) as f64, canvas.1.max(2) as f64);
        let short = f64::from(self.lines());
        let scale = short / w.min(h);
        let even = |v: f64| ((v / 2.0).round() as u32).max(1) * 2;
        (even(w * scale), even(h * scale))
    }

    /// Whether this is larger than the canvas (upscaling).
    pub fn upscales(self, canvas: (u32, u32)) -> bool {
        self.lines() > canvas.0.min(canvas.1)
    }
}

/// Trade-off between file size and picture quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quality {
    Smaller,
    #[default]
    Balanced,
    Higher,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::Smaller, Quality::Balanced, Quality::Higher];

    pub fn label(self) -> &'static str {
        match self {
            Quality::Smaller => "Smaller file",
            Quality::Balanced => "Balanced",
            Quality::Higher => "Higher quality",
        }
    }

    fn factor(self) -> f64 {
        match self {
            Quality::Smaller => 0.55,
            Quality::Balanced => 1.0,
            Quality::Higher => 1.8,
        }
    }
}

/// Video bit rate for `size` at `fps`: about 4 Mbit/s per megapixel at
/// 30 fps (≈ 8 Mbit/s for 1080p) at balanced quality. Newer codecs need
/// less for the same picture.
pub fn bit_rate(size: (u32, u32), fps: FrameRate, quality: Quality, codec: VideoCodec) -> usize {
    let pixels = f64::from(size.0) * f64::from(size.1);
    let codec_factor = match codec {
        VideoCodec::H264 => 1.0,
        VideoCodec::Vp9 | VideoCodec::Hevc => 0.65,
        VideoCodec::Av1 => 0.5,
    };
    let rate = pixels * 4.0 * fps.as_f64() / 30.0 * quality.factor() * codec_factor;
    (rate as usize).max(300_000)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportSettings {
    pub path: PathBuf,
    pub format: ExportFormat,
    /// Output size; must be even. Usually the canvas size.
    pub width: u32,
    pub height: u32,
    /// `None` keeps the project's frame rate.
    pub frame_rate: Option<FrameRate>,
    pub video_bit_rate: usize,
    pub audio_bit_rate: usize,
    /// Encoder name, or `None` for the first that works.
    pub encoder: Option<String>,
    /// Timeline range to export; `None` is the whole project.
    pub range: Option<(Time, Time)>,
}

impl ExportSettings {
    /// Full-canvas H.264 MP4 with bit rates suited to the size.
    pub fn for_project(project: &Project, path: &Path) -> Self {
        let c = project.canvas;
        let codec = VideoCodec::H264;
        ExportSettings {
            path: path.to_path_buf(),
            format: ExportFormat::Video { codec },
            width: c.width,
            height: c.height,
            frame_rate: None,
            video_bit_rate: bit_rate((c.width, c.height), c.frame_rate, Quality::Balanced, codec),
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
    #[error("couldn't write the file: {0}")]
    Io(#[from] std::io::Error),
}

/// What happened, for the "export finished" message.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportReport {
    pub path: PathBuf,
    pub frames: i64,
    pub duration: Time,
    /// Encoder used (empty for GIF).
    pub encoder: String,
    pub bytes: u64,
}

/// Frames whose start time falls inside `[start, end)` at `fps`.
fn frame_span(fps: FrameRate, start: Time, end: Time) -> (i64, i64) {
    let mut first = fps.time_to_frame(start);
    if fps.frame_to_time(first) < start {
        first += 1;
    }
    let mut last = fps.time_to_frame(end);
    if fps.frame_to_time(last) >= end {
        last -= 1;
    }
    (first, (last - first + 1).max(1))
}

/// Renders `project` as `settings` asks. `progress` receives `0.0..=1.0`.
/// On error or cancellation no partial file is left behind.
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
    let (path, frames, encoder) = match settings.format {
        ExportFormat::Video { codec } => export_video(
            project,
            cache,
            gpu,
            settings,
            codec,
            (start, end),
            cancel,
            progress,
        )?,
        ExportFormat::Gif { fps } => export_gif(
            project,
            cache,
            gpu,
            settings,
            fps,
            (start, end),
            cancel,
            progress,
        )?,
        ExportFormat::Mp3 => export_mp3(project, settings, (start, end), cancel, progress)?,
    };
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Ok(ExportReport {
        path,
        frames,
        duration: end - start,
        encoder,
        bytes,
    })
}

#[allow(clippy::too_many_arguments)]
fn export_video(
    project: &Project,
    cache: &ProjectCache,
    gpu: &Gpu,
    settings: &ExportSettings,
    codec: VideoCodec,
    (start, end): (Time, Time),
    cancel: &AtomicBool,
    progress: &dyn Fn(f32),
) -> Result<(PathBuf, i64, String), ExportError> {
    let fps = settings.frame_rate.unwrap_or(project.canvas.frame_rate);
    let (first, frames) = frame_span(fps, start, end);
    let size = (settings.width, settings.height);
    let mut writer = Mp4Writer::create(
        &settings.path,
        &VideoSettings {
            width: size.0,
            height: size.1,
            frame_rate: fps,
            bit_rate: settings.video_bit_rate,
            encoder: settings.encoder.clone(),
            codec,
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
            Ok((path, frames, encoder))
        }
        Err(e) => {
            writer.abort();
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn export_gif(
    project: &Project,
    cache: &ProjectCache,
    gpu: &Gpu,
    settings: &ExportSettings,
    fps: u32,
    (start, end): (Time, Time),
    cancel: &AtomicBool,
    progress: &dyn Fn(f32),
) -> Result<(PathBuf, i64, String), ExportError> {
    let rate = FrameRate::new(fps.max(1), 1);
    let (first, frames) = frame_span(rate, start, end);
    let size = (settings.width, settings.height);
    let mut writer = GifWriter::create(&settings.path, size, fps)?;
    let mut source = FrameSource::new(cache.clone(), false);
    let mut compositor = Compositor::new(gpu, size);
    let result = (|| -> Result<(), ExportError> {
        for i in 0..frames {
            if cancel.load(Ordering::Relaxed) {
                return Err(ExportError::Cancelled);
            }
            let t = rate.frame_to_time(first + i);
            let layers = source.layers_at(project, t, size);
            compositor.render(project.canvas.background, &layers);
            writer.write(&compositor.read_back()?)?;
            progress((i + 1) as f32 / frames as f32);
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok((writer.finish()?, frames, String::new())),
        Err(e) => {
            writer.abort();
            Err(e)
        }
    }
}

fn export_mp3(
    project: &Project,
    settings: &ExportSettings,
    (start, end): (Time, Time),
    cancel: &AtomicBool,
    progress: &dyn Fn(f32),
) -> Result<(PathBuf, i64, String), ExportError> {
    let mut writer = AudioWriter::create_mp3(&settings.path, settings.audio_bit_rate)?;
    let mut mixer = Mixer::new();
    let (s0, s1) = (time_to_sample(start), time_to_sample(end));
    // A tenth of a second at a time.
    let block = SAMPLE_RATE as i64 / 10;
    let mut buf = Vec::new();
    let result = (|| -> Result<(), ExportError> {
        let mut at = s0;
        while at < s1 {
            if cancel.load(Ordering::Relaxed) {
                return Err(ExportError::Cancelled);
            }
            let n = block.min(s1 - at);
            buf.resize(n as usize * CHANNELS, 0.0);
            mixer.mix(project, at, &mut buf);
            for s in buf.iter_mut() {
                *s = s.clamp(-1.0, 1.0);
            }
            writer.write(&buf)?;
            at += n;
            progress(((at - s0) as f64 / (s1 - s0) as f64) as f32);
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok((writer.finish()?, 0, "libmp3lame".into())),
        Err(e) => {
            writer.abort();
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolutions_keep_the_shape_with_even_sides() {
        assert_eq!(Resolution::P1080.size_for((1920, 1080)), (1920, 1080));
        assert_eq!(Resolution::P720.size_for((1920, 1080)), (1280, 720));
        assert_eq!(Resolution::P480.size_for((1920, 1080)), (854, 480));
        // Upright video: the short side is the width.
        assert_eq!(Resolution::P1080.size_for((1080, 1920)), (1080, 1920));
        assert_eq!(Resolution::P2160.size_for((1080, 1080)), (2160, 2160));
        assert!(Resolution::P2160.upscales((1920, 1080)));
        assert!(!Resolution::P720.upscales((1920, 1080)));
    }

    #[test]
    fn bit_rates_follow_size_quality_and_codec() {
        let hd = (1920, 1080);
        let r = |q, c| bit_rate(hd, FrameRate::FPS_30, q, c);
        assert!((7_000_000..9_000_000).contains(&r(Quality::Balanced, VideoCodec::H264)));
        assert!(r(Quality::Higher, VideoCodec::H264) > r(Quality::Balanced, VideoCodec::H264));
        assert!(r(Quality::Balanced, VideoCodec::Av1) < r(Quality::Balanced, VideoCodec::H264));
    }
}
