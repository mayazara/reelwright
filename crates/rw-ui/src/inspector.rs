//! Properties of the selected clips, in tabs.
//!
//! Every control previews live while it's dragged (the session's preview
//! patch) and commits one undoable edit when released.

use crate::actions::EditorAction;
use crate::format;
use crate::session::Session;
use crate::theme::Tokens;
use eframe::egui::{self, Response, RichText};
use rw_edit::timeline::{LookPatch, TimelineOp};
use rw_model::{
    BlendMode, ColorAdjust, ColorControl, Crop, Effect, EffectKind, Filter, Fit, Generated, Item,
    ItemId, Layout, Look, MAX_GAIN, MAX_SPEED, MAX_TRANSITION, MIN_SPEED, MIN_TRANSITION,
    MediaKind, Rgba, Time, Transform, TransitionId, TransitionKind,
};

pub const EMPTY_LABEL: &str = "Select an item to edit its properties.";
pub const VOLUME_LABEL: &str = "Volume";

/// Quietest settable level; anything below is silence.
const MIN_DB: f32 = -60.0;

pub fn gain_to_db(gain: f32) -> f32 {
    if gain <= 0.0 {
        MIN_DB
    } else {
        (20.0 * gain.log10()).max(MIN_DB)
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    if db <= MIN_DB {
        0.0
    } else {
        10f32.powf(db / 20.0).min(MAX_GAIN)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Transform,
    Colour,
    Filters,
    Effects,
    Speed,
    Fade,
    Audio,
    Background,
}

impl Tab {
    pub fn label(self) -> &'static str {
        match self {
            Tab::Transform => "Transform",
            Tab::Colour => "Adjust colours",
            Tab::Filters => "Filters",
            Tab::Effects => "Effects",
            Tab::Speed => "Speed",
            Tab::Fade => "Fade",
            Tab::Audio => "Audio",
            Tab::Background => "Background",
        }
    }
}

/// Crop shapes offered as one-click presets: label and width/height.
pub const CROP_RATIOS: &[(&str, f32)] = &[
    ("16:9", 16.0 / 9.0),
    ("9:16", 9.0 / 16.0),
    ("1:1", 1.0),
    ("4:3", 4.0 / 3.0),
    ("4:5", 0.8),
];

/// Collects edits for this frame.
struct Edits<'a> {
    session: &'a mut Session,
    ids: Vec<ItemId>,
    out: Vec<EditorAction>,
}

impl Edits<'_> {
    /// Live preview while `r` is being dragged; commit when it's let go or
    /// changed by click/keyboard.
    fn live(&mut self, r: &Response, patch: LookPatch) {
        if r.changed() {
            self.session
                .set_preview_patch(self.ids.clone(), patch.clone());
        }
        if r.drag_stopped()
            || (r.changed() && !r.dragged())
            || (r.lost_focus() && self.session.preview_patch.is_some())
        {
            let patch = self
                .session
                .preview_patch
                .take()
                .map(|(_, p)| p)
                .filter(|_| !r.changed())
                .unwrap_or(patch);
            self.commit(patch);
        }
    }

    fn commit(&mut self, patch: LookPatch) {
        self.session.clear_preview_patch();
        if !patch.is_empty() {
            self.out.push(EditorAction::Op(TimelineOp::Look {
                items: self.ids.clone(),
                patch,
            }));
        }
    }

    fn live_fill(&mut self, r: &Response, fill: Generated) {
        if r.changed() {
            self.session.set_preview_fill(self.ids.clone(), fill);
        }
        if r.drag_stopped()
            || (r.changed() && !r.dragged())
            || (r.lost_focus() && self.session.preview_fill.is_some())
        {
            let fill = self.session.preview_fill.take().map_or(fill, |(_, f)| f);
            self.session.clear_preview_patch();
            self.out.push(EditorAction::Op(TimelineOp::SetFill {
                items: self.ids.clone(),
                fill,
            }));
        }
    }
}

fn reset_button(ui: &mut egui::Ui, enabled: bool) -> bool {
    ui.add_enabled(enabled, egui::Button::new("Reset").small())
        .clicked()
}

pub fn show(ui: &mut egui::Ui, session: &mut Session, tokens: Tokens) -> Vec<EditorAction> {
    // Show the look as previewed (uncommitted drags included).
    let shown = session.preview_project().into_owned();
    let items: Vec<Item> = shown
        .items()
        .filter(|i| session.selection.contains(&i.id))
        .cloned()
        .collect();
    if items.is_empty()
        && let Some(id) = session.timeline.selected_transition
    {
        return transition_panel(ui, session, id, tokens);
    }
    if items.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new(EMPTY_LABEL).color(tokens.text_muted));
        });
        return Vec::new();
    }
    let ids: Vec<ItemId> = items.iter().map(|i| i.id).collect();
    let first = items[0].clone();
    let media_of = |i: &Item| i.media().and_then(|m| shown.media(m));
    let visual: Vec<&Item> = items
        .iter()
        .filter(|i| {
            i.generated_fill().is_some()
                || media_of(i).is_some_and(|m| m.kind() != MediaKind::Audio)
        })
        .collect();
    let audible = items
        .iter()
        .any(|i| media_of(i).is_some_and(|m| m.info.audio.is_some()));
    let background = items.iter().all(|i| i.generated_fill().is_some());
    // Clips that play their source can change speed and direction.
    let retimable: Vec<&Item> = items
        .iter()
        .filter(|i| {
            i.held_frame().is_none()
                && media_of(i)
                    .is_some_and(|m| matches!(m.kind(), MediaKind::Video | MediaKind::Audio))
        })
        .collect();

    let mut tabs = Vec::new();
    if !visual.is_empty() {
        tabs.extend([Tab::Transform, Tab::Colour, Tab::Filters, Tab::Effects]);
    }
    if !retimable.is_empty() {
        tabs.push(Tab::Speed);
    }
    if background {
        tabs.insert(0, Tab::Background);
    }
    tabs.push(Tab::Fade);
    if audible {
        tabs.push(Tab::Audio);
    }
    let tab_id = ui.make_persistent_id("inspector-tab");
    let mut tab: Tab = ui.data(|d| d.get_temp(tab_id)).unwrap_or(tabs[0]);
    if !tabs.contains(&tab) {
        tab = tabs[0];
    }

    // Header.
    let fps = shown.canvas.frame_rate;
    if let [item] = items.as_slice() {
        let name = match (item.generated_fill(), media_of(item)) {
            (Some(Generated::Solid { .. }), _) => "Colour background".to_owned(),
            (Some(Generated::Gradient { .. }), _) => "Gradient background".to_owned(),
            (None, Some(m)) => m.name.clone(),
            (None, None) => "Missing media".to_owned(),
        };
        ui.label(RichText::new(name).strong());
        ui.label(
            RichText::new(format!(
                "{} · {}",
                format::timecode(item.start, fps),
                format::timecode(item.duration, fps)
            ))
            .small()
            .color(tokens.text_muted),
        );
    } else {
        ui.label(RichText::new(format!("{} clips selected", items.len())).strong());
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for t in &tabs {
            if ui.selectable_label(tab == *t, t.label()).clicked() {
                tab = *t;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(tab_id, tab));
    ui.separator();

    let mut e = Edits {
        session,
        ids: ids.clone(),
        out: Vec::new(),
    };
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| match tab {
            Tab::Transform => transform_tab(ui, &mut e, &first.look, &shown, &first, tokens),
            Tab::Colour => colour_tab(ui, &mut e, &first.look.color),
            Tab::Filters => filters_tab(ui, &mut e, &first.look, tokens),
            Tab::Effects => effects_tab(ui, &mut e, &first.look, first.id, tokens),
            Tab::Speed => speed_tab(ui, &mut e, &retimable, audible, fps),
            Tab::Fade => fade_tab(ui, &mut e, &items),
            Tab::Audio => audio_tab(ui, &mut e, &items, &shown),
            Tab::Background => background_tab(ui, &mut e, first.generated_fill()),
        });
    e.out
}

/// A percentage field for a 0–1 fraction.
fn pct(v: &mut f32) -> egui::DragValue<'_> {
    egui::DragValue::new(v)
        .speed(0.002)
        .custom_formatter(|v, _| format!("{:.0} %", v * 100.0))
        .custom_parser(|s| {
            s.trim()
                .trim_end_matches('%')
                .trim()
                .parse::<f64>()
                .ok()
                .map(|v| v / 100.0)
        })
}

fn transform_tab(
    ui: &mut egui::Ui,
    e: &mut Edits<'_>,
    look: &Look,
    project: &rw_model::Project,
    item: &Item,
    tokens: Tokens,
) {
    let t = look.transform;
    let with = |f: &dyn Fn(&mut Transform)| {
        let mut n = t;
        f(&mut n);
        LookPatch {
            transform: Some(n),
            ..LookPatch::default()
        }
    };
    ui.horizontal(|ui| {
        ui.label("Fit");
        for fit in Fit::ALL {
            if ui.selectable_label(t.fit == fit, fit.label()).clicked() {
                e.commit(with(&|n| n.fit = fit));
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Layout");
        egui::ComboBox::from_id_salt("layout")
            .selected_text("Choose…")
            .show_ui(ui, |ui| {
                for l in Layout::ALL {
                    if ui.selectable_label(false, l.label()).clicked() {
                        e.commit(with(&|n| *n = l.apply(*n)));
                    }
                }
            });
    });
    egui::Grid::new("transform")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Position");
            ui.horizontal(|ui| {
                let (mut x, mut y) = (t.center[0], t.center[1]);
                let rx = ui.add(pct(&mut x)).on_hover_text("Horizontal centre");
                e.live(&rx, with(&|n| n.center[0] = x));
                let ry = ui.add(pct(&mut y)).on_hover_text("Vertical centre");
                e.live(&ry, with(&|n| n.center[1] = y));
            });
            ui.end_row();
            ui.label("Scale");
            let mut scale = t.scale();
            let r = ui.add(
                egui::Slider::new(&mut scale, 0.05..=3.0)
                    .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
            );
            let ratio = scale / t.scale().max(1e-4);
            e.live(
                &r,
                with(&|n| n.size = [t.size[0] * ratio, t.size[1] * ratio]),
            );
            ui.end_row();
            ui.label("Width / height");
            ui.horizontal(|ui| {
                let (mut w, mut h) = (t.size[0], t.size[1]);
                let rw = ui.add(pct(&mut w));
                e.live(&rw, with(&|n| n.size[0] = w));
                let rh = ui.add(pct(&mut h));
                e.live(&rh, with(&|n| n.size[1] = h));
            });
            ui.end_row();
            ui.label("Rotation");
            let mut rot = t.rotation;
            let r = ui.add(
                egui::Slider::new(&mut rot, 0.0..=359.9)
                    .suffix("°")
                    .max_decimals(1),
            );
            e.live(&r, with(&|n| n.rotation = rot));
            ui.end_row();
            ui.label("Flip");
            ui.horizontal(|ui| {
                if ui.selectable_label(t.flip_h, "Horizontal").clicked() {
                    e.commit(with(&|n| n.flip_h = !t.flip_h));
                }
                if ui.selectable_label(t.flip_v, "Vertical").clicked() {
                    e.commit(with(&|n| n.flip_v = !t.flip_v));
                }
            });
            ui.end_row();
            ui.label("Opacity");
            let mut op = look.opacity;
            let r = ui.add(
                egui::Slider::new(&mut op, 0.0..=1.0)
                    .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
            );
            e.live(
                &r,
                LookPatch {
                    opacity: Some(op),
                    ..LookPatch::default()
                },
            );
            ui.end_row();
            ui.label("Blend");
            egui::ComboBox::from_id_salt("blend")
                .selected_text(look.blend.label())
                .show_ui(ui, |ui| {
                    for b in BlendMode::ALL {
                        if ui.selectable_label(look.blend == b, b.label()).clicked() {
                            e.commit(LookPatch {
                                blend: Some(b),
                                ..LookPatch::default()
                            });
                        }
                    }
                });
            ui.end_row();
        });
    if reset_button(
        ui,
        !t.is_identity() || look.opacity != 1.0 || look.blend != BlendMode::Normal,
    ) {
        e.commit(LookPatch {
            transform: Some(Transform::default()),
            opacity: Some(1.0),
            blend: Some(BlendMode::Normal),
            ..LookPatch::default()
        });
    }

    ui.add_space(10.0);
    ui.label(RichText::new("Crop").strong());
    let display = item
        .media()
        .and_then(|m| project.media(m))
        .and_then(|m| m.info.video.as_ref().map(|v| v.display_size()))
        .unwrap_or((project.canvas.width, project.canvas.height));
    ui.horizontal_wrapped(|ui| {
        for (label, ratio) in CROP_RATIOS {
            if ui
                .button(*label)
                .on_hover_text("Crop to this shape")
                .clicked()
            {
                e.commit(LookPatch {
                    crop: Some(Crop::to_ratio(display, *ratio)),
                    ..LookPatch::default()
                });
            }
        }
    });
    let c = look.crop;
    egui::Grid::new("crop")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            for (label, value) in [
                ("Left", c.left),
                ("Top", c.top),
                ("Right", c.right),
                ("Bottom", c.bottom),
            ] {
                ui.label(label);
                let mut v = value;
                let r = ui.add(
                    egui::Slider::new(&mut v, 0.0..=0.95)
                        .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
                );
                let mut n = c;
                match label {
                    "Left" => n.left = v,
                    "Top" => n.top = v,
                    "Right" => n.right = v,
                    _ => n.bottom = v,
                }
                e.live(
                    &r,
                    LookPatch {
                        crop: Some(n.sanitized()),
                        ..LookPatch::default()
                    },
                );
                ui.end_row();
            }
        });
    if reset_button(ui, !c.is_none()) {
        e.commit(LookPatch {
            crop: Some(Crop::default()),
            ..LookPatch::default()
        });
    }
    ui.label(
        RichText::new("Tip: drag the handles in the preview to move, scale and rotate.")
            .small()
            .color(tokens.text_muted),
    );
}

fn colour_tab(ui: &mut egui::Ui, e: &mut Edits<'_>, color: &ColorAdjust) {
    egui::Grid::new("colour")
        .num_columns(3)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            for c in ColorControl::ALL {
                ui.label(c.label());
                let mut v = color.get(c);
                let r = ui.add(egui::Slider::new(&mut v, c.range()).max_decimals(2));
                let mut n = *color;
                n.set(c, v);
                e.live(
                    &r,
                    LookPatch {
                        color: Some(n),
                        ..LookPatch::default()
                    },
                );
                if ui
                    .add_enabled(color.get(c) != 0.0, egui::Button::new("↺").small())
                    .on_hover_text("Reset")
                    .clicked()
                {
                    let mut n = *color;
                    n.set(c, 0.0);
                    e.commit(LookPatch {
                        color: Some(n),
                        ..LookPatch::default()
                    });
                }
                ui.end_row();
            }
        });
    if reset_button(ui, !color.is_neutral()) {
        e.commit(LookPatch {
            color: Some(ColorAdjust::default()),
            ..LookPatch::default()
        });
    }
}

fn filters_tab(ui: &mut egui::Ui, e: &mut Edits<'_>, look: &Look, tokens: Tokens) {
    let current = look.filter.as_ref().map(|f| f.id.as_str());
    let intensity = look.filter.as_ref().map_or(1.0, |f| f.intensity);
    if let Some(f) = &look.filter {
        ui.horizontal(|ui| {
            ui.label("Intensity");
            let mut v = f.intensity;
            let r = ui.add(
                egui::Slider::new(&mut v, 0.0..=1.0)
                    .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
            );
            e.live(
                &r,
                LookPatch {
                    filter: Some(Some(Filter {
                        id: f.id.clone(),
                        intensity: v,
                    })),
                    ..LookPatch::default()
                },
            );
        });
        ui.add_space(4.0);
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        if ui.selectable_label(current.is_none(), "None").clicked() {
            e.commit(LookPatch {
                filter: Some(None),
                ..LookPatch::default()
            });
        }
        for f in rw_fx::filters() {
            // A swatch shows what the filter does to a mid-range colour wheel.
            let selected = current == Some(f.id);
            let resp = filter_swatch(ui, f, selected, tokens);
            if resp.clicked() {
                e.commit(LookPatch {
                    filter: Some(Some(Filter {
                        id: f.id.to_owned(),
                        intensity,
                    })),
                    ..LookPatch::default()
                });
            }
        }
    });
}

/// A small chip with four sample colours run through the filter's table.
fn filter_swatch(
    ui: &mut egui::Ui,
    f: &rw_fx::FilterInfo,
    selected: bool,
    tokens: Tokens,
) -> Response {
    let size = egui::vec2(88.0, 50.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, f.name)
    });
    let p = ui.painter();
    let samples: [[f32; 3]; 4] = [
        [0.85, 0.35, 0.25],
        [0.3, 0.6, 0.35],
        [0.25, 0.45, 0.8],
        [0.9, 0.8, 0.6],
    ];
    let sw = rect.width() / 4.0;
    for (i, s) in samples.iter().enumerate() {
        let o = f.lut.sample(*s).map(|v| (v * 255.0).round() as u8);
        let r = egui::Rect::from_min_size(
            rect.min + egui::vec2(sw * i as f32, 0.0),
            egui::vec2(sw, 30.0),
        );
        p.rect_filled(r, 0.0, egui::Color32::from_rgb(o[0], o[1], o[2]));
    }
    p.text(
        rect.center_bottom() - egui::vec2(0.0, 10.0),
        egui::Align2::CENTER_CENTER,
        f.name,
        egui::FontId::proportional(11.0),
        tokens.text,
    );
    let stroke = if selected {
        egui::Stroke::new(2.0, tokens.accent)
    } else if resp.hovered() {
        egui::Stroke::new(1.0, tokens.text_muted)
    } else {
        egui::Stroke::new(1.0, tokens.border)
    };
    p.rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        stroke,
        egui::StrokeKind::Inside,
    );
    resp.on_hover_text(f.name)
}

/// Effects in order, each with its intensity and settings; a grid adds more.
fn effects_tab(ui: &mut egui::Ui, e: &mut Edits<'_>, look: &Look, item: ItemId, tokens: Tokens) {
    let effects = look.effects.clone();
    let with = |f: &dyn Fn(&mut Vec<Effect>)| {
        let mut v = effects.clone();
        f(&mut v);
        LookPatch {
            effects: Some(v),
            ..LookPatch::default()
        }
    };
    if effects.is_empty() {
        ui.label(
            RichText::new("No effects yet. Pick one below.")
                .small()
                .color(tokens.text_muted),
        );
    }
    for (i, fx) in effects.iter().enumerate() {
        let info = rw_fx::effects::effect_info(fx.kind);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(info.name).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if named(ui.small_button("×"), &format!("Remove {}", info.name))
                        .on_hover_text("Remove")
                        .clicked()
                    {
                        e.commit(with(&|v| {
                            v.remove(i);
                        }));
                    }
                    if named(
                        ui.add_enabled(i + 1 < effects.len(), egui::Button::new("↓").small()),
                        &format!("Apply {} later", info.name),
                    )
                    .on_hover_text("Apply later")
                    .clicked()
                    {
                        e.commit(with(&|v| v.swap(i, i + 1)));
                    }
                    if named(
                        ui.add_enabled(i > 0, egui::Button::new("↑").small()),
                        &format!("Apply {} earlier", info.name),
                    )
                    .on_hover_text("Apply earlier")
                    .clicked()
                    {
                        e.commit(with(&|v| v.swap(i - 1, i)));
                    }
                });
            });
            let params = rw_fx::effects::resolved_params(fx);
            egui::Grid::new(("effect", i))
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label(if fx.kind == EffectKind::ChromaKey {
                        "Tolerance"
                    } else {
                        "Intensity"
                    });
                    let mut v = fx.intensity;
                    let r = ui.add(
                        egui::Slider::new(&mut v, 0.0..=1.0)
                            .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
                    );
                    e.live(&r, with(&|fxs| fxs[i].intensity = v));
                    ui.end_row();
                    let skip = if fx.kind == EffectKind::ChromaKey {
                        3
                    } else {
                        0
                    };
                    for (k, p) in info.params.iter().enumerate().skip(skip) {
                        ui.label(p.name);
                        let mut v = params[k];
                        let r = ui.add(egui::Slider::new(&mut v, p.min..=p.max).max_decimals(2));
                        e.live(
                            &r,
                            with(&|fxs| {
                                let mut all = params[..info.params.len()].to_vec();
                                all[k] = v;
                                fxs[i].params = all;
                            }),
                        );
                        ui.end_row();
                    }
                });
            if fx.kind == EffectKind::ChromaKey {
                ui.horizontal(|ui| {
                    let key = Rgba([
                        (params[0] * 255.0).round() as u8,
                        (params[1] * 255.0).round() as u8,
                        (params[2] * 255.0).round() as u8,
                        255,
                    ]);
                    ui.label("Key colour");
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                    ui.painter().rect_filled(
                        rect,
                        4.0,
                        egui::Color32::from_rgb(key.0[0], key.0[1], key.0[2]),
                    );
                    ui.label(RichText::new(to_hex(key)).small());
                    let picking = e
                        .session
                        .canvas_tools
                        .picking_key
                        .is_some_and(|p| p.item == item && p.index == i);
                    if picking {
                        ui.label(RichText::new("Click the preview…").color(tokens.accent));
                        if ui.small_button("Cancel").clicked() {
                            e.session.canvas_tools.picking_key = None;
                            e.session.clear_preview_patch();
                        }
                    } else if ui
                        .button("Pick from preview")
                        .on_hover_text("Click a spot in the preview to key out its colour")
                        .clicked()
                    {
                        // Show the clip unkeyed while picking.
                        e.session.canvas_tools.picking_key =
                            Some(crate::canvas_tools::KeyPick { item, index: i });
                        e.session
                            .set_preview_patch(e.ids.clone(), with(&|fxs| fxs[i].intensity = 0.0));
                    }
                });
            }
        });
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Add effect").strong());
    ui.horizontal_wrapped(|ui| {
        for info in &rw_fx::effects::EFFECTS {
            if ui.button(info.name).clicked() {
                let new = rw_fx::effects::new_effect(info.kind);
                e.commit(with(&|v| v.push(new.clone())));
            }
        }
    });
}

/// Gives an icon-only button a name for screen readers.
fn named(r: Response, name: &str) -> Response {
    let enabled = r.enabled();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    r
}

/// Speed presets offered as buttons.
pub const SPEED_PRESETS: &[f32] = &[0.25, 0.5, 1.0, 1.5, 2.0, 4.0];

fn speed_tab(
    ui: &mut egui::Ui,
    e: &mut Edits<'_>,
    items: &[&Item],
    has_sound: bool,
    fps: rw_model::FrameRate,
) {
    let Some(first) = items.first() else { return };
    let ids: Vec<ItemId> = items.iter().map(|i| i.id).collect();
    let set = |e: &mut Edits<'_>, speed: f32| {
        e.out.push(EditorAction::Op(TimelineOp::SetSpeed {
            items: ids.clone(),
            speed,
        }));
    };
    ui.horizontal(|ui| {
        ui.label("Speed");
        let id = ui.make_persistent_id(("speed", &e.ids));
        let mut v: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(first.speed);
        let r = ui.add(
            egui::Slider::new(&mut v, MIN_SPEED..=MAX_SPEED)
                .logarithmic(true)
                .custom_formatter(|v, _| format!("{v:.2}×"))
                .custom_parser(|s| s.trim().trim_end_matches(['×', 'x']).trim().parse().ok()),
        );
        if r.changed() {
            ui.data_mut(|d| d.insert_temp(id, v));
        }
        if (r.drag_stopped() || (r.changed() && !r.dragged())) && (v - first.speed).abs() > 1e-4 {
            ui.data_mut(|d| d.remove::<f32>(id));
            set(e, v);
        }
    });
    ui.horizontal_wrapped(|ui| {
        for preset in SPEED_PRESETS {
            let on = (first.speed - preset).abs() < 1e-4;
            if ui.selectable_label(on, format!("{preset}×")).clicked() && !on {
                set(e, *preset);
            }
        }
    });
    ui.label(
        RichText::new(format!(
            "Plays for {}",
            format::timecode(first.duration, fps)
        ))
        .small(),
    );
    ui.add_space(6.0);
    let mut reverse = items.iter().all(|i| i.reverse);
    if ui.checkbox(&mut reverse, "Play backwards").changed() {
        e.out.push(EditorAction::Op(TimelineOp::SetReverse {
            items: ids.clone(),
            reverse,
        }));
    }
    if has_sound {
        let mut keep = items.iter().all(|i| i.keep_pitch);
        if ui
            .checkbox(&mut keep, "Keep the pitch of the sound")
            .on_hover_text("Off: sound gets higher when faster and lower when slower")
            .changed()
        {
            e.out.push(EditorAction::Op(TimelineOp::SetKeepPitch {
                items: ids.clone(),
                keep,
            }));
        }
    }
    ui.add_space(6.0);
    if ui
        .button("Freeze frame at playhead")
        .on_hover_text("Holds the frame under the playhead for 2 seconds")
        .clicked()
    {
        e.out.push(EditorAction::FreezeFrame);
    }
}

/// Kind and length of the transition picked on the timeline.
fn transition_panel(
    ui: &mut egui::Ui,
    session: &mut Session,
    id: TransitionId,
    tokens: Tokens,
) -> Vec<EditorAction> {
    let mut out = Vec::new();
    let Some(tr) = session
        .project
        .tracks
        .iter()
        .flat_map(|t| &t.transitions)
        .find(|t| t.id == id)
        .cloned()
    else {
        session.timeline.selected_transition = None;
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new(EMPTY_LABEL).color(tokens.text_muted));
        });
        return out;
    };
    ui.label(RichText::new("Transition").strong());
    ui.label(
        RichText::new(tr.kind.label())
            .small()
            .color(tokens.text_muted),
    );
    ui.separator();
    egui::Grid::new("transition")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Style");
            egui::ComboBox::from_id_salt("transition-kind")
                .selected_text(tr.kind.label())
                .show_ui(ui, |ui| {
                    for k in TransitionKind::ALL {
                        if ui.selectable_label(tr.kind == k, k.label()).clicked() && k != tr.kind {
                            out.push(EditorAction::Op(TimelineOp::SetTransition {
                                id,
                                kind: Some(k),
                                duration: None,
                            }));
                        }
                    }
                });
            ui.end_row();
            ui.label("Duration");
            let data_id = ui.make_persistent_id(("transition-length", id));
            let current = tr.duration.as_secs_f64();
            let mut v: f64 = ui.data(|d| d.get_temp(data_id)).unwrap_or(current);
            let r = ui.add(
                egui::Slider::new(
                    &mut v,
                    MIN_TRANSITION.as_secs_f64()..=MAX_TRANSITION.as_secs_f64(),
                )
                .suffix(" s")
                .max_decimals(2),
            );
            if r.changed() {
                ui.data_mut(|d| d.insert_temp(data_id, v));
            }
            if (r.drag_stopped() || (r.changed() && !r.dragged())) && (v - current).abs() > 1e-3 {
                ui.data_mut(|d| d.remove::<f64>(data_id));
                out.push(EditorAction::Op(TimelineOp::SetTransition {
                    id,
                    kind: None,
                    duration: Some(Time::from_secs_f64(v)),
                }));
            }
            ui.end_row();
        });
    ui.label(
        RichText::new("Also drag the transition's edges on the timeline. It can't be longer than twice the shorter clip.")
            .small()
            .color(tokens.text_muted),
    );
    if ui.button("Remove transition").clicked() {
        out.push(EditorAction::Op(TimelineOp::RemoveTransition { id }));
        session.timeline.selected_transition = None;
    }
    out
}

fn fade_tab(ui: &mut egui::Ui, e: &mut Edits<'_>, items: &[Item]) {
    let shortest = items.iter().map(|i| i.duration).min().unwrap_or(Time::ZERO);
    let max = (shortest.as_secs_f64() / 2.0).clamp(0.1, 10.0);
    let first = &items[0].look;
    egui::Grid::new("fade")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Fade in");
            let mut v = first.fade_in.as_secs_f64();
            let r = ui.add(
                egui::Slider::new(&mut v, 0.0..=max)
                    .suffix(" s")
                    .max_decimals(2),
            );
            e.live(
                &r,
                LookPatch {
                    fade_in: Some(Time::from_secs_f64(v)),
                    ..LookPatch::default()
                },
            );
            ui.end_row();
            ui.label("Fade out");
            let mut v = first.fade_out.as_secs_f64();
            let r = ui.add(
                egui::Slider::new(&mut v, 0.0..=max)
                    .suffix(" s")
                    .max_decimals(2),
            );
            e.live(
                &r,
                LookPatch {
                    fade_out: Some(Time::from_secs_f64(v)),
                    ..LookPatch::default()
                },
            );
            ui.end_row();
        });
    ui.label(RichText::new("Fades apply to both picture and sound.").small());
}

fn audio_tab(ui: &mut egui::Ui, e: &mut Edits<'_>, items: &[Item], project: &rw_model::Project) {
    let media_of = |i: &Item| i.media().and_then(|m| project.media(m));
    let audible: Vec<&Item> = items
        .iter()
        .filter(|i| media_of(i).is_some_and(|m| m.info.audio.is_some()))
        .collect();
    let Some(first) = audible.first() else { return };
    ui.horizontal(|ui| {
        ui.label(VOLUME_LABEL);
        let id = ui.make_persistent_id(("volume", &e.ids));
        let current = gain_to_db(first.gain);
        let mut db: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(current);
        let slider = egui::Slider::new(&mut db, MIN_DB..=12.0)
            .suffix(" dB")
            .custom_formatter(|v, _| {
                if v <= f64::from(MIN_DB) {
                    "−∞".into()
                } else {
                    format!("{v:+.1}")
                }
            });
        let r = ui.add(slider);
        if r.changed() {
            ui.data_mut(|d| d.insert_temp(id, db));
        }
        if (r.drag_stopped() || (r.changed() && !r.dragged())) && (db - current).abs() > 0.01 {
            ui.data_mut(|d| d.remove::<f32>(id));
            e.out.push(EditorAction::Op(TimelineOp::SetGain {
                items: e.ids.clone(),
                gain: db_to_gain(db),
            }));
        }
    });
    let mut muted = audible.iter().all(|i| i.muted);
    if ui.checkbox(&mut muted, "Mute").changed() {
        e.out.push(EditorAction::Op(TimelineOp::SetMuted {
            items: e.ids.clone(),
            muted,
        }));
    }
    let can_detach = items.iter().any(|i| {
        !i.audio_detached
            && media_of(i).is_some_and(|m| m.kind() == MediaKind::Video && m.info.audio.is_some())
    });
    if can_detach && ui.button("Detach audio").clicked() {
        e.out.push(EditorAction::DetachAudio);
    }
}

/// Hue (0–360), saturation and value (0–1) of an sRGB colour.
pub fn to_hsv(c: Rgba) -> (f32, f32, f32) {
    let [r, g, b, _] = c.0.map(|v| f32::from(v) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if max == 0.0 { 0.0 } else { d / max }, max)
}

pub fn from_hsv(h: f32, s: f32, v: f32) -> Rgba {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let q = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgba([q(r), q(g), q(b), 255])
}

pub fn to_hex(c: Rgba) -> String {
    format!("#{:02X}{:02X}{:02X}", c.0[0], c.0[1], c.0[2])
}

pub fn parse_hex(s: &str) -> Option<Rgba> {
    let s = s.trim().trim_start_matches('#');
    let s = if s.len() == 3 {
        s.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        s.to_owned()
    };
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(&s, 16).ok()?;
    Some(Rgba([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]))
}

/// HSV sliders plus a hex field for one colour. Returns the response that
/// changed (if any) and the new colour.
fn colour_editor(ui: &mut egui::Ui, id: &str, c: Rgba) -> (Option<Response>, Rgba) {
    let (h, s, v) = to_hsv(c);
    let (mut h2, mut s2, mut v2) = (h, s, v);
    let mut changed = None;
    let mut out = c;
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 4.0, egui::Color32::from_rgb(c.0[0], c.0[1], c.0[2]));
        let hex_id = ui.make_persistent_id(("hex", id));
        let mut text: String = ui.data(|d| d.get_temp(hex_id)).unwrap_or_else(|| to_hex(c));
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(80.0));
        if r.changed() {
            ui.data_mut(|d| d.insert_temp(hex_id, text.clone()));
        }
        if r.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(hex_id));
            if let Some(parsed) = parse_hex(&text) {
                out = parsed;
                changed = Some(r);
            }
        }
    });
    egui::Grid::new(("hsv", id)).num_columns(2).show(ui, |ui| {
        ui.label("Hue");
        let r = ui.add(egui::Slider::new(&mut h2, 0.0..=359.0).suffix("°"));
        if r.changed() || r.drag_stopped() {
            out = from_hsv(h2, s, v);
            changed = Some(r);
        }
        ui.end_row();
        ui.label("Saturation");
        let r = ui.add(egui::Slider::new(&mut s2, 0.0..=1.0));
        if r.changed() || r.drag_stopped() {
            out = from_hsv(h, s2, v);
            changed = Some(r);
        }
        ui.end_row();
        ui.label("Brightness");
        let r = ui.add(egui::Slider::new(&mut v2, 0.0..=1.0));
        if r.changed() || r.drag_stopped() {
            out = from_hsv(h, s, v2);
            changed = Some(r);
        }
        ui.end_row();
    });
    (changed, out)
}

fn background_tab(ui: &mut egui::Ui, e: &mut Edits<'_>, fill: Option<Generated>) {
    let Some(fill) = fill else { return };
    let gradient = matches!(fill, Generated::Gradient { .. });
    ui.horizontal(|ui| {
        if ui.selectable_label(!gradient, "Solid").clicked()
            && let Generated::Gradient { from, .. } = fill
        {
            e.out.push(EditorAction::Op(TimelineOp::SetFill {
                items: e.ids.clone(),
                fill: Generated::Solid { color: from },
            }));
        }
        if ui.selectable_label(gradient, "Gradient").clicked()
            && let Generated::Solid { color } = fill
        {
            let (h, s, v) = to_hsv(color);
            let to = from_hsv((h + 40.0) % 360.0, s, (v * 0.6).max(0.1));
            e.out.push(EditorAction::Op(TimelineOp::SetFill {
                items: e.ids.clone(),
                fill: Generated::Gradient {
                    from: color,
                    to,
                    angle: 90.0,
                },
            }));
        }
    });
    match fill {
        Generated::Solid { color } => {
            let (r, c) = colour_editor(ui, "solid", color);
            if let Some(r) = r {
                e.live_fill(&r, Generated::Solid { color: c });
            }
        }
        Generated::Gradient { from, to, angle } => {
            ui.label("From");
            let (r, c) = colour_editor(ui, "from", from);
            if let Some(r) = r {
                e.live_fill(&r, Generated::Gradient { from: c, to, angle });
            }
            ui.label("To");
            let (r, c) = colour_editor(ui, "to", to);
            if let Some(r) = r {
                e.live_fill(&r, Generated::Gradient { from, to: c, angle });
            }
            ui.horizontal(|ui| {
                ui.label("Angle");
                let mut a = angle;
                let r = ui.add(egui::Slider::new(&mut a, 0.0..=359.0).suffix("°"));
                if r.changed() || r.drag_stopped() {
                    e.live_fill(&r, Generated::Gradient { from, to, angle: a });
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_round_trip() {
        assert_eq!(db_to_gain(0.0), 1.0);
        assert!((gain_to_db(db_to_gain(-6.0)) + 6.0).abs() < 1e-4);
        assert_eq!(db_to_gain(-60.0), 0.0);
        assert_eq!(gain_to_db(0.0), -60.0);
        assert!((db_to_gain(12.0) - MAX_GAIN).abs() < 1e-4);
    }

    #[test]
    fn hsv_and_hex_round_trip() {
        for c in [
            Rgba([255, 0, 0, 255]),
            Rgba([14, 124, 116, 255]),
            Rgba([242, 165, 65, 255]),
            Rgba([0, 0, 0, 255]),
        ] {
            let (h, s, v) = to_hsv(c);
            assert_eq!(from_hsv(h, s, v), c);
            assert_eq!(parse_hex(&to_hex(c)), Some(c));
        }
        assert_eq!(parse_hex("#fff"), Some(Rgba([255, 255, 255, 255])));
        assert_eq!(parse_hex("nope"), None);
    }
}
