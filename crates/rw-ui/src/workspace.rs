//! The editor's docked panel layout: library left, preview centre,
//! inspector right, timeline along the bottom.

use crate::theme::Tokens;
use eframe::egui::{self, RichText, WidgetText};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};

/// A dockable editor panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    Library,
    Preview,
    Inspector,
    Timeline,
}

impl Panel {
    pub const ALL: [Panel; 4] = [
        Panel::Library,
        Panel::Preview,
        Panel::Inspector,
        Panel::Timeline,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Panel::Library => "Library",
            Panel::Preview => "Preview",
            Panel::Inspector => "Inspector",
            Panel::Timeline => "Timeline",
        }
    }

    /// Text shown while the panel has no content.
    pub fn placeholder(self) -> &'static str {
        match self {
            Panel::Library => "Imported media, text styles and graphics will appear here.",
            Panel::Preview => "Add something to the timeline to see it here.",
            Panel::Inspector => "Select an item to edit its properties.",
            Panel::Timeline => "Drag media here to start editing.",
        }
    }
}

/// Builds the default layout.
pub fn default_layout() -> DockState<Panel> {
    let mut state = DockState::new(vec![Panel::Preview]);
    let tree = state.main_surface_mut();
    let [top, _timeline] = tree.split_below(NodeIndex::root(), 0.62, vec![Panel::Timeline]);
    let [centre, _library] = tree.split_left(top, 0.24, vec![Panel::Library]);
    let [_preview, _inspector] = tree.split_right(centre, 0.70, vec![Panel::Inspector]);
    state
}

struct Viewer {
    tokens: Tokens,
}

impl TabViewer for Viewer {
    type Tab = Panel;

    fn title(&mut self, tab: &mut Panel) -> WidgetText {
        tab.title().into()
    }

    fn id(&mut self, tab: &mut Panel) -> egui::Id {
        egui::Id::new(("dock-panel", *tab))
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Panel) {
        if *tab == Panel::Preview {
            let rect = ui.available_rect_before_wrap();
            ui.painter()
                .rect_filled(rect, 0.0, self.tokens.preview_backdrop);
        }
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new(tab.placeholder()).color(self.tokens.text_muted));
        });
    }

    fn is_closeable(&self, _tab: &Panel) -> bool {
        false
    }
}

/// Shows the docked editor layout filling `ui`.
pub fn show(ui: &mut egui::Ui, state: &mut DockState<Panel>, tokens: Tokens) {
    let style = egui_dock::Style::from_egui(ui.style().as_ref());
    DockArea::new(state)
        .style(style)
        .show_close_buttons(false)
        .show_leaf_close_all_buttons(false)
        .show_leaf_collapse_buttons(false)
        .show_inside(ui, &mut Viewer { tokens });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_has_each_panel_once() {
        let state = default_layout();
        for panel in Panel::ALL {
            let count = state.iter_all_tabs().filter(|(_, t)| **t == panel).count();
            assert_eq!(count, 1, "{panel:?}");
        }
    }
}
