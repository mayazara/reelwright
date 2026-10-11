//! The preview canvas: composites on egui's own GPU device and shows the
//! result as a native texture, with no CPU copy.

use eframe::egui;
use eframe::egui_wgpu;
use eframe::egui_wgpu::wgpu;
use rw_cache::ProjectCache;
use rw_engine::frames::FrameSource;
use rw_model::{Canvas, Project, Time};
use rw_render::{Compositor, Gpu, Layer};
use std::sync::Arc;

/// The GPU the UI renders with, when there is one (not in headless tests).
#[derive(Clone)]
pub struct UiGpu {
    pub gpu: Gpu,
    pub renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
}

impl UiGpu {
    pub fn from_render_state(rs: &egui_wgpu::RenderState) -> Self {
        let info = rs.adapter.get_info();
        UiGpu {
            gpu: Gpu::shared(
                rs.device.clone(),
                rs.queue.clone(),
                format!("{} ({:?})", info.name, info.backend),
            ),
            renderer: rs.renderer.clone(),
        }
    }
}

/// Pixel size to render the preview at: the canvas shape fitted into the
/// panel, at screen resolution, never above the canvas size, rounded so
/// small panel resizes don't reallocate every frame.
pub fn target_size(panel: egui::Vec2, pixels_per_point: f32, canvas: Canvas) -> (u32, u32) {
    let (cw, ch) = (canvas.width as f32, canvas.height as f32);
    let scale = ((panel.x * pixels_per_point) / cw)
        .min((panel.y * pixels_per_point) / ch)
        .clamp(0.01, 1.0);
    // Round the width to 16 px; the height follows the canvas shape.
    let w = (((cw * scale / 16.0).round() as u32).max(1) * 16).min(canvas.width);
    let h = (((w as f32 * ch / cw / 2.0).round() as u32).max(1) * 2).min(canvas.height);
    (w, h)
}

pub struct Preview {
    ui_gpu: UiGpu,
    compositor: Compositor,
    texture: Option<egui::TextureId>,
    /// Decodes for scrubbing while paused (playback uses the player's own).
    scrub: FrameSource,
    /// What the texture shows: (time, project revision, size).
    shown: Option<(Time, u64, (u32, u32))>,
}

impl Preview {
    pub fn new(ui_gpu: UiGpu, cache: ProjectCache) -> Self {
        let compositor = Compositor::new(&ui_gpu.gpu, (16, 16));
        Preview {
            ui_gpu,
            compositor,
            texture: None,
            scrub: FrameSource::new(cache, true),
            shown: None,
        }
    }

    fn register(&mut self) {
        let device = &self.ui_gpu.gpu.device;
        let view = self.compositor.output_view();
        let mut r = self.ui_gpu.renderer.write();
        match self.texture {
            Some(id) => {
                r.update_egui_texture_from_wgpu_texture(device, view, wgpu::FilterMode::Linear, id)
            }
            None => {
                self.texture =
                    Some(r.register_native_texture(device, view, wgpu::FilterMode::Linear))
            }
        }
    }

    fn ensure_size(&mut self, size: (u32, u32)) {
        if self.compositor.size() != size || self.texture.is_none() {
            self.compositor.resize(size);
            self.register();
            self.shown = None;
        }
    }

    /// Composites layers (from the player while playing).
    pub fn show_layers(&mut self, project: &Project, layers: &[Layer], size: (u32, u32)) {
        self.ensure_size(size);
        self.compositor.render(project.canvas.background, layers);
        self.shown = None;
    }

    /// Renders the frame at `t` if it isn't already showing. `revision`
    /// changes whenever the project does.
    pub fn show_paused(&mut self, project: &Project, t: Time, revision: u64, size: (u32, u32)) {
        self.ensure_size(size);
        if self.shown == Some((t, revision, size)) {
            return;
        }
        let layers = self.scrub.layers_at(project, t, size);
        self.compositor.render(project.canvas.background, &layers);
        self.shown = Some((t, revision, size));
    }

    /// Forces the next paused frame to re-render (e.g. proxies changed).
    pub fn invalidate(&mut self) {
        self.shown = None;
        self.scrub.reset();
    }

    pub fn texture(&self) -> Option<egui::TextureId> {
        self.texture
    }

    pub fn gpu(&self) -> &Gpu {
        &self.ui_gpu.gpu
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        if let Some(id) = self.texture.take() {
            self.ui_gpu.renderer.write().free_texture(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rw_model::AspectPreset;

    #[test]
    fn target_size_fits_panel_and_caps_at_canvas() {
        let hd = Canvas::from_preset(AspectPreset::Widescreen16x9);
        assert_eq!(target_size(egui::vec2(960.0, 540.0), 1.0, hd), (960, 540));
        assert_eq!(
            target_size(egui::vec2(4000.0, 3000.0), 2.0, hd),
            (1920, 1080)
        );
        let vertical = Canvas::from_preset(AspectPreset::Vertical9x16);
        let (w, h) = target_size(egui::vec2(800.0, 600.0), 1.0, vertical);
        assert!(h <= 608 && w < h, "{w}x{h}");
    }
}
