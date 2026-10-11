//! Compositor: draws a list of layers into a canvas-sized frame on the GPU.
//!
//! Layers are blended bottom to top in linear light into an `Rgba16Float`
//! canvas, then encoded to an sRGB `Rgba8UnormSrgb` output that the preview
//! shows directly and export reads back. Preview and export share this code;
//! only the output size differs.

mod compositor;
mod gpu;

pub use compositor::{Compositor, Layer, OUTPUT_FORMAT, Source};
pub use gpu::Gpu;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no usable GPU adapter: {0}")]
    NoAdapter(String),
    #[error("GPU device error: {0}")]
    Device(String),
    #[error("reading the frame back failed: {0}")]
    Readback(String),
}

/// Rectangle `(x, y, w, h)` in output pixels where a source of display size
/// `src` is drawn to fit entirely inside `dst`, centred (letterboxed).
pub fn fit_contain(src: (u32, u32), dst: (u32, u32)) -> (f32, f32, f32, f32) {
    let (sw, sh) = (src.0.max(1) as f32, src.1.max(1) as f32);
    let (dw, dh) = (dst.0 as f32, dst.1 as f32);
    let scale = (dw / sw).min(dh / sh);
    let (w, h) = (sw * scale, sh * scale);
    ((dw - w) / 2.0, (dh - h) / 2.0, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contain_letterboxes_and_pillarboxes() {
        assert_eq!(
            fit_contain((1920, 1080), (1920, 1080)),
            (0.0, 0.0, 1920.0, 1080.0)
        );
        // 4:3 in 16:9: pillarbox.
        assert_eq!(
            fit_contain((1440, 1080), (1920, 1080)),
            (240.0, 0.0, 1440.0, 1080.0)
        );
        // Vertical video in 16:9.
        let (x, y, w, h) = fit_contain((1080, 1920), (1920, 1080));
        assert_eq!((y, h), (0.0, 1080.0));
        assert!((w - 607.5).abs() < 0.01 && (x - 656.25).abs() < 0.01);
        // Small source scales up.
        assert_eq!(
            fit_contain((160, 90), (1920, 1080)),
            (0.0, 0.0, 1920.0, 1080.0)
        );
    }
}
