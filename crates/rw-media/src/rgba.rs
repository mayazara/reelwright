//! A simple owned RGBA8 image.

use std::io;
use std::path::Path;

#[derive(Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    /// Row-major, 4 bytes per pixel, no padding.
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for RgbaImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RgbaImage({}x{})", self.width, self.height)
    }
}

impl RgbaImage {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        assert_eq!(pixels.len(), width as usize * height as usize * 4);
        RgbaImage {
            width,
            height,
            pixels,
        }
    }

    fn px(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels[i..i + 4].try_into().expect("4 bytes")
    }

    /// Rotates clockwise by a multiple of 90 degrees.
    pub fn rotated(&self, degrees_cw: u16) -> RgbaImage {
        let (w, h) = (self.width, self.height);
        let turns = (degrees_cw / 90) % 4;
        if turns == 0 {
            return self.clone();
        }
        let (nw, nh) = if turns % 2 == 1 { (h, w) } else { (w, h) };
        let mut out = Vec::with_capacity(self.pixels.len());
        for y in 0..nh {
            for x in 0..nw {
                let (sx, sy) = match turns {
                    1 => (y, h - 1 - x),
                    2 => (w - 1 - x, h - 1 - y),
                    _ => (w - 1 - y, x),
                };
                out.extend_from_slice(&self.px(sx, sy));
            }
        }
        RgbaImage::new(nw, nh, out)
    }

    pub fn save_png(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("png.tmp");
        image::save_buffer_with_format(
            &tmp,
            &self.pixels,
            self.width,
            self.height,
            image::ExtendedColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .map_err(io::Error::other)?;
        std::fs::rename(tmp, path)
    }

    pub fn load_png(path: &Path) -> io::Result<RgbaImage> {
        let img = image::open(path).map_err(io::Error::other)?.into_rgba8();
        let (w, h) = img.dimensions();
        Ok(RgbaImage::new(w, h, img.into_raw()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2x1 image: red, green.
    fn sample() -> RgbaImage {
        RgbaImage::new(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255])
    }

    #[test]
    fn rotate_90_moves_left_pixel_to_top() {
        let r = sample().rotated(90);
        assert_eq!((r.width, r.height), (1, 2));
        assert_eq!(r.px(0, 0), [255, 0, 0, 255]);
        assert_eq!(r.px(0, 1), [0, 255, 0, 255]);
    }

    #[test]
    fn four_quarter_turns_are_identity() {
        let img = RgbaImage::new(3, 2, (0..24).collect());
        let back = img.rotated(90).rotated(90).rotated(90).rotated(90);
        assert_eq!(back, img);
        assert_eq!(img.rotated(180).rotated(180), img);
        assert_eq!(img.rotated(270), img.rotated(90).rotated(180));
    }

    #[test]
    fn png_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a/b.png");
        sample().save_png(&p).unwrap();
        assert_eq!(RgbaImage::load_png(&p).unwrap(), sample());
    }
}
