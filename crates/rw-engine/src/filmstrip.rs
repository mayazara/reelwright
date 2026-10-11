//! Filmstrips: small frames sampled through a video, packed in one image,
//! so timeline clips can show what's at each point.

use rw_cache::ProjectCache;
use rw_media::{MediaError, RgbaImage, VideoReader};
use rw_model::{MediaKind, MediaRef, Time};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// Tile height in pixels; width follows the video's shape.
pub const TILE_HEIGHT: u32 = 64;
/// At most this many tiles per file; long files sample more sparsely.
pub const MAX_TILES: u32 = 240;
/// Tiles per row in the packed image (keeps it within GPU texture limits).
pub const COLUMNS: u32 = 16;

/// Where each tile is in the packed image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilmstripInfo {
    /// Source time between tiles; tile `n` shows time `n · interval`.
    pub interval: Time,
    pub count: u32,
    pub tile_width: u32,
    pub tile_height: u32,
    pub columns: u32,
}

impl FilmstripInfo {
    /// Tile showing source time `t`.
    pub fn tile_at(&self, t: Time) -> u32 {
        let n = t.max(Time::ZERO).ticks() / self.interval.ticks().max(1);
        (n.max(0) as u32).min(self.count.saturating_sub(1))
    }

    /// Pixel rectangle `(x, y, w, h)` of tile `n` in the packed image.
    pub fn tile_rect(&self, n: u32) -> (u32, u32, u32, u32) {
        (
            (n % self.columns) * self.tile_width,
            (n / self.columns) * self.tile_height,
            self.tile_width,
            self.tile_height,
        )
    }

    pub fn image_size(&self) -> (u32, u32) {
        let rows = self.count.div_ceil(self.columns);
        (
            self.columns.min(self.count) * self.tile_width,
            rows * self.tile_height,
        )
    }
}

/// Cache paths of a media file's filmstrip: the image and its layout.
pub fn paths(cache: &ProjectCache, media: &MediaRef) -> (PathBuf, PathBuf) {
    let dir = cache.root().join("filmstrips");
    (
        dir.join(format!("{}.png", media.hash.as_str())),
        dir.join(format!("{}.json", media.hash.as_str())),
    )
}

pub fn load_info(cache: &ProjectCache, media: &MediaRef) -> Option<FilmstripInfo> {
    let (_, info) = paths(cache, media);
    serde_json::from_slice(&std::fs::read(info).ok()?).ok()
}

/// Builds the filmstrip if missing. Only videos and animations have one.
pub fn ensure(
    cache: &ProjectCache,
    media: &MediaRef,
    cancel: &AtomicBool,
) -> Result<Option<PathBuf>, MediaError> {
    let Some(v) = &media.info.video else {
        return Ok(None);
    };
    if !matches!(media.kind(), MediaKind::Video | MediaKind::Animation)
        || media.info.duration <= Time::ZERO
    {
        return Ok(None);
    }
    let (png, json) = paths(cache, media);
    if png.exists() && json.exists() {
        return Ok(Some(png));
    }
    let (dw, dh) = v.display_size();
    let tile_height = TILE_HEIGHT;
    let tile_width =
        ((tile_height as f32 * dw as f32 / dh.max(1) as f32).round() as u32).clamp(8, 256);
    // One tile per second, fewer for long files.
    let secs = media.info.duration.as_secs_f64();
    let count = (secs.ceil() as u32).clamp(1, MAX_TILES);
    let interval = Time::from_secs_f64(secs / f64::from(count));
    let info = FilmstripInfo {
        interval,
        count,
        tile_width,
        tile_height,
        columns: COLUMNS,
    };
    // Decode in coded orientation, then rotate each tile for display.
    let coded = if v.rotation % 180 == 90 {
        (tile_height, tile_width)
    } else {
        (tile_width, tile_height)
    };
    let mut reader = VideoReader::open(&media.path, Some(coded), false)?;
    let (w, h) = info.image_size();
    let mut sheet = vec![0u8; (w * h * 4) as usize];
    for n in 0..count {
        if cancel.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        let t = Time(interval.ticks() * i64::from(n)) + Time(interval.ticks() / 2);
        let frame = reader.frame_at(t)?;
        let tile = frame.image.rotated(v.rotation);
        let (x0, y0, tw, th) = info.tile_rect(n);
        for row in 0..th.min(tile.height) {
            let src = (row * tile.width * 4) as usize;
            let dst = (((y0 + row) * w + x0) * 4) as usize;
            let len = (tw.min(tile.width) * 4) as usize;
            sheet[dst..dst + len].copy_from_slice(&tile.pixels[src..src + len]);
        }
    }
    RgbaImage::new(w, h, sheet).save_png(&png)?;
    std::fs::write(
        &json,
        serde_json::to_vec(&info).map_err(std::io::Error::other)?,
    )?;
    Ok(Some(png))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_lookup() {
        let info = FilmstripInfo {
            interval: Time::from_secs(2),
            count: 20,
            tile_width: 100,
            tile_height: 64,
            columns: 16,
        };
        assert_eq!(info.tile_at(Time::from_millis(4500)), 2);
        assert_eq!(info.tile_at(Time::from_secs(999)), 19);
        assert_eq!(info.tile_at(-Time::from_secs(1)), 0);
        assert_eq!(info.tile_rect(17), (100, 64, 100, 64));
        assert_eq!(info.image_size(), (1600, 128));
    }
}
