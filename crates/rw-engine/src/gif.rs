//! Animated GIF output, quantised per frame in Rust (FFmpeg's palette
//! filters aren't in the trimmed build).

use rw_media::RgbaImage;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

/// Quantiser effort: 1 (best) to 30 (fastest); 10 is a good balance.
const SPEED: i32 = 10;

pub struct GifWriter {
    encoder: gif::Encoder<BufWriter<File>>,
    path: PathBuf,
    partial: PathBuf,
    fps: u32,
    frames: u64,
}

impl std::fmt::Debug for GifWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GifWriter")
            .field("path", &self.path)
            .finish()
    }
}

impl GifWriter {
    /// A looping GIF of `size` at `fps` frames a second.
    pub fn create(path: &Path, size: (u32, u32), fps: u32) -> std::io::Result<Self> {
        let mut partial = path.as_os_str().to_owned();
        partial.push(".part");
        let partial = PathBuf::from(partial);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let w = u16::try_from(size.0).map_err(std::io::Error::other)?;
        let h = u16::try_from(size.1).map_err(std::io::Error::other)?;
        let file = BufWriter::new(File::create(&partial)?);
        let mut encoder = gif::Encoder::new(file, w, h, &[]).map_err(std::io::Error::other)?;
        encoder
            .set_repeat(gif::Repeat::Infinite)
            .map_err(std::io::Error::other)?;
        Ok(GifWriter {
            encoder,
            path: path.to_path_buf(),
            partial,
            fps: fps.max(1),
            frames: 0,
        })
    }

    /// Appends a frame. Frame delays are whole hundredths of a second, so
    /// they're spread to keep the total length exact.
    pub fn write(&mut self, image: &RgbaImage) -> std::io::Result<()> {
        let at = |n: u64| (n * 100 + u64::from(self.fps) / 2) / u64::from(self.fps);
        let delay = at(self.frames + 1) - at(self.frames);
        let mut pixels = image.pixels.clone();
        let mut frame = gif::Frame::from_rgba_speed(
            image.width as u16,
            image.height as u16,
            &mut pixels,
            SPEED,
        );
        frame.delay = delay as u16;
        self.encoder
            .write_frame(&frame)
            .map_err(std::io::Error::other)?;
        self.frames += 1;
        Ok(())
    }

    pub fn finish(self) -> std::io::Result<PathBuf> {
        let GifWriter {
            encoder,
            path,
            partial,
            ..
        } = self;
        let mut file = encoder.into_inner().map_err(std::io::Error::other)?;
        std::io::Write::flush(&mut file)?;
        drop(file);
        std::fs::rename(&partial, &path)?;
        Ok(path)
    }

    pub fn abort(self) {
        let partial = self.partial.clone();
        drop(self);
        let _ = std::fs::remove_file(partial);
    }
}
