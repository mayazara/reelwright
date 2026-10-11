//! The Backgrounds tab: colour and gradient clips.

use crate::theme::Tokens;
use eframe::egui::{
    self, Color32, CornerRadius, Mesh, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2,
};
use rw_model::{Generated, Rgba};

/// Original preset colours: name and sRGB.
pub const SOLIDS: &[(&str, [u8; 3])] = &[
    ("Black", [0, 0, 0]),
    ("White", [255, 255, 255]),
    ("Charcoal", [38, 42, 46]),
    ("Cream", [246, 238, 222]),
    ("Teal", [14, 124, 116]),
    ("Amber", [242, 165, 65]),
    ("Coral", [236, 104, 88]),
    ("Sky", [120, 176, 232]),
    ("Forest", [38, 92, 60]),
    ("Plum", [94, 48, 108]),
    ("Sand", [214, 190, 150]),
    ("Slate", [92, 106, 122]),
];

/// Original preset gradients: name, from, to, angle.
pub const GRADIENTS: &[(&str, [u8; 3], [u8; 3], f32)] = &[
    ("Dusk", [94, 48, 108], [242, 165, 65], 90.0),
    ("Lagoon", [14, 124, 116], [120, 176, 232], 45.0),
    ("Ember glow", [236, 104, 88], [242, 165, 65], 0.0),
    ("Mist", [92, 106, 122], [246, 238, 222], 90.0),
    ("Night", [10, 14, 30], [52, 60, 110], 90.0),
    ("Meadow", [38, 92, 60], [190, 214, 120], 135.0),
    ("Blush", [246, 206, 214], [236, 104, 88], 45.0),
    ("Steel", [38, 42, 46], [150, 160, 172], 0.0),
];

pub fn all() -> Vec<(&'static str, Generated)> {
    let rgba = |c: [u8; 3]| Rgba([c[0], c[1], c[2], 255]);
    SOLIDS
        .iter()
        .map(|(n, c)| (*n, Generated::Solid { color: rgba(*c) }))
        .chain(GRADIENTS.iter().map(|(n, a, b, angle)| {
            (
                *n,
                Generated::Gradient {
                    from: rgba(*a),
                    to: rgba(*b),
                    angle: *angle,
                },
            )
        }))
        .collect()
}

fn c32(c: Rgba) -> Color32 {
    Color32::from_rgb(c.0[0], c.0[1], c.0[2])
}

/// Paints a fill into `rect` (gradients as a two-colour mesh).
pub fn paint(p: &egui::Painter, rect: Rect, fill: Generated) {
    match fill {
        Generated::Solid { color } => {
            p.rect_filled(rect, CornerRadius::same(4), c32(color));
        }
        Generated::Gradient { from, to, angle } => {
            let dir = vec2(angle.to_radians().cos(), angle.to_radians().sin());
            let span = dir.x.abs() * 0.5 + dir.y.abs() * 0.5;
            let mut mesh = Mesh::default();
            let corners = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
            ];
            for c in corners {
                let uv = Pos2::new(
                    (c.x - rect.left()) / rect.width(),
                    (c.y - rect.top()) / rect.height(),
                );
                let t = (((uv.x - 0.5) * dir.x + (uv.y - 0.5) * dir.y) / span * 0.5 + 0.5)
                    .clamp(0.0, 1.0);
                mesh.colored_vertex(c, c32(from).lerp_to_gamma(c32(to), t));
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            p.add(mesh);
        }
    }
}

/// Grid of swatches. Clicking returns the fill; dragging carries it to
/// the timeline.
pub fn show(ui: &mut egui::Ui, tokens: Tokens) -> Option<Generated> {
    let mut chosen = None;
    ui.label(
        egui::RichText::new("Click to add at the playhead, or drag onto the timeline.")
            .small()
            .color(tokens.text_muted),
    );
    ui.add_space(4.0);
    let items = all();
    crate::layout::grid(ui, &items, 72.0, 8.0, |ui, (name, fill)| {
        let (rect, resp) = ui.allocate_exact_size(vec2(72.0, 58.0), Sense::click_and_drag());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, *name));
        let swatch = Rect::from_min_size(rect.min, vec2(72.0, 40.0));
        paint(ui.painter(), swatch, *fill);
        let stroke = if resp.hovered() {
            Stroke::new(2.0, tokens.accent)
        } else {
            Stroke::new(1.0, tokens.border)
        };
        ui.painter()
            .rect_stroke(swatch, CornerRadius::same(4), stroke, StrokeKind::Inside);
        ui.painter().text(
            pos2(rect.center().x, rect.bottom() - 8.0),
            egui::Align2::CENTER_CENTER,
            *name,
            egui::FontId::proportional(11.0),
            tokens.text,
        );
        if resp.dragged() {
            resp.dnd_set_drag_payload(*fill);
        }
        if resp.clicked() {
            chosen = Some(*fill);
        }
        resp.on_hover_text(*name);
    });
    chosen
}
