//! Small vector icons drawn with the painter. Original shapes; no icon font.

use eframe::egui::{
    self, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2,
};

fn s(color: Color32) -> Stroke {
    Stroke::new(1.5, color)
}

/// Padlock, open or closed.
pub fn lock(p: &Painter, r: Rect, closed: bool, color: Color32) {
    let c = r.center();
    let body = Rect::from_center_size(c + vec2(0.0, 2.5), vec2(r.width() * 0.6, r.height() * 0.42));
    p.rect(
        body,
        CornerRadius::same(2),
        if closed { color } else { Color32::TRANSPARENT },
        s(color),
        StrokeKind::Inside,
    );
    let w = body.width() * 0.32;
    let top = body.top() - r.height() * 0.24;
    let right = if closed { c.x + w } else { c.x + w + 3.0 };
    let pts = vec![
        pos2(c.x - w, body.top()),
        pos2(c.x - w, top + 2.0),
        pos2(c.x, top - 1.0),
        pos2(right, top + 2.0),
        pos2(right, if closed { body.top() } else { top + 5.0 }),
    ];
    p.add(Shape::line(pts, s(color)));
}

/// Eye, open or crossed out.
pub fn eye(p: &Painter, r: Rect, open: bool, color: Color32) {
    let c = r.center();
    let w = r.width() * 0.42;
    let h = r.height() * 0.22;
    let top: Vec<Pos2> = (0..=12)
        .map(|i| {
            let t = i as f32 / 12.0;
            pos2(
                c.x - w + 2.0 * w * t,
                c.y - h * (std::f32::consts::PI * t).sin(),
            )
        })
        .collect();
    let bottom: Vec<Pos2> = top.iter().map(|q| pos2(q.x, 2.0 * c.y - q.y)).collect();
    p.add(Shape::line(top, s(color)));
    p.add(Shape::line(bottom, s(color)));
    p.circle_filled(c, h * 0.55, color);
    if !open {
        p.line_segment(
            [pos2(c.x - w, c.y + h * 1.6), pos2(c.x + w, c.y - h * 1.6)],
            Stroke::new(2.0, color),
        );
    }
}

/// Speaker with waves, or with a cross when muted.
pub fn speaker(p: &Painter, r: Rect, muted: bool, color: Color32) {
    let c = r.center();
    let u = r.height() / 16.0;
    let body = vec![
        pos2(c.x - 6.0 * u, c.y - 2.5 * u),
        pos2(c.x - 3.0 * u, c.y - 2.5 * u),
        pos2(c.x + 1.0 * u, c.y - 6.0 * u),
        pos2(c.x + 1.0 * u, c.y + 6.0 * u),
        pos2(c.x - 3.0 * u, c.y + 2.5 * u),
        pos2(c.x - 6.0 * u, c.y + 2.5 * u),
    ];
    p.add(Shape::convex_polygon(body, color, Stroke::NONE));
    if muted {
        let o = pos2(c.x + 5.0 * u, c.y);
        p.line_segment(
            [o + vec2(-2.0 * u, -2.0 * u), o + vec2(2.0 * u, 2.0 * u)],
            s(color),
        );
        p.line_segment(
            [o + vec2(-2.0 * u, 2.0 * u), o + vec2(2.0 * u, -2.0 * u)],
            s(color),
        );
    } else {
        for rad in [3.5f32, 6.0] {
            let pts: Vec<Pos2> = (0..=8)
                .map(|k| {
                    let a = -0.8 + 1.6 * k as f32 / 8.0;
                    pos2(c.x + 1.5 * u + rad * u * a.cos(), c.y + rad * u * a.sin())
                })
                .collect();
            p.add(Shape::line(pts, s(color)));
        }
    }
}

/// Horseshoe magnet, for snapping.
pub fn magnet(p: &Painter, r: Rect, on: bool, color: Color32) {
    let c = r.center();
    let rad = r.width() * 0.28;
    let pts: Vec<Pos2> = (0..=12)
        .map(|k| {
            let a = std::f32::consts::PI * k as f32 / 12.0;
            pos2(c.x - rad * a.cos(), c.y + rad * a.sin())
        })
        .collect();
    let mut arm = vec![pos2(c.x - rad, c.y - rad)];
    arm.extend(pts);
    arm.push(pos2(c.x + rad, c.y - rad));
    p.add(Shape::line(arm, Stroke::new(3.0, color)));
    if on {
        for x in [c.x - rad, c.x + rad] {
            p.line_segment(
                [
                    pos2(x - 1.5, c.y - rad - 1.0),
                    pos2(x + 1.5, c.y - rad - 1.0),
                ],
                Stroke::new(3.0, Color32::WHITE),
            );
        }
    }
}

pub fn play(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let u = r.height() * 0.28;
    p.add(Shape::convex_polygon(
        vec![
            pos2(c.x - u * 0.8, c.y - u),
            pos2(c.x + u, c.y),
            pos2(c.x - u * 0.8, c.y + u),
        ],
        color,
        Stroke::NONE,
    ));
}

pub fn pause(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let u = r.height() * 0.28;
    for dx in [-u * 0.55, u * 0.55] {
        p.rect_filled(
            Rect::from_center_size(pos2(c.x + dx, c.y), vec2(u * 0.55, u * 2.0)),
            CornerRadius::same(1),
            color,
        );
    }
}

/// A square button that draws an icon; returns its response. `label` is
/// the accessible name and tooltip.
/// Four corner brackets: pointing out to go full screen, in to leave it.
pub fn full_screen(p: &Painter, r: Rect, leave: bool, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let arm = r.width() * 0.32;
    for (cx, cy) in [
        (r.left(), r.top()),
        (r.right(), r.top()),
        (r.left(), r.bottom()),
        (r.right(), r.bottom()),
    ] {
        // Direction from the corner towards the middle.
        let dx = if cx < r.center().x { 1.0 } else { -1.0 };
        let dy = if cy < r.center().y { 1.0 } else { -1.0 };
        // Leaving: the bracket's corner sits inward, arms pointing out.
        let (x, y, sx, sy) = if leave {
            (cx + dx * arm, cy + dy * arm, -dx, -dy)
        } else {
            (cx, cy, dx, dy)
        };
        p.line_segment([pos2(x, y), pos2(x + sx * arm, y)], stroke);
        p.line_segment([pos2(x, y), pos2(x, y + sy * arm)], stroke);
    }
}

pub fn button(
    ui: &mut egui::Ui,
    label: &str,
    active: bool,
    draw: impl FnOnce(&Painter, Rect, Color32),
) -> egui::Response {
    let size = vec2(22.0, 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, label));
    let visuals = ui.style().interact_selectable(&resp, active);
    if resp.hovered() || active {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), visuals.weak_bg_fill);
    }
    draw(ui.painter(), rect.shrink(3.0), visuals.fg_stroke.color);
    resp.on_hover_text(label)
}
