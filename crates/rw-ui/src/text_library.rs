//! The Text tab: ready-made titles, each shown in its own font and style.

use crate::actions::TextItem;
use crate::theme::Tokens;
use eframe::egui::{self, CornerRadius, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use rw_text::presets::Preset;

/// Carried while a text preset is dragged to the timeline.
#[derive(Debug, Clone)]
pub struct TextDrag(pub TextItem);

const TILE: egui::Vec2 = vec2(150.0, 84.0);

/// Presets in library order: plain text first.
pub fn presets() -> Vec<Preset> {
    std::iter::once(rw_text::presets::plain())
        .chain(rw_text::presets::all())
        .collect()
}

/// A small picture of the preset, drawn once and kept as a texture.
fn thumbnail(ctx: &egui::Context, p: &Preset) -> Option<egui::TextureHandle> {
    let id = egui::Id::new(("text-preset-thumb", p.name));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(t);
    }
    // Drawn once to measure, then again at the size that fills the tile
    // (at twice its resolution for crisp edges), capped so short text
    // isn't blown up.
    let text = p.spec.text_at(rw_model::Time::ZERO, p.duration);
    let probe = 200.0;
    let (_, img) = rw_text::render(&text, &p.spec.style, probe, None);
    let fit = ((TILE.x - 16.0) * 2.0 / img.width.max(1) as f32)
        .min((TILE.y - 16.0) * 2.0 / img.height.max(1) as f32)
        .min(3.0);
    let (_, img) = rw_text::render(&text, &p.spec.style, (probe * fit).round(), None);
    if img.width == 0 || img.height == 0 {
        return None;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [img.width as usize, img.height as usize],
        &img.pixels,
    );
    let tex = ctx.load_texture(format!("text-preset-{}", p.name), image, Default::default());
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

/// Grid of presets. Clicking returns one to add at the playhead; dragging
/// carries it to the timeline.
pub fn show(ui: &mut egui::Ui, tokens: Tokens) -> Option<TextItem> {
    let mut chosen = None;
    ui.label(
        egui::RichText::new("Click to add at the playhead, or drag onto the timeline.")
            .small()
            .color(tokens.text_muted),
    );
    ui.add_space(4.0);
    let all = presets();
    crate::layout::grid(ui, &all, TILE.x, 8.0, |ui, p| {
        let (rect, resp) = ui.allocate_exact_size(TILE + vec2(0.0, 18.0), Sense::click_and_drag());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, p.name));
        let tile = Rect::from_min_size(rect.min, TILE);
        let painter = ui.painter_at(tile);
        painter.rect_filled(
            tile,
            CornerRadius::same(4),
            egui::Color32::from_rgb(38, 44, 60),
        );
        if let Some(tex) = thumbnail(ui.ctx(), p) {
            // Fit the drawn text inside the tile.
            let size = tex.size_vec2() / 2.0;
            let scale = ((TILE.x - 8.0) / size.x)
                .min((TILE.y - 8.0) / size.y)
                .min(1.0);
            let shown = Rect::from_center_size(tile.center(), size * scale);
            painter.image(
                tex.id(),
                shown,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        let stroke = if resp.hovered() {
            Stroke::new(2.0, tokens.accent)
        } else {
            Stroke::new(1.0, tokens.border)
        };
        ui.painter()
            .rect_stroke(tile, CornerRadius::same(4), stroke, StrokeKind::Inside);
        ui.painter().text(
            pos2(rect.center().x, rect.bottom() - 8.0),
            egui::Align2::CENTER_CENTER,
            p.name,
            egui::FontId::proportional(11.0),
            tokens.text,
        );
        if resp.dragged() {
            resp.dnd_set_drag_payload(TextDrag(p.clone().into()));
        }
        if resp.clicked() {
            chosen = Some(p.clone().into());
        }
        resp.on_hover_text(p.name);
    });
    chosen
}
