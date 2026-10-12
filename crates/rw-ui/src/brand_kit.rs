//! The Brand kit tab: colours, fonts and logos kept on this computer.

use crate::theme::Tokens;
use eframe::egui::{self, CornerRadius, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use rw_assets::brand::{BrandKit, LOGO_PACK};
use rw_model::Rgba;

pub const ADD_BRAND_TEXT_LABEL: &str = "Add text in brand style";
pub const ADD_FONT_LABEL: &str = "Add font";

#[derive(Debug, Clone, PartialEq)]
pub enum BrandAction {
    /// Colour the selected text, shape, visualiser or background.
    ApplyColour(Rgba),
    /// Set the selected text's font.
    ApplyFont(String),
    /// New text in the kit's first font and colour.
    AddBrandText,
    /// Put a logo on the timeline at the playhead.
    AddLogo(String),
    ImportLogo,
    RemoveLogo(String),
    AddColour(Rgba),
    RemoveColour(usize),
    AddFont(String),
    RemoveFont(usize),
}

/// Per-tab state: the colour being picked for adding.
#[derive(Debug)]
pub struct BrandState {
    pub new_colour: egui::Color32,
}

impl Default for BrandState {
    fn default() -> Self {
        BrandState {
            new_colour: egui::Color32::from_rgb(14, 124, 116),
        }
    }
}

fn c32(c: Rgba) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.0[0], c.0[1], c.0[2], c.0[3])
}

pub fn show(
    ui: &mut egui::Ui,
    kit: &BrandKit,
    logos: &[String],
    state: &mut BrandState,
    tokens: Tokens,
) -> Option<BrandAction> {
    let mut out = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        let ready = !kit.fonts.is_empty() || !kit.colours.is_empty();
        if ui
            .add_enabled(ready, egui::Button::new(ADD_BRAND_TEXT_LABEL))
            .on_hover_text("New text in your first brand font and colour")
            .on_disabled_hover_text("Add a brand font or colour first")
            .clicked()
        {
            out = Some(BrandAction::AddBrandText);
        }
        ui.label(
            egui::RichText::new("Click a colour or font to use it on the selected clip.")
                .small()
                .color(tokens.text_muted),
        );
        ui.add_space(6.0);

        ui.label(egui::RichText::new("Colours").strong());
        ui.horizontal_wrapped(|ui| {
            for (i, c) in kit.colours.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                let name = crate::inspector::to_hex(*c);
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        format!("Brand colour {name}"),
                    )
                });
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(6), c32(*c));
                let stroke = if resp.hovered() {
                    Stroke::new(2.0, tokens.accent)
                } else {
                    Stroke::new(1.0, tokens.border)
                };
                ui.painter()
                    .rect_stroke(rect, CornerRadius::same(6), stroke, StrokeKind::Inside);
                if resp.clicked() {
                    out = Some(BrandAction::ApplyColour(*c));
                }
                resp.on_hover_text(format!("{name} — click to apply"))
                    .context_menu(|ui| {
                        if ui.button("Remove from brand kit").clicked() {
                            out = Some(BrandAction::RemoveColour(i));
                            ui.close();
                        }
                    });
            }
        });
        ui.horizontal(|ui| {
            egui::color_picker::color_edit_button_srgba(
                ui,
                &mut state.new_colour,
                egui::color_picker::Alpha::Opaque,
            );
            if ui.button("Add colour").clicked() {
                let [r, g, b, a] = state.new_colour.to_srgba_unmultiplied();
                out = Some(BrandAction::AddColour(Rgba([r, g, b, a])));
            }
        });
        ui.add_space(8.0);

        ui.label(egui::RichText::new("Fonts").strong());
        for (i, f) in kit.fonts.iter().enumerate() {
            let resp = ui
                .add(egui::Button::new(f).min_size(vec2(200.0, 0.0)))
                .on_hover_text("Click to use on the selected text");
            if resp.clicked() {
                out = Some(BrandAction::ApplyFont(f.clone()));
            }
            resp.context_menu(|ui| {
                if ui.button("Remove from brand kit").clicked() {
                    out = Some(BrandAction::RemoveFont(i));
                    ui.close();
                }
            });
        }
        egui::ComboBox::from_label(ADD_FONT_LABEL)
            .selected_text("Choose…")
            .width(200.0)
            .height(320.0)
            .show_ui(ui, |ui| {
                let (bundled, system) = rw_text::font_families();
                for f in bundled.into_iter().chain(system) {
                    if !kit.fonts.contains(&f) && ui.selectable_label(false, &f).clicked() {
                        out = Some(BrandAction::AddFont(f));
                    }
                }
            });
        ui.add_space(8.0);

        ui.label(egui::RichText::new("Logos").strong());
        crate::layout::grid(ui, logos, 72.0, 6.0, |ui, id| {
            let (rect, resp) = ui.allocate_exact_size(vec2(72.0, 72.0), Sense::click_and_drag());
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, id.as_str())
            });
            ui.painter()
                .rect_filled(rect, CornerRadius::same(4), tokens.surface_raised);
            let key = format!("{LOGO_PACK}/{id}");
            let tex_id = egui::Id::new(("brand-logo", &key));
            let tex = ui
                .ctx()
                .data(|d| d.get_temp::<egui::TextureHandle>(tex_id))
                .or_else(|| {
                    let sticker = rw_model::StickerRef {
                        pack: LOGO_PACK.into(),
                        id: id.clone(),
                    };
                    let (_, r) = rw_assets::raster::sticker(&sticker, 96)?;
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [r.width as usize, r.height as usize],
                        &r.pixels,
                    );
                    let t = ui
                        .ctx()
                        .load_texture(&key, image, egui::TextureOptions::LINEAR);
                    ui.ctx().data_mut(|d| d.insert_temp(tex_id, t.clone()));
                    Some(t)
                });
            if let Some(t) = tex {
                let size = t.size_vec2();
                let scale = (62.0 / size.x).min(62.0 / size.y);
                ui.painter().image(
                    t.id(),
                    Rect::from_center_size(rect.center(), size * scale),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            if resp.dragged() {
                resp.dnd_set_drag_payload(crate::graphics_library::GraphicDrag(
                    rw_model::Item::sticker_item(
                        rw_model::StickerRef {
                            pack: LOGO_PACK.into(),
                            id: id.clone(),
                        },
                        rw_model::Time::ZERO,
                    ),
                ));
            }
            if resp.clicked() {
                out = Some(BrandAction::AddLogo(id.clone()));
            }
            resp.on_hover_text(id.as_str()).context_menu(|ui| {
                if ui.button("Remove from brand kit").clicked() {
                    out = Some(BrandAction::RemoveLogo(id.clone()));
                    ui.close();
                }
            });
        });
        if ui
            .button("Add logo…")
            .on_hover_text("A PNG or SVG picture")
            .clicked()
        {
            out = Some(BrandAction::ImportLogo);
        }
    });
    out
}
