//! Media I/O behind the [`MediaIo`] trait: probe, decode, encode, mux,
//! proxies, thumbnails, waveforms.
//!
//! Nothing outside this crate touches FFmpeg, so the binding can be swapped.

pub mod decode;
pub mod encode;
mod ffmpeg;
#[cfg(feature = "fixtures")]
pub mod fixtures;
pub mod peaks;
mod rgba;

pub use decode::{AudioReader, VideoFrame, VideoReader};
pub use encode::{AudioSettings, Mp4Writer, VideoSettings};
pub use ffmpeg::Ffmpeg;
pub use peaks::Peaks;
pub use rgba::RgbaImage;

use rw_model::{MediaInfo, MediaKind, Time};
use std::path::Path;
use std::sync::atomic::AtomicBool;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("could not open {path}: {reason}")]
    Open { path: String, reason: String },
    #[error("unsupported file: {0}")]
    Unsupported(String),
    #[error("no video in this file")]
    NoVideo,
    #[error("decoding failed: {0}")]
    Decode(String),
    #[error("encoding failed: {0}")]
    Encode(String),
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Reports fractional progress in `0.0..=1.0`.
pub type Progress<'a> = &'a (dyn Fn(f32) + Sync);

/// Everything the app needs from a media library.
pub trait MediaIo: Send + Sync {
    /// Reads container and stream information.
    fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError>;

    /// Decodes the frame at (or just after) `at`, scaled to fit within
    /// `max_size` × `max_size` and rotated for display.
    fn thumbnail(&self, path: &Path, at: Time, max_size: u32) -> Result<RgbaImage, MediaError>;

    /// Writes a lightweight intra-frame proxy of the video stream, at most
    /// `max_height` lines tall (after rotation). Deletes partial output on
    /// error or cancellation.
    fn make_proxy(
        &self,
        path: &Path,
        out: &Path,
        max_height: u32,
        cancel: &AtomicBool,
        progress: Progress<'_>,
    ) -> Result<(), MediaError>;
}

/// Codecs that are expensive to decode in software at preview rates.
const HEAVY_CODECS: &[&str] = &["hevc", "av1"];

/// Height of generated proxies.
pub const PROXY_HEIGHT: u32 = 540;

/// Whether a file should get a proxy for smooth preview: anything above
/// 1080p, or encoded with a codec that is slow to decode in software.
pub fn needs_proxy(info: &MediaInfo) -> bool {
    if info.kind != MediaKind::Video {
        return false;
    }
    let Some(v) = &info.video else { return false };
    let (w, h) = v.display_size();
    w.max(h) > 1920 || w.min(h) > 1080 || HEAVY_CODECS.contains(&v.codec.as_str())
}

/// Where in a clip to take its thumbnail: one second in, or 10% of the way
/// for clips shorter than ten seconds.
pub fn thumbnail_time(info: &MediaInfo) -> Time {
    if info.duration <= Time::ZERO {
        return Time::ZERO;
    }
    Time::from_secs(1).min(Time(info.duration.ticks() / 10))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rw_model::{FrameRate, VideoInfo};

    fn video(codec: &str, w: u32, h: u32, rotation: u16) -> MediaInfo {
        MediaInfo {
            kind: MediaKind::Video,
            container: "mp4".into(),
            duration: Time::from_secs(20),
            video: Some(VideoInfo {
                codec: codec.into(),
                width: w,
                height: h,
                frame_rate: FrameRate::FPS_30,
                pixel_format: "yuv420p".into(),
                has_alpha: false,
                rotation,
            }),
            audio: None,
        }
    }

    #[test]
    fn proxy_policy() {
        assert!(!needs_proxy(&video("h264", 1920, 1080, 0)));
        assert!(!needs_proxy(&video("h264", 1920, 1080, 90)));
        assert!(needs_proxy(&video("h264", 3840, 2160, 0)));
        assert!(needs_proxy(&video("hevc", 1280, 720, 0)));
        assert!(!needs_proxy(&video("prores", 1920, 1080, 0)));
    }

    #[test]
    fn thumbnail_time_policy() {
        assert_eq!(thumbnail_time(&video("h264", 1, 1, 0)), Time::from_secs(1));
        let mut short = video("h264", 1, 1, 0);
        short.duration = Time::from_secs(2);
        assert_eq!(thumbnail_time(&short), Time::from_millis(200));
    }
}
