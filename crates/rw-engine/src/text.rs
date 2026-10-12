//! Text items as compositor layers.
//!
//! A text item's box isn't set by its transform's size the way a picture's
//! is: the text has a natural size (its layout at its font size), and the
//! transform's size scales that (`[1, 1]` = natural). The centre places it.

use rw_model::{Item, Time};
use std::sync::Arc;

/// Where a text item sits at `local` into it, on a canvas of `canvas`
/// pixels: centre and size as shares of the canvas, plus the drawn text.
pub struct Placed {
    pub center: [f32; 2],
    pub size: [f32; 2],
    pub opacity: f32,
    pub key: u64,
    pub image: Arc<rw_text::TextImage>,
}

/// Lays out `item` (a text item) at `local` for a canvas `canvas` in size.
pub fn place(item: &Item, local: Time, canvas: (u32, u32)) -> Option<Placed> {
    let spec = item.text()?;
    let text = spec.text_at(local, item.duration);
    let pose = rw_text::pose_at(spec, &text, local, item.duration);
    let (key, image) = rw_text::render(&text, &spec.style, canvas.1 as f32, pose.reveal);
    let t = item.look.transform;
    let size = [
        image.width as f32 / canvas.0.max(1) as f32 * t.size[0] * pose.scale,
        image.height as f32 / canvas.1.max(1) as f32 * t.size[1] * pose.scale,
    ];
    let mut center = [t.center[0] + pose.offset[0], t.center[1] + pose.offset[1]];
    if let Some(roll) = pose.roll {
        // From just below the frame to just above it.
        center[1] = 1.0 + size[1] / 2.0 - roll * (1.0 + size[1]);
    }
    Some(Placed {
        center,
        size,
        opacity: pose.opacity,
        key,
        image,
    })
}
