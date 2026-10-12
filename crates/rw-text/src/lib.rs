//! Titles and captions: text layout, drawing and animation presets.
//!
//! [`render`] draws a text item's text in its style; [`motion::pose_at`]
//! says how it moves at a moment. Drawn text is cached, so a still title
//! costs nothing per frame.

pub mod fonts;
pub mod motion;
pub mod presets;
mod raster;

pub use motion::{Pose, pose_at};
pub use raster::TextImage;

use cosmic_text::{FontSystem, SwashCache};
use rw_model::TextStyle;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

/// Most drawn texts kept for reuse.
const CACHE_ENTRIES: usize = 256;

struct Renderer {
    system: FontSystem,
    swash: SwashCache,
    cache: HashMap<u64, Arc<TextImage>>,
    order: std::collections::VecDeque<u64>,
}

/// The app's fonts folder, set once before the first text is drawn.
static USER_FONTS: OnceLock<std::path::PathBuf> = OnceLock::new();
static RENDERER: OnceLock<Mutex<Renderer>> = OnceLock::new();

/// Adds the user's fonts folder (call at start-up, before drawing text).
pub fn set_user_fonts_dir(dir: std::path::PathBuf) {
    let _ = USER_FONTS.set(dir);
}

fn renderer() -> &'static Mutex<Renderer> {
    RENDERER.get_or_init(|| {
        Mutex::new(Renderer {
            system: fonts::font_system(USER_FONTS.get().map(|p| p.as_path())),
            swash: SwashCache::new(),
            cache: HashMap::new(),
            order: std::collections::VecDeque::new(),
        })
    })
}

fn key(text: &str, style: &TextStyle, canvas_height: f32, reveal: Option<usize>) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    // Styles hold floats; their JSON form is a stable, exact key.
    serde_json::to_string(style)
        .unwrap_or_default()
        .hash(&mut h);
    canvas_height.to_bits().hash(&mut h);
    reveal.hash(&mut h);
    h.finish()
}

/// Draws `text` in `style` for a canvas `canvas_height` pixels tall,
/// showing only the first `reveal` characters if set. Cached.
pub fn render(
    text: &str,
    style: &TextStyle,
    canvas_height: f32,
    reveal: Option<usize>,
) -> (u64, Arc<TextImage>) {
    let k = key(text, style, canvas_height, reveal);
    let mut r = renderer().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(img) = r.cache.get(&k) {
        return (k, img.clone());
    }
    let Renderer { system, swash, .. } = &mut *r;
    let img = Arc::new(raster::render(
        system,
        swash,
        text,
        style,
        canvas_height,
        reveal,
    ));
    r.cache.insert(k, img.clone());
    r.order.push_back(k);
    while r.order.len() > CACHE_ENTRIES {
        if let Some(old) = r.order.pop_front() {
            r.cache.remove(&old);
        }
    }
    (k, img)
}

/// Bundled title font families, then fonts on this computer.
pub fn font_families() -> (Vec<String>, Vec<String>) {
    let r = renderer().lock().unwrap_or_else(|e| e.into_inner());
    (
        fonts::bundled_families(&r.system),
        fonts::system_families(&r.system),
    )
}
