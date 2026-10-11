//! Thumbnail textures loaded from PNG files on demand.

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use rw_media::RgbaImage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct Textures {
    loaded: HashMap<PathBuf, Option<TextureHandle>>,
}

impl Textures {
    /// Texture for a PNG on disk, loading it the first time. Returns `None`
    /// if the file is missing or unreadable (and remembers that until
    /// [`Self::forget`]).
    pub fn get(&mut self, ctx: &egui::Context, path: &Path) -> Option<TextureHandle> {
        self.loaded
            .entry(path.to_path_buf())
            .or_insert_with(|| {
                let img = RgbaImage::load_png(path).ok()?;
                let color = ColorImage::from_rgba_unmultiplied(
                    [img.width as usize, img.height as usize],
                    &img.pixels,
                );
                Some(ctx.load_texture(path.display().to_string(), color, TextureOptions::LINEAR))
            })
            .clone()
    }

    /// Drops a cached texture so the file is read again next time.
    pub fn forget(&mut self, path: &Path) {
        self.loaded.remove(path);
    }

    pub fn clear(&mut self) {
        self.loaded.clear();
    }
}
