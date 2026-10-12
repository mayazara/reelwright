//! Drawing stickers and shapes into images at a given size. Cached.

use crate::stickers::{self, Format};
use resvg::tiny_skia::{
    self, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform,
};
use resvg::usvg;
use rw_model::{Rgba, ShapeKind, ShapeSpec, StickerRef};
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

/// A drawn graphic: straight-alpha sRGB RGBA8.
#[derive(Debug, Clone, PartialEq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Raster {
    fn from_pixmap(p: Pixmap) -> Raster {
        let (width, height) = (p.width(), p.height());
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for px in p.pixels() {
            let c = px.demultiply();
            pixels.extend([c.red(), c.green(), c.blue(), c.alpha()]);
        }
        Raster {
            width,
            height,
            pixels,
        }
    }
}

const KEPT: usize = 128;

type Cache = (HashMap<u64, Arc<Raster>>, VecDeque<u64>);

fn cached(key: u64, make: impl FnOnce() -> Option<Raster>) -> Option<(u64, Arc<Raster>)> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(r) = cache.lock().unwrap_or_else(|e| e.into_inner()).0.get(&key) {
        return Some((key, r.clone()));
    }
    let r = Arc::new(make()?);
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
    c.0.insert(key, r.clone());
    c.1.push_back(key);
    while c.1.len() > KEPT {
        if let Some(old) = c.1.pop_front() {
            c.0.remove(&old);
        }
    }
    Some((key, r))
}

fn hash(parts: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

/// Draws `sticker` `height` pixels tall (width follows its shape). `None`
/// when the pack or picture is missing or unreadable.
pub fn sticker(sticker: &StickerRef, height: u32) -> Option<(u64, Arc<Raster>)> {
    let height = height.clamp(2, 4096);
    cached(hash(("sticker", sticker, height)), || {
        let (data, format) = stickers::data(&sticker.pack, &sticker.id)?;
        match format {
            Format::Svg => {
                let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).ok()?;
                let size = tree.size();
                let scale = height as f32 / size.height().max(1.0);
                let width = (size.width() * scale).round().max(1.0) as u32;
                let mut pixmap = Pixmap::new(width, height)?;
                resvg::render(
                    &tree,
                    Transform::from_scale(scale, scale),
                    &mut pixmap.as_mut(),
                );
                Some(Raster::from_pixmap(pixmap))
            }
            Format::Png => {
                let img = image::load_from_memory_with_format(&data, image::ImageFormat::Png)
                    .ok()?
                    .to_rgba8();
                let scale = height as f32 / img.height().max(1) as f32;
                let width = (img.width() as f32 * scale).round().max(1.0) as u32;
                let img = image::imageops::resize(
                    &img,
                    width,
                    height,
                    image::imageops::FilterType::Triangle,
                );
                Some(Raster {
                    width,
                    height,
                    pixels: img.into_raw(),
                })
            }
        }
    })
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.0[0], c.0[1], c.0[2], c.0[3]);
    p.anti_alias = true;
    p
}

/// The outline of a filled shape in a `w`×`h` box inset by `inset`.
fn outline(spec: &ShapeSpec, w: f32, h: f32, inset: f32) -> Option<tiny_skia::Path> {
    let (x0, y0, x1, y1) = (inset, inset, w - inset, h - inset);
    let (bw, bh) = (x1 - x0, y1 - y0);
    let mut pb = PathBuilder::new();
    match spec.kind {
        ShapeKind::Rectangle => {
            let r = spec.corner_radius * bw.min(bh);
            rounded_rect(&mut pb, x0, y0, x1, y1, r);
        }
        ShapeKind::Ellipse => {
            return PathBuilder::from_oval(Rect::from_ltrb(x0, y0, x1, y1)?);
        }
        ShapeKind::Triangle => {
            pb.move_to(x0 + bw / 2.0, y0);
            pb.line_to(x1, y1);
            pb.line_to(x0, y1);
            pb.close();
        }
        ShapeKind::Star => {
            let (cx, cy) = (x0 + bw / 2.0, y0 + bh / 2.0);
            for i in 0..10 {
                let r = if i % 2 == 0 { 1.0 } else { 0.42 };
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                let (px, py) = (
                    cx + a.cos() * r * bw / 2.0,
                    cy + a.sin() * r * bh / 2.0 * 1.05,
                );
                if i == 0 {
                    pb.move_to(px, py);
                } else {
                    pb.line_to(px, py);
                }
            }
            pb.close();
        }
        ShapeKind::SpeechBubble => {
            let body = y0 + bh * 0.78;
            let r = spec.corner_radius * bw.min(bh * 0.78);
            rounded_rect(&mut pb, x0, y0, x1, body, r);
            pb.move_to(x0 + bw * 0.18, body - 1.0);
            pb.line_to(x0 + bw * 0.12, y1);
            pb.line_to(x0 + bw * 0.38, body - 1.0);
            pb.close();
        }
        ShapeKind::Heart => {
            let (cx, top) = (x0 + bw / 2.0, y0 + bh * 0.28);
            pb.move_to(cx, y1);
            pb.cubic_to(
                x0 - bw * 0.05,
                y0 + bh * 0.62,
                x0,
                y0 + bh * 0.05,
                x0 + bw * 0.27,
                y0,
            );
            pb.cubic_to(x0 + bw * 0.42, y0 - bh * 0.03, cx, y0 + bh * 0.1, cx, top);
            pb.cubic_to(
                cx,
                y0 + bh * 0.1,
                x1 - bw * 0.42,
                y0 - bh * 0.03,
                x1 - bw * 0.27,
                y0,
            );
            pb.cubic_to(x1, y0 + bh * 0.05, x1 + bw * 0.05, y0 + bh * 0.62, cx, y1);
            pb.close();
        }
        ShapeKind::Line | ShapeKind::Arrow | ShapeKind::DoubleArrow => return None,
    }
    pb.finish()
}

fn rounded_rect(pb: &mut PathBuilder, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) {
    let r = r.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0).max(0.0);
    // Quarter circles approximated by cubics.
    let k = 0.552_284_8 * r;
    pb.move_to(x0 + r, y0);
    pb.line_to(x1 - r, y0);
    pb.cubic_to(x1 - r + k, y0, x1, y0 + r - k, x1, y0 + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1);
    pb.line_to(x0 + r, y1);
    pb.cubic_to(x0 + r - k, y1, x0, y1 - r + k, x0, y1 - r);
    pb.line_to(x0, y0 + r);
    pb.cubic_to(x0, y0 + r - k, x0 + r - k, y0, x0 + r, y0);
    pb.close();
}

/// Draws `spec` `height` pixels tall (width from its aspect).
pub fn shape(spec: &ShapeSpec, height: u32) -> Option<(u64, Arc<Raster>)> {
    let height = height.clamp(2, 4096);
    let key = hash((
        "shape",
        serde_json::to_string(spec).unwrap_or_default(),
        height,
    ));
    cached(key, || {
        let h = height as f32;
        let w = (h * spec.aspect).round().clamp(2.0, 8192.0);
        let mut pixmap = Pixmap::new(w as u32, height)?;
        let stroke_px = spec.stroke.map_or(0.0, |s| s.width * h);
        if spec.kind.is_line() {
            let s = spec.stroke?;
            let head = (h * 0.45).min(w * 0.3);
            let y = h / 2.0;
            let (mut a, mut b) = (stroke_px / 2.0 + 1.0, w - stroke_px / 2.0 - 1.0);
            if matches!(spec.kind, ShapeKind::Arrow | ShapeKind::DoubleArrow) {
                b = w - head * 0.6;
            }
            if spec.kind == ShapeKind::DoubleArrow {
                a = head * 0.6;
            }
            let mut pb = PathBuilder::new();
            pb.move_to(a, y);
            pb.line_to(b, y);
            let line = pb.finish()?;
            let stroke = Stroke {
                width: stroke_px.max(1.0),
                line_cap: LineCap::Round,
                ..Stroke::default()
            };
            pixmap.stroke_path(&line, &paint(s.color), &stroke, Transform::identity(), None);
            let mut heads = Vec::new();
            if matches!(spec.kind, ShapeKind::Arrow | ShapeKind::DoubleArrow) {
                heads.push((w - 1.0, -1.0));
            }
            if spec.kind == ShapeKind::DoubleArrow {
                heads.push((1.0, 1.0));
            }
            for (tip, dir) in heads {
                let mut pb = PathBuilder::new();
                pb.move_to(tip, y);
                pb.line_to(tip + dir * head, y - head / 2.0);
                pb.line_to(tip + dir * head, y + head / 2.0);
                pb.close();
                if let Some(p) = pb.finish() {
                    pixmap.fill_path(
                        &p,
                        &paint(s.color),
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
        } else {
            let path = outline(spec, w, h, stroke_px / 2.0 + 1.0)?;
            if let Some(f) = spec.fill {
                pixmap.fill_path(
                    &path,
                    &paint(f),
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
            if let Some(s) = spec.stroke
                && stroke_px > 0.0
            {
                let stroke = Stroke {
                    width: stroke_px,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                };
                pixmap.stroke_path(&path, &paint(s.color), &stroke, Transform::identity(), None);
            }
        }
        Some(Raster::from_pixmap(pixmap))
    })
}

/// Draws an audio visualiser `height` pixels tall from `bands` (0–1, low to
/// high) and `wave` (−1–1). Not cached: it changes every frame.
pub fn visualiser(
    spec: &rw_model::VisualiserSpec,
    bands: &[f32],
    wave: &[f32],
    height: u32,
) -> Option<(u64, Arc<Raster>)> {
    use rw_model::VisualiserStyle;
    let height = height.clamp(2, 4096);
    let h = height as f32;
    let w = (h * spec.aspect()).round().max(2.0);
    let mut pixmap = Pixmap::new(w as u32, height)?;
    let p = paint(spec.color);
    match spec.style {
        VisualiserStyle::Bars => {
            let n = bands.len().max(1) as f32;
            let slot = w / n;
            let bar = (slot * 0.7).max(1.0);
            for (i, v) in bands.iter().enumerate() {
                // A sliver even when silent, so the bars read as bars.
                let bh = (v.clamp(0.0, 1.0) * (h - 2.0)).max(h * 0.03);
                let x = i as f32 * slot + (slot - bar) / 2.0;
                let mut pb = PathBuilder::new();
                rounded_rect(&mut pb, x, h - bh, x + bar, h, bar * 0.3);
                if let Some(path) = pb.finish() {
                    pixmap.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
                }
            }
        }
        VisualiserStyle::Wave => {
            let mut pb = PathBuilder::new();
            let n = wave.len().max(2);
            for (i, v) in wave.iter().enumerate() {
                let x = i as f32 / (n - 1) as f32 * (w - 4.0) + 2.0;
                let y = h / 2.0 - v.clamp(-1.0, 1.0) * (h / 2.0 - 3.0);
                if i == 0 {
                    pb.move_to(x, y);
                } else {
                    pb.line_to(x, y);
                }
            }
            if wave.is_empty() {
                pb.move_to(2.0, h / 2.0);
                pb.line_to(w - 2.0, h / 2.0);
            }
            if let Some(path) = pb.finish() {
                let stroke = Stroke {
                    width: (h * 0.04).max(1.5),
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                };
                pixmap.stroke_path(&path, &p, &stroke, Transform::identity(), None);
            }
        }
        VisualiserStyle::Circle => {
            let (cx, cy) = (w / 2.0, h / 2.0);
            let inner = h * 0.22;
            let reach = h / 2.0 - inner - 2.0;
            let n = bands.len().max(1);
            let width = (std::f32::consts::TAU * inner / n as f32 * 0.6).max(1.0);
            for (i, v) in bands.iter().enumerate() {
                let a = std::f32::consts::TAU * i as f32 / n as f32 - std::f32::consts::FRAC_PI_2;
                let len = (v.clamp(0.0, 1.0) * reach).max(reach * 0.05);
                let (s, c) = a.sin_cos();
                let mut pb = PathBuilder::new();
                pb.move_to(cx + c * inner, cy + s * inner);
                pb.line_to(cx + c * (inner + len), cy + s * (inner + len));
                if let Some(path) = pb.finish() {
                    let stroke = Stroke {
                        width,
                        line_cap: LineCap::Round,
                        ..Stroke::default()
                    };
                    pixmap.stroke_path(&path, &p, &stroke, Transform::identity(), None);
                }
            }
        }
    }
    let key = hash((
        "visualiser",
        serde_json::to_string(spec).unwrap_or_default(),
        height,
        bands.iter().map(|v| (v * 255.0) as u8).collect::<Vec<_>>(),
        wave.iter().map(|v| (v * 127.0) as i8).collect::<Vec<_>>(),
    ));
    Some((key, Arc::new(Raster::from_pixmap(pixmap))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_at(r: &Raster, x: u32, y: u32) -> u8 {
        r.pixels[((y * r.width + x) * 4 + 3) as usize]
    }

    #[test]
    fn every_shape_draws_something_inside_its_box() {
        for kind in ShapeKind::ALL {
            let spec = ShapeSpec::new(kind);
            let (_, r) = shape(&spec, 100).unwrap();
            assert_eq!(r.height, 100, "{kind:?}");
            assert_eq!(r.width, (100.0 * spec.aspect).round() as u32, "{kind:?}");
            let inked = r.pixels.chunks(4).filter(|p| p[3] > 128).count();
            assert!(inked > 200, "{kind:?} drew {inked} pixels");
        }
        // A circle's corner is empty and its middle full.
        let (_, c) = shape(&ShapeSpec::new(ShapeKind::Ellipse), 100).unwrap();
        assert_eq!(alpha_at(&c, 2, 2), 0);
        assert_eq!(alpha_at(&c, 50, 50), 255);
    }

    #[test]
    fn stickers_render_at_the_asked_height() {
        let s = StickerRef {
            pack: stickers::BUNDLED.into(),
            id: stickers::bundled()[0].id.clone(),
        };
        let (k1, r) = sticker(&s, 128).unwrap();
        assert_eq!(r.height, 128);
        assert!(r.pixels.chunks(4).filter(|p| p[3] > 128).count() > 1000);
        let (k2, _) = sticker(&s, 128).unwrap();
        assert_eq!(k1, k2, "cached");
        let missing = StickerRef {
            pack: "nope".into(),
            id: "x".into(),
        };
        assert!(sticker(&missing, 64).is_none());
    }
}
