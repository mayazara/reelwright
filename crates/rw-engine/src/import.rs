//! Turning files on disk into [`MediaRef`]s and preparing their cache files.

use rw_cache::ProjectCache;
use rw_media::{MediaError, MediaIo, PROXY_HEIGHT, needs_proxy, thumbnail_time};
use rw_model::{ContentHash, MediaId, MediaKind, MediaRef};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{SystemTime, UNIX_EPOCH};

/// Longest side of media thumbnails, in pixels.
pub const THUMBNAIL_SIZE: u32 = 320;

/// File extensions picked up when importing a whole folder. Files picked
/// individually or dropped are probed whatever their extension.
pub const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "mpg", "mpeg", "ts", "mts", "m2ts", "wmv", "flv",
    "3gp", "ogv", "mxf", "png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "gif", "apng", "mp3",
    "wav", "m4a", "aac", "flac", "ogg", "opus", "wma", "aif", "aiff",
];

pub fn has_media_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| MEDIA_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Recursively lists media files under `dir`, sorted by path. Hidden files
/// and folders are skipped.
pub fn scan_folder(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let ty = entry.file_type()?;
            if ty.is_dir() {
                stack.push(path);
            } else if ty.is_file() && has_media_extension(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Probes and fingerprints one file.
pub fn probe_file(io: &dyn MediaIo, path: &Path) -> Result<MediaRef, MediaError> {
    let path = std::path::absolute(path)?;
    let info = io.probe(&path)?;
    let size_bytes = std::fs::metadata(&path)?.len();
    let hash = ContentHash::of_file(&path)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    Ok(MediaRef {
        id: MediaId::new(),
        name,
        path,
        hash,
        size_bytes,
        info,
        imported_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    })
}

/// Writes the thumbnail if it is missing. Returns its path, or `None` for
/// media without pictures.
pub fn ensure_thumbnail(
    io: &dyn MediaIo,
    cache: &ProjectCache,
    media: &MediaRef,
) -> Result<Option<PathBuf>, MediaError> {
    if media.kind() == MediaKind::Audio {
        return Ok(None);
    }
    let path = cache.thumbnail(&media.hash);
    if !path.exists() {
        let img = io.thumbnail(&media.path, thumbnail_time(&media.info), THUMBNAIL_SIZE)?;
        img.save_png(&path)?;
    }
    Ok(Some(path))
}

/// Computes waveform peaks if the media has sound and they are missing.
pub fn ensure_peaks(
    cache: &ProjectCache,
    media: &MediaRef,
    cancel: &AtomicBool,
) -> Result<Option<PathBuf>, MediaError> {
    if media.info.audio.is_none() {
        return Ok(None);
    }
    let path = cache.peaks(&media.hash);
    if !path.exists() {
        rw_media::Peaks::compute(&media.path, cancel)?.save(&path)?;
    }
    Ok(Some(path))
}

/// Writes the preview proxy if the media needs one and it is missing.
pub fn ensure_proxy(
    io: &dyn MediaIo,
    cache: &ProjectCache,
    media: &MediaRef,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f32) + Sync),
) -> Result<Option<PathBuf>, MediaError> {
    if !needs_proxy(&media.info) {
        return Ok(None);
    }
    let path = cache.proxy(&media.hash);
    if !path.exists() {
        io.make_proxy(&media.path, &path, PROXY_HEIGHT, cancel, progress)?;
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_filter() {
        assert!(has_media_extension(Path::new("a/B.MOV")));
        assert!(has_media_extension(Path::new("song.flac")));
        assert!(!has_media_extension(Path::new("notes.txt")));
        assert!(!has_media_extension(Path::new("noext")));
    }

    #[test]
    fn scan_skips_hidden_and_non_media() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::create_dir_all(d.join(".hidden")).unwrap();
        for f in ["a.mp4", "sub/b.wav", "c.txt", ".d.mp4", ".hidden/e.mp4"] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        let found = scan_folder(d).unwrap();
        let rel: Vec<_> = found
            .iter()
            .map(|p| {
                p.strip_prefix(d)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(rel, vec!["a.mp4", "sub/b.wav"]);
    }
}
