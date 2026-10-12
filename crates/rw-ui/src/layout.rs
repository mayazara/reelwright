//! Layout helpers.

use eframe::egui;

/// Lays `items` out in rows of fixed-width cells, as many per row as fit.
///
/// egui's wrapping layout can't know a framed widget's width before placing
/// it, so grids of cards are laid out by counting columns instead.
pub fn grid<T>(
    ui: &mut egui::Ui,
    items: &[T],
    cell_width: f32,
    spacing: f32,
    mut cell: impl FnMut(&mut egui::Ui, &T),
) {
    let available = ui.available_width();
    let columns = (((available + spacing) / (cell_width + spacing)).floor() as usize).max(1);
    ui.spacing_mut().item_spacing = egui::vec2(spacing, spacing);
    for row in items.chunks(columns) {
        ui.horizontal_top(|ui| {
            for item in row {
                cell(ui, item);
            }
        });
    }
}

/// Smallest width of a number box (drag fields and slider values), wide
/// enough for "−60.0 dB" or "100 %", so values don't resize their box.
pub const VALUE_BOX_WIDTH: f32 = 66.0;

/// Text whose digits all take the width of the widest digit, in a box at
/// least as wide as `widest` would be. Changing numbers (timecodes, counts)
/// then never move what's next to them.
pub fn steady_text(
    ui: &mut egui::Ui,
    text: &str,
    widest: &str,
    font: egui::FontId,
    color: egui::Color32,
) -> egui::Response {
    let (digit, widths): (f32, Vec<f32>) = ui.ctx().fonts_mut(|f| {
        let digit = ('0'..='9')
            .map(|c| f.glyph_width(&font, c))
            .fold(0.0, f32::max);
        let widths = text.chars().map(|c| f.glyph_width(&font, c)).collect();
        (digit, widths)
    });
    let advance = |c: char, w: f32| if c.is_ascii_digit() { digit } else { w };
    let measure = |s: &str| -> f32 {
        ui.ctx()
            .fonts_mut(|f| s.chars().map(|c| advance(c, f.glyph_width(&font, c))).sum())
    };
    let width = measure(text).max(measure(widest));
    let height = ui.ctx().fonts_mut(|f| f.row_height(&font));
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    let painter = ui.painter_at(rect.expand(1.0));
    let mut x = rect.left();
    for (c, w) in text.chars().zip(widths) {
        let cell = advance(c, w);
        painter.text(
            egui::pos2(x + cell / 2.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            c,
            font.clone(),
            color,
        );
        x += cell;
    }
    resp
}
