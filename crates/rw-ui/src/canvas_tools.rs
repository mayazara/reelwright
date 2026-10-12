//! On-canvas editing in the preview: floating toolbar, selection by click,
//! and handles to move, scale, rotate and crop the selected clip.

use crate::actions::EditorAction;
use crate::session::Session;
use crate::theme::Tokens;
use eframe::egui::{
    self, Align2, CornerRadius, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
    pos2, vec2,
};
use rw_edit::timeline::{LookPatch, TimelineOp};
use rw_model::{Fit, Item, ItemId, Layout, Look, MediaKind, Project, Time};

pub const CROP_LABEL: &str = "Crop";
const HANDLE: f32 = 7.0;
const ROTATE_OFFSET: f32 = 26.0;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Grip {
    Move,
    /// Corner, signs of local x and y.
    Corner(f32, f32),
    /// Edge on the local x (0) or y (1) axis, with its sign.
    Edge(usize, f32),
    Rotate,
}

#[derive(Debug, Clone)]
struct Drag {
    item: ItemId,
    grip: Grip,
    start: Pos2,
    look: Look,
    geom: Geometry,
    /// Text keeps its transform's size as a scale of its natural size.
    text: bool,
}

#[derive(Debug, Default)]
pub struct CanvasState {
    pub crop_mode: bool,
    /// Waiting for a click in the preview to set a green-screen colour.
    pub picking_key: Option<KeyPick>,
    drag: Option<Drag>,
}

/// The green-screen effect whose key colour the next preview click sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPick {
    pub item: ItemId,
    /// Position in the clip's effect list.
    pub index: usize,
}

/// Where a clip's visible picture is, in canvas pixels.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    center: Vec2,
    half: Vec2,
    /// Clockwise, radians.
    angle: f32,
    /// Size of the whole uncropped picture at this scale (for crop drags).
    full: Vec2,
}

impl Geometry {
    fn axes(self) -> (Vec2, Vec2) {
        let (s, c) = self.angle.sin_cos();
        (vec2(c, s), vec2(-s, c))
    }

    fn to_canvas(self, local: Vec2) -> Vec2 {
        let (ax, ay) = self.axes();
        self.center + ax * local.x + ay * local.y
    }

    fn to_local(self, p: Vec2) -> Vec2 {
        let (ax, ay) = self.axes();
        let d = p - self.center;
        vec2(d.dot(ax), d.dot(ay))
    }

    fn contains(self, p: Vec2) -> bool {
        let l = self.to_local(p);
        l.x.abs() <= self.half.x && l.y.abs() <= self.half.y
    }
}

fn display_size(project: &Project, item: &Item) -> Option<(f32, f32)> {
    if item.generated_fill().is_some() {
        return Some((project.canvas.width as f32, project.canvas.height as f32));
    }
    let m = item.media().and_then(|id| project.media(id))?;
    if m.kind() == MediaKind::Audio {
        return None;
    }
    m.info.video.as_ref().map(|v| {
        let (w, h) = v.display_size();
        (w as f32, h as f32)
    })
}

fn geometry(project: &Project, item: &Item, at: Time) -> Option<Geometry> {
    let canvas = (project.canvas.width, project.canvas.height);
    if item.has_natural_size() {
        let local = (at - item.start).clamp(Time::ZERO, item.duration);
        let p = rw_engine::graphics::place(item, local, canvas, None)?;
        let half = vec2(p.size[0] * canvas.0 as f32, p.size[1] * canvas.1 as f32) / 2.0;
        return Some(Geometry {
            center: vec2(p.center[0] * canvas.0 as f32, p.center[1] * canvas.1 as f32),
            half,
            angle: item.look.transform.rotation.to_radians(),
            full: half * 2.0,
        });
    }
    let display = display_size(project, item)?;
    let look = &item.look;
    let t = look.transform;
    let generated = item.generated_fill().is_some();
    let content = rw_render::content_size(display, look, canvas, generated);
    let boxed = vec2(t.size[0] * canvas.0 as f32, t.size[1] * canvas.1 as f32);
    // Fill shows only the part inside the box.
    let visible = if t.fit == Fit::Cover && !generated {
        vec2(content.0.min(boxed.x), content.1.min(boxed.y))
    } else {
        vec2(content.0, content.1)
    };
    let (kx, ky) = look.crop.kept();
    Some(Geometry {
        center: vec2(t.center[0] * canvas.0 as f32, t.center[1] * canvas.1 as f32),
        half: visible / 2.0,
        angle: t.rotation.to_radians(),
        full: vec2(content.0 / kx.max(1e-3), content.1 / ky.max(1e-3)),
    })
}

/// Visual clips at `t`, topmost first.
fn visual_items_at(project: &Project, t: Time) -> Vec<&Item> {
    let mut v = rw_engine::frames::visible_items(project, t);
    v.reverse();
    v
}

fn apply_drag(d: &Drag, p: Vec2, canvas: Vec2, shift: bool, crop_mode: bool) -> LookPatch {
    let g = d.geom;
    let start = d.start.to_vec2();
    let mut t = d.look.transform;
    let mut crop = d.look.crop;
    match d.grip {
        Grip::Move => {
            let delta = (p - start) / canvas;
            t.center = [t.center[0] + delta.x, t.center[1] + delta.y];
        }
        Grip::Rotate => {
            let a0 = (start - g.center).angle();
            let a1 = (p - g.center).angle();
            let mut deg = t.rotation + (a1 - a0).to_degrees();
            if shift {
                deg = (deg / 15.0).round() * 15.0;
            }
            t.rotation = deg.rem_euclid(360.0);
        }
        Grip::Corner(sx, sy) if !crop_mode => {
            let k = ((p - g.center).length() / (start - g.center).length().max(1.0)).max(0.02);
            let _ = (sx, sy);
            t.size = [t.size[0] * k, t.size[1] * k];
        }
        Grip::Edge(axis, sign) if !crop_mode => {
            let l0 = g.to_local(start)[axis] * sign;
            let l1 = g.to_local(p)[axis] * sign;
            let k = (l1 / l0.max(1.0)).max(0.02);
            // Make the box match what's visible first, so stretching one
            // side feels direct whatever the fit was.
            if t.fit == Fit::Contain && !d.text {
                t.size = [g.half.x * 2.0 / canvas.x, g.half.y * 2.0 / canvas.y];
                t.fit = Fit::Stretch;
            }
            t.size[axis] *= k;
        }
        Grip::Corner(sx, sy) => {
            let d_local = g.to_local(p) - g.to_local(start);
            if sx < 0.0 {
                crop.left += d_local.x / g.full.x;
            } else {
                crop.right -= d_local.x / g.full.x;
            }
            if sy < 0.0 {
                crop.top += d_local.y / g.full.y;
            } else {
                crop.bottom -= d_local.y / g.full.y;
            }
        }
        Grip::Edge(axis, sign) => {
            let d_local = (g.to_local(p) - g.to_local(start))[axis];
            match (axis, sign < 0.0) {
                (0, true) => crop.left += d_local / g.full.x,
                (0, false) => crop.right -= d_local / g.full.x,
                (_, true) => crop.top += d_local / g.full.y,
                (_, false) => crop.bottom -= d_local / g.full.y,
            }
        }
    }
    if crop != d.look.crop {
        LookPatch {
            crop: Some(crop.sanitized()),
            ..LookPatch::default()
        }
    } else {
        LookPatch {
            transform: Some(t.sanitized()),
            ..LookPatch::default()
        }
    }
}

fn toolbar(
    ui: &mut egui::Ui,
    area: Rect,
    session: &Session,
    state: &mut CanvasState,
    item: &Item,
    tokens: Tokens,
    out: &mut Vec<EditorAction>,
) {
    let id = item.id;
    let t = item.look.transform;
    let commit = |out: &mut Vec<EditorAction>, transform| {
        out.push(EditorAction::Op(TimelineOp::Look {
            items: vec![id],
            patch: LookPatch {
                transform: Some(transform),
                ..LookPatch::default()
            },
        }));
    };
    let bar = Rect::from_center_size(
        pos2(area.center().x, area.top() + 20.0),
        vec2(area.width().min(560.0), 32.0),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    egui::Frame::new()
        .fill(tokens.surface_raised.gamma_multiply(0.95))
        .stroke(Stroke::new(1.0, tokens.border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(&mut child, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(state.crop_mode, CROP_LABEL)
                    .on_hover_text("Drag the edges to crop")
                    .clicked()
                {
                    state.crop_mode = !state.crop_mode;
                }
                ui.separator();
                if ui
                    .button("Flip ↔")
                    .on_hover_text("Flip horizontally")
                    .clicked()
                {
                    commit(
                        out,
                        rw_model::Transform {
                            flip_h: !t.flip_h,
                            ..t
                        },
                    );
                }
                if ui
                    .button("Flip ↕")
                    .on_hover_text("Flip vertically")
                    .clicked()
                {
                    commit(
                        out,
                        rw_model::Transform {
                            flip_v: !t.flip_v,
                            ..t
                        },
                    );
                }
                if ui.button("Rotate 90°").clicked() {
                    commit(
                        out,
                        rw_model::Transform {
                            rotation: (t.rotation + 90.0).rem_euclid(360.0),
                            ..t
                        },
                    );
                }
                ui.separator();
                for fit in Fit::ALL {
                    if ui.selectable_label(t.fit == fit, fit.label()).clicked() {
                        commit(out, rw_model::Transform { fit, ..t });
                    }
                }
                ui.separator();
                ui.menu_button("Layout", |ui| {
                    for l in Layout::ALL {
                        if ui.button(l.label()).clicked() {
                            commit(out, l.apply(t));
                            ui.close();
                        }
                    }
                });
            });
        });
    let _ = session;
}

/// Draws handles over the preview image (`image` is where the canvas is on
/// screen) and handles pointer input. Returns edits.
pub fn show(
    ui: &mut egui::Ui,
    panel: Rect,
    image: Rect,
    session: &mut Session,
    state: &mut CanvasState,
    tokens: Tokens,
) -> Vec<EditorAction> {
    let mut out = Vec::new();
    if session.player.is_playing() {
        return out;
    }
    let shown = session.preview_project().into_owned();
    let canvas = vec2(shown.canvas.width as f32, shown.canvas.height as f32);
    let scale = image.size() / canvas;
    let to_screen = |v: Vec2| image.min + v * scale;
    let to_canvas = |p: Pos2| (p - image.min) / scale;
    let playhead = session.playhead();
    let items = visual_items_at(&shown, playhead);
    let selected: Option<&Item> = items
        .iter()
        .copied()
        .find(|i| session.selection.contains(&i.id));

    let resp = ui.interact(image, ui.id().with("canvas-tools"), Sense::click_and_drag());
    let painter = ui.painter_at(panel);

    if let Some(pick) = state.picking_key {
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let uv = to_canvas(p) / canvas;
            let colour = session
                .preview
                .as_ref()
                .and_then(|pv| pv.sample(uv.x, uv.y));
            state.picking_key = None;
            session.clear_preview_patch();
            if let (Some(c), Some((_, item))) = (colour, session.project.find_item(pick.item))
                && let Some(fx) = item.look.effects.get(pick.index)
            {
                let mut effects = item.look.effects.clone();
                let mut params = rw_fx::effects::resolved_params(fx)
                    [..rw_fx::effects::effect_info(fx.kind).params.len()]
                    .to_vec();
                for (k, v) in c.0[..3].iter().enumerate() {
                    params[k] = f32::from(*v) / 255.0;
                }
                effects[pick.index].params = params;
                out.push(EditorAction::Op(TimelineOp::Look {
                    items: vec![pick.item],
                    patch: rw_edit::timeline::LookPatch {
                        effects: Some(effects),
                        ..Default::default()
                    },
                }));
            }
        }
        return out;
    }

    if let Some(item) = selected
        && let Some(g) = geometry(&shown, item, playhead)
    {
        // Outline and handles.
        let corners: Vec<Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|(sx, sy)| to_screen(g.to_canvas(vec2(sx * g.half.x, sy * g.half.y))))
            .collect();
        let color = if state.crop_mode {
            tokens.highlight
        } else {
            tokens.accent
        };
        painter.add(egui::Shape::closed_line(
            corners.clone(),
            Stroke::new(1.5, color),
        ));
        let mut grips: Vec<(Grip, Pos2)> = Vec::new();
        for (i, (sx, sy)) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .enumerate()
        {
            grips.push((Grip::Corner(*sx, *sy), corners[i]));
        }
        for (axis, sign) in [(0usize, -1.0f32), (0, 1.0), (1, -1.0), (1, 1.0)] {
            let mut l = Vec2::ZERO;
            l[axis] = sign * g.half[axis];
            grips.push((Grip::Edge(axis, sign), to_screen(g.to_canvas(l))));
        }
        let top_mid = to_screen(g.to_canvas(vec2(0.0, -g.half.y)));
        let up = (top_mid - to_screen(g.center)).normalized();
        let knob = top_mid + up * ROTATE_OFFSET;
        if !state.crop_mode {
            painter.line_segment([top_mid, knob], Stroke::new(1.0, color));
            grips.push((Grip::Rotate, knob));
        }
        for (grip, p) in &grips {
            match grip {
                Grip::Rotate => {
                    painter.circle(*p, HANDLE, tokens.surface_raised, Stroke::new(1.5, color));
                }
                _ => {
                    painter.rect(
                        Rect::from_center_size(*p, Vec2::splat(HANDLE * 1.6)),
                        CornerRadius::same(2),
                        tokens.surface_raised,
                        Stroke::new(1.5, color),
                        StrokeKind::Middle,
                    );
                }
            }
        }
        // Cursor feedback.
        if let Some(hp) = resp.hover_pos()
            && state.drag.is_none()
        {
            let over = grips
                .iter()
                .find(|(_, p)| p.distance(hp) <= HANDLE + 2.0)
                .map(|(g, _)| *g);
            let icon = match over {
                Some(Grip::Rotate) => CursorIcon::Alias,
                Some(Grip::Corner(..)) => CursorIcon::ResizeNwSe,
                Some(Grip::Edge(0, _)) => CursorIcon::ResizeHorizontal,
                Some(Grip::Edge(..)) => CursorIcon::ResizeVertical,
                _ if g.contains(to_canvas(hp)) => CursorIcon::Move,
                _ => CursorIcon::Default,
            };
            ui.ctx().set_cursor_icon(icon);
        }
        if resp.drag_started()
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
        {
            let grip = grips
                .iter()
                .find(|(_, gp)| gp.distance(p) <= HANDLE + 2.0)
                .map(|(g, _)| *g)
                .or_else(|| g.contains(to_canvas(p)).then_some(Grip::Move));
            if let Some(grip) = grip {
                state.drag = Some(Drag {
                    item: item.id,
                    grip,
                    start: to_canvas(p).to_pos2(),
                    look: item.look.clone(),
                    geom: g,
                    text: item.has_natural_size(),
                });
            }
        }
        if state.crop_mode {
            painter.text(
                image.left_bottom() + vec2(8.0, -8.0),
                Align2::LEFT_BOTTOM,
                "Cropping — drag the edges",
                FontId::proportional(12.0),
                tokens.highlight,
            );
        }
        toolbar(ui, panel, session, state, item, tokens, &mut out);
    }

    if let Some(d) = &state.drag
        && let Some(p) = ui.input(|i| i.pointer.interact_pos())
    {
        let shift = ui.input(|i| i.modifiers.shift);
        let patch = apply_drag(d, to_canvas(p), canvas, shift, state.crop_mode);
        let item = d.item;
        if resp.drag_stopped() {
            state.drag = None;
            session.clear_preview_patch();
            out.push(EditorAction::Op(TimelineOp::Look {
                items: vec![item],
                patch,
            }));
        } else {
            session.set_preview_patch(vec![item], patch);
        }
    } else if resp.drag_stopped() {
        state.drag = None;
    }

    // Clicking a clip on the canvas selects it.
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let c = to_canvas(p);
        let hit = items
            .iter()
            .find(|i| geometry(&shown, i, playhead).is_some_and(|g| g.contains(c)));
        session.selection.clear();
        if let Some(i) = hit {
            session.selection.insert(i.id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rw_model::Transform;

    fn drag(grip: Grip, look: Look, geom: Geometry, start: Pos2) -> Drag {
        Drag {
            item: ItemId::new(),
            grip,
            start,
            look,
            geom,
            text: false,
        }
    }

    fn geom(look: &Look) -> Geometry {
        // A 1600x900 picture on a 1600x900 canvas.
        let content = rw_render::content_size((1600.0, 900.0), look, (1600, 900), false);
        let t = look.transform;
        let (kx, ky) = look.crop.kept();
        Geometry {
            center: vec2(t.center[0] * 1600.0, t.center[1] * 900.0),
            half: vec2(content.0, content.1) / 2.0,
            angle: t.rotation.to_radians(),
            full: vec2(content.0 / kx, content.1 / ky),
        }
    }

    const CANVAS: Vec2 = vec2(1600.0, 900.0);

    #[test]
    fn move_scale_rotate() {
        let look = Look::default();
        let g = geom(&look);
        let m = apply_drag(
            &drag(Grip::Move, look.clone(), g, pos2(800.0, 450.0)),
            vec2(960.0, 540.0),
            CANVAS,
            false,
            false,
        );
        let t = m.transform.unwrap();
        assert!((t.center[0] - 0.6).abs() < 1e-4 && (t.center[1] - 0.6).abs() < 1e-4);

        // Dragging a corner halfway to the centre halves the size.
        let s = apply_drag(
            &drag(Grip::Corner(1.0, 1.0), look.clone(), g, pos2(1600.0, 900.0)),
            vec2(1200.0, 675.0),
            CANVAS,
            false,
            false,
        );
        assert!((s.transform.unwrap().size[0] - 0.5).abs() < 1e-3);

        // A quarter turn around the centre; Shift snaps.
        let r = apply_drag(
            &drag(Grip::Rotate, look.clone(), g, pos2(800.0, 100.0)),
            vec2(1150.0, 455.0),
            CANVAS,
            true,
            false,
        );
        assert_eq!(r.transform.unwrap().rotation, 90.0);
    }

    #[test]
    fn edge_drag_stretches_and_crop_mode_crops() {
        let look = Look {
            transform: Transform {
                size: [1.0, 1.0],
                ..Transform::default()
            },
            ..Look::default()
        };
        let g = geom(&look);
        let e = apply_drag(
            &drag(Grip::Edge(0, 1.0), look.clone(), g, pos2(1600.0, 450.0)),
            vec2(1200.0, 450.0),
            CANVAS,
            false,
            false,
        );
        let t = e.transform.unwrap();
        assert_eq!(t.fit, Fit::Stretch);
        assert!((t.size[0] - 0.5).abs() < 1e-3 && (t.size[1] - 1.0).abs() < 1e-3);

        // In crop mode, pulling the left edge in by a quarter crops 25 %.
        let c = apply_drag(
            &drag(Grip::Edge(0, -1.0), look.clone(), g, pos2(0.0, 450.0)),
            vec2(400.0, 450.0),
            CANVAS,
            false,
            true,
        );
        let crop = c.crop.unwrap();
        assert!((crop.left - 0.25).abs() < 1e-3 && crop.right == 0.0);
    }
}
