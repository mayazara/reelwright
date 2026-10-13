//! The Graphics tab: stickers and emoji, shapes and audio visualisers.

use crate::theme::Tokens;
use eframe::egui::{self, CornerRadius, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use rw_assets::stickers::{self, StickerInfo};
use rw_model::{Item, ShapeKind, ShapeSpec, StickerRef, Time, VisualiserSpec, VisualiserStyle};

/// Carried while a graphic is dragged to the timeline.
#[derive(Debug, Clone)]
pub struct GraphicDrag(pub Item);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Stickers,
    Shapes,
    Visualisers,
}

#[derive(Debug, Default)]
pub struct GraphicsState {
    pub section: Section,
    pub search: String,
}

const TILE: f32 = 64.0;

/// A picture drawn by `draw` once, kept as a texture under `key`.
fn texture(
    ctx: &egui::Context,
    key: &str,
    draw: impl FnOnce() -> Option<rw_assets::raster::Raster>,
) -> Option<egui::TextureHandle> {
    let id = egui::Id::new(("graphic-thumb", key));
    if let Some(t) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(id)) {
        return t;
    }
    let tex = draw().map(|r| {
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [r.width as usize, r.height as usize],
            &r.pixels,
        );
        ctx.load_texture(key, image, egui::TextureOptions::LINEAR)
    });
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// One clickable, draggable tile showing `tex`; returns the item to add.
fn tile(
    ui: &mut egui::Ui,
    name: &str,
    tokens: Tokens,
    make_item: impl Fn() -> Item,
    thumb: impl FnOnce(&egui::Context) -> Option<egui::TextureHandle>,
) -> Option<Item> {
    let (rect, resp) = ui.allocate_exact_size(vec2(TILE, TILE), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(4), tokens.surface_raised);
        if let Some(tex) = thumb(ui.ctx()) {
            let size = tex.size_vec2();
            let scale = ((TILE - 10.0) / size.x).min((TILE - 10.0) / size.y);
            painter.image(
                tex.id(),
                Rect::from_center_size(rect.center(), size * scale),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        let stroke = if resp.hovered() {
            Stroke::new(2.0, tokens.accent)
        } else {
            Stroke::new(1.0, tokens.border)
        };
        painter.rect_stroke(rect, CornerRadius::same(4), stroke, StrokeKind::Inside);
    }
    if resp.dragged() {
        resp.dnd_set_drag_payload(GraphicDrag(make_item()));
    }
    let clicked = resp.clicked();
    resp.on_hover_text(name);
    clicked.then(make_item)
}

fn sticker_item(pack: &str, id: &str) -> Item {
    Item::sticker_item(
        StickerRef {
            pack: pack.into(),
            id: id.into(),
        },
        Time::ZERO,
    )
}

fn sticker_grid(
    ui: &mut egui::Ui,
    pack: &str,
    list: &[&StickerInfo],
    tokens: Tokens,
    chosen: &mut Option<Item>,
) {
    crate::layout::grid(ui, list, TILE, 6.0, |ui, s| {
        let key = format!("{pack}/{}", s.id);
        let r = tile(
            ui,
            &s.title(),
            tokens,
            || sticker_item(pack, &s.id),
            |ctx| {
                texture(ctx, &key, || {
                    rw_assets::raster::sticker(
                        &StickerRef {
                            pack: pack.into(),
                            id: s.id.clone(),
                        },
                        96,
                    )
                    .map(|(_, r)| (*r).clone())
                })
            },
        );
        if r.is_some() {
            *chosen = r;
        }
    });
}

pub fn show(ui: &mut egui::Ui, state: &mut GraphicsState, tokens: Tokens) -> Option<Item> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.section, Section::Stickers, "Stickers");
        ui.selectable_value(&mut state.section, Section::Shapes, "Shapes");
        ui.selectable_value(&mut state.section, Section::Visualisers, "Visualisers");
    });
    ui.label(
        egui::RichText::new("Click to add at the playhead, or drag onto the timeline.")
            .small()
            .color(tokens.text_muted),
    );
    match state.section {
        Section::Stickers => {
            crate::layout::name(
                ui.add(
                    egui::TextEdit::singleline(&mut state.search)
                        .hint_text("Search stickers")
                        .desired_width(f32::INFINITY),
                ),
                "Search stickers",
            );
            egui::ScrollArea::vertical().show(ui, |ui| {
                let query = state.search.trim();
                // Bundled stickers by group, in index order.
                let mut groups: Vec<(&str, Vec<&StickerInfo>)> = Vec::new();
                for s in stickers::bundled().iter().filter(|s| s.matches(query)) {
                    match groups.iter_mut().find(|(g, _)| *g == s.group) {
                        Some((_, v)) => v.push(s),
                        None => groups.push((&s.group, vec![s])),
                    }
                }
                for (group, list) in &groups {
                    ui.label(egui::RichText::new(*group).strong());
                    sticker_grid(ui, stickers::BUNDLED, list, tokens, &mut chosen);
                    ui.add_space(6.0);
                }
                for (pack, list) in stickers::user_packs() {
                    let list: Vec<&StickerInfo> =
                        list.iter().filter(|s| s.matches(query)).collect();
                    if list.is_empty() {
                        continue;
                    }
                    ui.label(egui::RichText::new(&pack).strong());
                    sticker_grid(ui, &pack, &list, tokens, &mut chosen);
                    ui.add_space(6.0);
                }
                if groups.is_empty() && !query.is_empty() {
                    ui.label(egui::RichText::new("No stickers match.").color(tokens.text_muted));
                }
                if let Some(dir) = stickers::user_packs_dir() {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "Add your own: put a folder of SVG or PNG pictures in {}",
                            dir.display()
                        ))
                        .small()
                        .color(tokens.text_muted),
                    );
                }
            });
        }
        Section::Shapes => {
            crate::layout::grid(ui, &ShapeKind::ALL, TILE, 6.0, |ui, kind| {
                let key = format!("shape/{kind:?}");
                let r = tile(
                    ui,
                    kind.label(),
                    tokens,
                    || Item::shape_item(ShapeSpec::new(*kind), Time::ZERO),
                    |ctx| {
                        texture(ctx, &key, || {
                            rw_assets::raster::shape(&ShapeSpec::new(*kind), 48)
                                .map(|(_, r)| (*r).clone())
                        })
                    },
                );
                if r.is_some() {
                    chosen = r;
                }
            });
        }
        Section::Visualisers => {
            ui.label(
                egui::RichText::new("These move with the project's sound.")
                    .small()
                    .color(tokens.text_muted),
            );
            crate::layout::grid(ui, &VisualiserStyle::ALL, TILE, 6.0, |ui, style| {
                let key = format!("visualiser/{style:?}");
                let r = tile(
                    ui,
                    style.label(),
                    tokens,
                    || Item::visualiser_item(VisualiserSpec::new(*style), Time::ZERO),
                    |ctx| {
                        texture(ctx, &key, || {
                            // A made-up spectrum for the picture.
                            let spec = VisualiserSpec::new(*style);
                            let bands: Vec<f32> = (0..spec.bars)
                                .map(|i| {
                                    let x = f32::from(i) / f32::from(spec.bars);
                                    (0.85 - x * 0.6) * (0.6 + 0.4 * (x * 17.0).sin().abs())
                                })
                                .collect();
                            let wave: Vec<f32> =
                                (0..64).map(|i| (i as f32 * 0.5).sin() * 0.7).collect();
                            rw_assets::raster::visualiser(&spec, &bands, &wave, 48)
                                .map(|(_, r)| (*r).clone())
                        })
                    },
                );
                if r.is_some() {
                    chosen = r;
                }
            });
        }
    }
    chosen
}
