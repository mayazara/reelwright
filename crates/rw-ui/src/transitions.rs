//! The Transitions tab: every transition as a small diagram of itself
//! half-way through.

use crate::theme::Tokens;
use eframe::egui::{self, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use rw_model::TransitionKind;

/// Outgoing and incoming clip colours in the diagrams.
const A: Color32 = Color32::from_rgb(14, 124, 116);
const B: Color32 = Color32::from_rgb(242, 165, 65);

/// Paints `kind` half-way through into `r`.
fn diagram(p: &egui::Painter, r: Rect, kind: TransitionKind) {
    use TransitionKind as K;
    let mix = A.lerp_to_gamma(B, 0.5);
    let half = |x0: f32, x1: f32, y0: f32, y1: f32| {
        Rect::from_min_max(
            pos2(r.left() + r.width() * x0, r.top() + r.height() * y0),
            pos2(r.left() + r.width() * x1, r.top() + r.height() * y1),
        )
    };
    p.rect_filled(r, CornerRadius::ZERO, A);
    match kind {
        K::CrossDissolve | K::BlurDissolve => {
            p.rect_filled(r, CornerRadius::ZERO, mix);
        }
        K::FadeBlack => {
            p.rect_filled(r, CornerRadius::ZERO, Color32::from_rgb(20, 20, 22));
        }
        K::FadeWhite => {
            p.rect_filled(r, CornerRadius::ZERO, Color32::from_rgb(245, 245, 240));
        }
        K::WipeLeft | K::SlideLeft => {
            p.rect_filled(half(0.5, 1.0, 0.0, 1.0), CornerRadius::ZERO, B);
        }
        K::WipeRight | K::SlideRight => {
            p.rect_filled(half(0.0, 0.5, 0.0, 1.0), CornerRadius::ZERO, B);
        }
        K::WipeUp | K::SlideUp => {
            p.rect_filled(half(0.0, 1.0, 0.5, 1.0), CornerRadius::ZERO, B);
        }
        K::WipeDown | K::SlideDown => {
            p.rect_filled(half(0.0, 1.0, 0.0, 0.5), CornerRadius::ZERO, B);
        }
        K::Push => {
            p.rect_filled(half(0.0, 0.5, 0.0, 1.0), CornerRadius::ZERO, A);
            p.rect_filled(half(0.5, 1.0, 0.0, 1.0), CornerRadius::ZERO, B);
        }
        K::ZoomIn => {
            p.rect_filled(r, CornerRadius::ZERO, mix);
            p.rect_stroke(
                r.shrink(4.0),
                CornerRadius::ZERO,
                Stroke::new(1.5, B),
                StrokeKind::Inside,
            );
        }
        K::ZoomOut => {
            p.rect_filled(r, CornerRadius::ZERO, B);
            p.rect_filled(r.shrink2(r.size() * 0.25), CornerRadius::ZERO, A);
        }
        K::Spin => {
            p.rect_filled(r, CornerRadius::ZERO, Color32::from_rgb(20, 20, 22));
            let c = r.center();
            let s = r.height() * 0.3;
            let pts = [(-1.0f32, -0.6f32), (1.0, -0.6), (1.0, 0.6), (-1.0, 0.6)].map(|(x, y)| {
                let a = 0.5f32;
                pos2(
                    c.x + s * (x * a.cos() - y * a.sin()),
                    c.y + s * (x * a.sin() + y * a.cos()),
                )
            });
            p.add(egui::Shape::convex_polygon(pts.to_vec(), B, Stroke::NONE));
        }
        K::CircleReveal => {
            p.circle_filled(r.center(), r.height() * 0.32, B);
        }
        K::Glitch => {
            for i in 0..6 {
                let y0 = i as f32 / 6.0;
                let shift = [0.0, 0.12, -0.08, 0.05, -0.15, 0.1][i];
                let band =
                    half(0.0, 1.0, y0, y0 + 1.0 / 6.0).translate(vec2(shift * r.width(), 0.0));
                p.rect_filled(
                    band.intersect(r),
                    CornerRadius::ZERO,
                    if i % 2 == 0 { B } else { A },
                );
            }
        }
        K::LightLeak => {
            p.rect_filled(r, CornerRadius::ZERO, mix);
            p.circle_filled(
                pos2(r.center().x, r.top() + r.height() * 0.35),
                r.height() * 0.3,
                Color32::from_rgba_unmultiplied(255, 200, 140, 170),
            );
        }
        K::PageTurn => {
            p.rect_filled(r, CornerRadius::ZERO, A);
            p.add(egui::Shape::convex_polygon(
                vec![
                    r.right_bottom(),
                    pos2(r.right(), r.top() + r.height() * 0.2),
                    pos2(r.left() + r.width() * 0.35, r.bottom()),
                ],
                B,
                Stroke::NONE,
            ));
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(r.right(), r.top() + r.height() * 0.2),
                    pos2(r.left() + r.width() * 0.35, r.bottom()),
                    pos2(r.left() + r.width() * 0.45, r.top() + r.height() * 0.3),
                ],
                Color32::from_rgb(200, 225, 222),
                Stroke::NONE,
            ));
        }
    }
}

/// Grid of transitions. Clicking returns one (added next to the selected
/// clip); dragging carries it to a cut on the timeline.
pub fn show(ui: &mut egui::Ui, tokens: Tokens) -> Option<TransitionKind> {
    let mut chosen = None;
    ui.label(
        egui::RichText::new(
            "Drag onto the cut between two clips, or click to add next to the selected clip.",
        )
        .small()
        .color(tokens.text_muted),
    );
    ui.add_space(4.0);
    crate::layout::grid(ui, &TransitionKind::ALL, 100.0, 8.0, |ui, kind| {
        let name = kind.label();
        let (rect, resp) = ui.allocate_exact_size(vec2(100.0, 66.0), Sense::click_and_drag());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
        let tile = Rect::from_min_size(rect.min, vec2(100.0, 48.0));
        diagram(&ui.painter_at(tile), tile, *kind);
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
            name,
            egui::FontId::proportional(10.5),
            tokens.text,
        );
        if resp.dragged() {
            resp.dnd_set_drag_payload(*kind);
        }
        if resp.clicked() {
            chosen = Some(*kind);
        }
        resp.on_hover_text(name);
    });
    chosen
}
