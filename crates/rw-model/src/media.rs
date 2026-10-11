//! Imported media references and what probing found out about them.

use crate::ids::MediaId;
use crate::time::{FrameRate, Time};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Broad category used by the media bin's type filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Video,
    Audio,
    Image,
    /// Animated image (GIF, APNG). Behaves like a silent video.
    Animation,
}

impl MediaKind {
    pub const ALL: [MediaKind; 4] = [
        MediaKind::Video,
        MediaKind::Audio,
        MediaKind::Image,
        MediaKind::Animation,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MediaKind::Video => "Video",
            MediaKind::Audio => "Audio",
            MediaKind::Image => "Image",
            MediaKind::Animation => "Animated image",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoInfo {
    pub codec: String,
    /// Coded size, before rotation.
    pub width: u32,
    pub height: u32,
    pub frame_rate: FrameRate,
    pub pixel_format: String,
    pub has_alpha: bool,
    /// Clockwise display rotation in degrees (0, 90, 180, 270).
    #[serde(default)]
    pub rotation: u16,
}

impl VideoInfo {
    /// Size as displayed, after applying rotation.
    pub fn display_size(&self) -> (u32, u32) {
        if self.rotation % 180 == 90 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Result of probing a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub kind: MediaKind,
    pub container: String,
    /// Zero for still images.
    pub duration: Time,
    pub video: Option<VideoInfo>,
    pub audio: Option<AudioInfo>,
}

/// Content fingerprint used to relink moved files.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentHash(pub String);

impl ContentHash {
    /// Bytes hashed from each end of a file. Hashing whole multi-gigabyte
    /// files on import would be slow; size plus head and tail is enough to
    /// recognise the same file after a move.
    pub const SAMPLE: u64 = 1 << 20;

    pub fn of_file(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let mut hasher = blake3::Hasher::new();
        hasher.update(&len.to_le_bytes());
        let mut buf = vec![0u8; Self::SAMPLE.min(len) as usize];
        file.read_exact(&mut buf)?;
        hasher.update(&buf);
        if len > Self::SAMPLE {
            let tail = Self::SAMPLE.min(len - Self::SAMPLE);
            file.seek(SeekFrom::End(-(tail as i64)))?;
            buf.resize(tail as usize, 0);
            file.read_exact(&mut buf)?;
            hasher.update(&buf);
        }
        Ok(ContentHash(hasher.finalize().to_hex()[..32].to_owned()))
    }

    /// Short form for cache file names.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A file imported into the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaRef {
    pub id: MediaId,
    /// Display name; defaults to the file name.
    pub name: String,
    /// Absolute path at import time.
    pub path: PathBuf,
    pub hash: ContentHash,
    pub size_bytes: u64,
    pub info: MediaInfo,
    /// Seconds since the Unix epoch when it was imported.
    pub imported_at: u64,
}

impl MediaRef {
    pub fn kind(&self) -> MediaKind {
        self.info.kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn hash_ignores_location_but_not_content() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        let data: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        File::create(&a).unwrap().write_all(&data).unwrap();
        File::create(&b).unwrap().write_all(&data).unwrap();
        assert_eq!(
            ContentHash::of_file(&a).unwrap(),
            ContentHash::of_file(&b).unwrap()
        );

        let mut changed = data.clone();
        *changed.last_mut().unwrap() ^= 1;
        File::create(&b).unwrap().write_all(&changed).unwrap();
        assert_ne!(
            ContentHash::of_file(&a).unwrap(),
            ContentHash::of_file(&b).unwrap()
        );
    }

    #[test]
    fn hash_small_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        std::fs::write(&a, b"hi").unwrap();
        assert_eq!(ContentHash::of_file(&a).unwrap().0.len(), 32);
    }

    #[test]
    fn rotated_display_size() {
        let v = VideoInfo {
            codec: "h264".into(),
            width: 1920,
            height: 1080,
            frame_rate: FrameRate::FPS_30,
            pixel_format: "yuv420p".into(),
            has_alpha: false,
            rotation: 90,
        };
        assert_eq!(v.display_size(), (1080, 1920));
    }
}
