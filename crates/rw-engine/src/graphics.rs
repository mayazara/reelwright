//! Items with a natural size: text, stickers and shapes.
//!
//! Their transform's centre places them and its size scales the natural
//! size (`[1, 1]` = natural). Text is laid out at its font size; stickers
//! and shapes are drawn at the size they're shown so they stay sharp.

use rw_audio::spectrum::Levels;
use rw_media::RgbaImage;
use rw_model::{Fit, Item, ItemKind, SHAPE_HEIGHT, STICKER_HEIGHT, Time, VISUALISER_HEIGHT};
use rw_render::Layer;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

/// Converted images kept so a still graphic isn't copied every frame.
const KEPT: usize = 96;

type Kept = (HashMap<u64, Arc<RgbaImage>>, VecDeque<u64>);

fn converted(key: u64, w: u32, h: u32, pixels: &[u8]) -> Arc<RgbaImage> {
    static KEEP: OnceLock<Mutex<Kept>> = OnceLock::new();
    let mut keep = KEEP
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(i) = keep.0.get(&key) {
        return i.clone();
    }
    let i = Arc::new(RgbaImage::new(w, h, pixels.to_vec()));
    keep.0.insert(key, i.clone());
    keep.1.push_back(key);
    while keep.1.len() > KEPT {
        if let Some(old) = keep.1.pop_front() {
            keep.0.remove(&old);
        }
    }
    i
}

/// Where a natural-size item sits at `local` into it on a canvas of
/// `canvas` pixels: centre and size as shares of the canvas.
pub struct Placed {
    pub center: [f32; 2],
    pub size: [f32; 2],
    pub opacity: f32,
    pub key: u64,
    pub image: Arc<RgbaImage>,
}

/// Lays out a text, sticker, shape or visualiser item for a canvas
/// `canvas` in size. A visualiser draws `levels` (flat without them).
pub fn place(
    item: &Item,
    local: Time,
    canvas: (u32, u32),
    levels: Option<&Levels>,
) -> Option<Placed> {
    let t = item.look.transform;
    let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
    match &item.kind {
        ItemKind::Text { .. } => {
            let p = crate::text::place(item, local, canvas)?;
            Some(Placed {
                center: p.center,
                size: p.size,
                opacity: p.opacity,
                key: p.key,
                image: converted(p.key, p.image.width, p.image.height, &p.image.pixels),
            })
        }
        ItemKind::Sticker { sticker } => {
            let px = (STICKER_HEIGHT * ch * t.size[1].abs()).round().max(2.0) as u32;
            let (key, r) = rw_assets::raster::sticker(sticker, px)?;
            Some(Placed {
                center: t.center,
                size: [
                    r.width as f32 / cw * (t.size[0] / t.size[1].max(1e-3)),
                    r.height as f32 / ch,
                ],
                opacity: 1.0,
                key,
                image: converted(key, r.width, r.height, &r.pixels),
            })
        }
        ItemKind::Shape { shape } => {
            // Drawn at the shown size, so stretching changes the shape's
            // proportions without blurring it.
            let px = (SHAPE_HEIGHT * ch * t.size[1].abs()).round().max(2.0) as u32;
            let mut spec = shape.clone();
            spec.aspect *= t.size[0] / t.size[1].max(1e-3);
            let (key, r) = rw_assets::raster::shape(&spec.sanitized(), px)?;
            Some(Placed {
                center: t.center,
                size: [r.width as f32 / cw, r.height as f32 / ch],
                opacity: 1.0,
                key,
                image: converted(key, r.width, r.height, &r.pixels),
            })
        }
        ItemKind::Visualiser { visualiser } => {
            let px = (VISUALISER_HEIGHT * ch * t.size[1].abs()).round().max(2.0) as u32;
            let flat = Levels::default();
            let l = levels.unwrap_or(&flat);
            let bands: Vec<f32> = if l.bands.is_empty() {
                vec![0.0; usize::from(visualiser.bars)]
            } else {
                l.bands.clone()
            };
            let (key, r) = rw_assets::raster::visualiser(visualiser, &bands, &l.wave, px)?;
            Some(Placed {
                center: t.center,
                size: [
                    r.width as f32 / cw * (t.size[0] / t.size[1].max(1e-3)),
                    r.height as f32 / ch,
                ],
                opacity: 1.0,
                key,
                image: Arc::new(RgbaImage::new(r.width, r.height, r.pixels.clone())),
            })
        }
        _ => None,
    }
}

/// The compositor layer for a natural-size item at `local`, drawn for an
/// output `out` pixels in size.
pub fn layer(item: &Item, local: Time, out: (u32, u32), levels: Option<&Levels>) -> Option<Layer> {
    let p = place(item, local, out, levels)?;
    let mut layer = Layer::image(p.image, p.key, 0);
    layer.look = item.look.clone();
    layer.look.transform.center = p.center;
    layer.look.transform.size = p.size;
    layer.look.transform.fit = Fit::Stretch;
    layer.look.opacity *= p.opacity;
    Some(layer)
}
