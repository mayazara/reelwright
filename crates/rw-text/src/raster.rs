//! Lays out and draws text with its box, shadow and outline into an image.

use cosmic_text::{
    Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, SwashContent,
    Weight,
};
use rw_model::{Rgba, TextAlign, TextStyle};

/// A drawn piece of text: straight-alpha sRGB RGBA8.
#[derive(Debug, Clone, PartialEq)]
pub struct TextImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// Characters drawn and in the full text (fewer while typing on).
    pub drawn_chars: usize,
    /// Whether every character had a glyph in some font.
    pub all_glyphs_found: bool,
    /// Glyphs after shaping (ligatures and conjuncts make fewer than
    /// characters).
    pub glyphs: usize,
}

/// Linear-ish float RGBA, premultiplied, in sRGB space (simple and close
/// to how titles are usually composited).
type Px = [f32; 4];

fn rgba_f(c: Rgba) -> Px {
    let a = f32::from(c.0[3]) / 255.0;
    [
        f32::from(c.0[0]) / 255.0 * a,
        f32::from(c.0[1]) / 255.0 * a,
        f32::from(c.0[2]) / 255.0 * a,
        a,
    ]
}

/// `src` over `dst`, `src` premultiplied, scaled by `k`.
fn over(dst: &mut Px, src: Px, k: f32) {
    let a = src[3] * k;
    for i in 0..3 {
        dst[i] = src[i] * k + dst[i] * (1.0 - a);
    }
    dst[3] = a + dst[3] * (1.0 - a);
}

/// Separable box blur of `a` (w×h) with `radius`, three passes ≈ Gaussian.
fn blur(a: &mut [f32], w: usize, h: usize, radius: f32) {
    let r = (radius / 3.0f32.sqrt()).round() as usize;
    if r == 0 {
        return;
    }
    let mut tmp = vec![0.0f32; a.len()];
    for _ in 0..3 {
        // Horizontal.
        for y in 0..h {
            let row = &a[y * w..(y + 1) * w];
            let mut sum: f32 = row.iter().take(r + 1).sum();
            for x in 0..w {
                tmp[y * w + x] = sum / (2 * r + 1) as f32;
                if x + r + 1 < w {
                    sum += row[x + r + 1];
                }
                if x >= r {
                    sum -= row[x - r];
                }
            }
        }
        // Vertical.
        for x in 0..w {
            let mut sum = 0.0;
            for y in 0..(r + 1).min(h) {
                sum += tmp[y * w + x];
            }
            for y in 0..h {
                a[y * w + x] = sum / (2 * r + 1) as f32;
                if y + r + 1 < h {
                    sum += tmp[(y + r + 1) * w + x];
                }
                if y >= r {
                    sum -= tmp[(y - r) * w + x];
                }
            }
        }
    }
}

/// Exact squared distance transform along one line (Felzenszwalb).
fn edt_1d(f: &[f32], d: &mut [f32], v: &mut [usize], z: &mut [f32]) {
    let n = f.len();
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    for q in 1..n {
        loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * (q - p) as f32);
            if s <= z[k] {
                if k == 0 {
                    v[0] = q;
                    z[0] = f32::NEG_INFINITY;
                    z[1] = f32::INFINITY;
                    break;
                }
                k -= 1;
                continue;
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f32::INFINITY;
            break;
        }
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        *out = (q as f32 - p as f32).powi(2) + f[p];
    }
}

/// Distance from each pixel to the nearest pixel with coverage ≥ 0.5.
fn distance_to_shape(a: &[f32], w: usize, h: usize) -> Vec<f32> {
    let big = 1e12f32;
    let mut grid: Vec<f32> = a
        .iter()
        .map(|v| if *v >= 0.5 { 0.0 } else { big })
        .collect();
    let n = w.max(h);
    let (mut f, mut d) = (vec![0.0f32; n], vec![0.0f32; n]);
    let (mut v, mut z) = (vec![0usize; n], vec![0.0f32; n + 1]);
    for x in 0..w {
        for y in 0..h {
            f[y] = grid[y * w + x];
        }
        edt_1d(&f[..h], &mut d[..h], &mut v[..h], &mut z[..h + 1]);
        for y in 0..h {
            grid[y * w + x] = d[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&grid[y * w..(y + 1) * w]);
        edt_1d(&f[..w], &mut d[..w], &mut v[..w], &mut z[..w + 1]);
        for x in 0..w {
            grid[y * w + x] = d[x].sqrt();
        }
    }
    grid
}

/// Coverage of a rounded rectangle at pixel centre `(x, y)`.
fn rounded_rect(x: f32, y: f32, min: [f32; 2], max: [f32; 2], radius: f32) -> f32 {
    let half = [(max[0] - min[0]) / 2.0, (max[1] - min[1]) / 2.0];
    let c = [(max[0] + min[0]) / 2.0, (max[1] + min[1]) / 2.0];
    let r = radius.min(half[0]).min(half[1]).max(0.0);
    let q = [
        (x - c[0]).abs() - (half[0] - r),
        (y - c[1]).abs() - (half[1] - r),
    ];
    let outside = (q[0].max(0.0).powi(2) + q[1].max(0.0).powi(2)).sqrt();
    let dist = outside + q[0].max(q[1]).min(0.0) - r;
    (0.5 - dist).clamp(0.0, 1.0)
}

/// Draws `text` in `style` with letters `scale` pixels per unit of font
/// size (the canvas height in output pixels). Only the first `reveal`
/// characters are drawn, but the layout (and box) is that of the whole
/// text, so typing on doesn't move anything.
pub fn render(
    system: &mut FontSystem,
    cache: &mut SwashCache,
    text: &str,
    style: &TextStyle,
    canvas_height: f32,
    reveal: Option<usize>,
) -> TextImage {
    let font_px = (style.size * canvas_height).max(1.0);
    let line_px = font_px * style.line_spacing;
    let mut buffer = Buffer::new(system, Metrics::new(font_px, line_px));
    buffer.set_size(None, None);
    let attrs = Attrs::new()
        .family(Family::Name(&style.font))
        .weight(Weight(style.weight))
        .style(if style.italic {
            Style::Italic
        } else {
            Style::Normal
        })
        .letter_spacing(style.letter_spacing);
    let align = match style.align {
        TextAlign::Left => Align::Left,
        TextAlign::Center => Align::Center,
        TextAlign::Right => Align::Right,
    };
    buffer.set_text(text, &attrs, Shaping::Advanced, Some(align));
    buffer.shape_until_scroll(system, false);

    // Bounds of the laid-out text.
    let mut text_w = 0.0f32;
    let mut lines = 0usize;
    for run in buffer.layout_runs() {
        text_w = text_w.max(run.line_w);
        lines = lines.max(run.line_i + 1);
    }
    let text_h = (lines.max(1) as f32) * line_px;

    // Room around the text for the box, outline and shadow.
    let outline_px = style.outline.map_or(0.0, |o| o.width * font_px);
    let shadow_px = style.shadow.map_or(0.0, |s| {
        s.offset[0].abs().max(s.offset[1].abs()) * font_px + 2.0 * s.blur * font_px
    });
    let box_px = style.background.map_or(0.0, |b| b.padding * font_px);
    let pad = (outline_px + 1.0).max(shadow_px + 1.0).max(box_px).ceil() + 2.0;
    let w = (text_w + 2.0 * pad).ceil().max(1.0) as usize;
    let h = (text_h + 2.0 * pad).ceil().max(1.0) as usize;

    // Which characters to draw.
    let reveal_byte = reveal.map(|n| text.char_indices().nth(n).map_or(text.len(), |(i, _)| i));
    let total_chars = text.chars().count();

    // Glyph coverage (plain glyphs) and colour glyphs (emoji).
    let mut fill = vec![0.0f32; w * h];
    let mut colour: Vec<Px> = vec![[0.0; 4]; w * h];
    let mut all_found = true;
    let mut glyph_count = 0;
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            glyph_count += 1;
            if glyph.glyph_id == 0 && !run.text[glyph.start..glyph.end].trim().is_empty() {
                all_found = false;
            }
            if reveal_byte.is_some_and(|r| glyph.start >= r) {
                continue;
            }
            let physical = glyph.physical((pad, pad + run.line_y), 1.0);
            let Some(image) = cache.get_image(system, physical.cache_key).as_ref() else {
                continue;
            };
            let p = image.placement;
            let x0 = physical.x + p.left;
            let y0 = physical.y - p.top;
            for gy in 0..p.height as i32 {
                let y = y0 + gy;
                if y < 0 || y >= h as i32 {
                    continue;
                }
                for gx in 0..p.width as i32 {
                    let x = x0 + gx;
                    if x < 0 || x >= w as i32 {
                        continue;
                    }
                    let i = y as usize * w + x as usize;
                    let j = (gy * p.width as i32 + gx) as usize;
                    match image.content {
                        SwashContent::Mask => {
                            let a = f32::from(image.data[j]) / 255.0;
                            fill[i] = (fill[i] + a).min(1.0);
                        }
                        SwashContent::SubpixelMask => {
                            let a = f32::from(image.data[j * 4 + 1]) / 255.0;
                            fill[i] = (fill[i] + a).min(1.0);
                        }
                        SwashContent::Color => {
                            let d = &image.data[j * 4..j * 4 + 4];
                            let a = f32::from(d[3]) / 255.0;
                            let src = [
                                f32::from(d[0]) / 255.0 * a,
                                f32::from(d[1]) / 255.0 * a,
                                f32::from(d[2]) / 255.0 * a,
                                a,
                            ];
                            over(&mut colour[i], src, 1.0);
                        }
                    }
                }
            }
        }
    }
    let shape: Vec<f32> = fill.iter().zip(&colour).map(|(f, c)| f.max(c[3])).collect();

    let mut out: Vec<Px> = vec![[0.0; 4]; w * h];
    // Box behind the text.
    if let Some(b) = style.background {
        let pad_px = b.padding * font_px;
        let min = [pad - pad_px, pad - pad_px];
        let max = [pad + text_w + pad_px, pad + text_h + pad_px];
        let c = rgba_f(b.color);
        for y in 0..h {
            for x in 0..w {
                let k = rounded_rect(x as f32 + 0.5, y as f32 + 0.5, min, max, b.radius * font_px);
                if k > 0.0 {
                    over(&mut out[y * w + x], c, k);
                }
            }
        }
    }
    // Shadow: the letters' shape, blurred and shifted.
    if let Some(s) = style.shadow {
        let mut a = shape.clone();
        if let Some(o) = style.outline {
            let r = o.width * font_px;
            if r > 0.0 {
                let dist = distance_to_shape(&shape, w, h);
                for (v, d) in a.iter_mut().zip(&dist) {
                    *v = v.max((r - d + 0.5).clamp(0.0, 1.0));
                }
            }
        }
        blur(&mut a, w, h, s.blur * font_px);
        let (dx, dy) = (
            (s.offset[0] * font_px).round() as i64,
            (s.offset[1] * font_px).round() as i64,
        );
        let c = rgba_f(s.color);
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let (sx, sy) = (x - dx, y - dy);
                if sx < 0 || sy < 0 || sx >= w as i64 || sy >= h as i64 {
                    continue;
                }
                let k = a[sy as usize * w + sx as usize];
                if k > 0.0 {
                    over(&mut out[y as usize * w + x as usize], c, k);
                }
            }
        }
    }
    // Outline: everything within `width` of the letters.
    if let Some(o) = style.outline {
        let r = o.width * font_px;
        if r > 0.0 {
            let dist = distance_to_shape(&shape, w, h);
            let c = rgba_f(o.color);
            for i in 0..w * h {
                let k = ((r - dist[i] + 0.5).clamp(0.0, 1.0)).max(shape[i]);
                if k > 0.0 {
                    over(&mut out[i], c, k);
                }
            }
        }
    }
    // The letters themselves.
    let c = rgba_f(style.color);
    for i in 0..w * h {
        if fill[i] > 0.0 {
            over(&mut out[i], c, fill[i]);
        }
        if colour[i][3] > 0.0 {
            over(&mut out[i], colour[i], 1.0);
        }
    }

    let mut pixels = Vec::with_capacity(w * h * 4);
    for p in out {
        let a = p[3].clamp(0.0, 1.0);
        let un = |v: f32| {
            if a > 0.0 {
                ((v / a).clamp(0.0, 1.0) * 255.0).round() as u8
            } else {
                0
            }
        };
        pixels.extend([un(p[0]), un(p[1]), un(p[2]), (a * 255.0).round() as u8]);
    }
    TextImage {
        width: w as u32,
        height: h as u32,
        pixels,
        drawn_chars: reveal.map_or(total_chars, |n| n.min(total_chars)),
        all_glyphs_found: all_found,
        glyphs: glyph_count,
    }
}
