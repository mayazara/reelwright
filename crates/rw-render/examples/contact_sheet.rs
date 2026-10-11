//! Tiles golden images into one PNG for review:
//! `cargo run -p rw-render --example contact_sheet -- out.png name...`
use rw_media::RgbaImage;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("output path");
    let names: Vec<String> = args.collect();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let imgs: Vec<RgbaImage> = names
        .iter()
        .map(|n| RgbaImage::load_png(&dir.join(format!("{n}.png"))).expect("golden image"))
        .collect();
    let (w, h) = (imgs[0].width, imgs[0].height);
    let cols = 4u32;
    let rows = (imgs.len() as u32).div_ceil(cols);
    let scale = 3u32;
    let (sw, sh) = (
        cols * w * scale + (cols + 1) * 4,
        rows * h * scale + (rows + 1) * 4,
    );
    let mut px = vec![40u8; (sw * sh * 4) as usize];
    for (i, img) in imgs.iter().enumerate() {
        let (cx, cy) = (i as u32 % cols, i as u32 / cols);
        let (ox, oy) = (4 + cx * (w * scale + 4), 4 + cy * (h * scale + 4));
        for y in 0..h * scale {
            for x in 0..w * scale {
                let s = (((y / scale) * w + x / scale) * 4) as usize;
                let d = (((oy + y) * sw + ox + x) * 4) as usize;
                px[d..d + 4].copy_from_slice(&img.pixels[s..s + 4]);
            }
        }
    }
    RgbaImage::new(sw, sh, px)
        .save_png(std::path::Path::new(&out))
        .expect("write sheet");
}
