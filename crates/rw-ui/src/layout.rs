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
