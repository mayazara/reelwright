//! The editor's docked panel layout: library left, preview centre,
//! inspector right, timeline along the bottom.

use crate::media_bin::{self, BinAction, BinState};
use crate::session::Session;
use crate::textures::Textures;
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

    /// Text shown while the panel has no content. The library has its own
    /// empty state in [`media_bin`].
    pub fn placeholder(self) -> &'static str {
        match self {
            Panel::Library => media_bin::EMPTY_LABEL,
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
    let [centre, _library] = tree.split_left(top, 0.28, vec![Panel::Library]);
    let [_preview, _inspector] = tree.split_right(centre, 0.70, vec![Panel::Inspector]);
    state
}

struct Viewer<'a> {
    tokens: Tokens,
    session: &'a Session,
    bin: &'a mut BinState,
    textures: &'a mut Textures,
    bin_action: Option<BinAction>,
}

impl TabViewer for Viewer<'_> {
    type Tab = Panel;

    fn title(&mut self, tab: &mut Panel) -> WidgetText {
        tab.title().into()
    }

    fn id(&mut self, tab: &mut Panel) -> egui::Id {
        egui::Id::new(("dock-panel", *tab))
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Panel) {
        match tab {
            Panel::Library => {
                if let Some(a) =
                    media_bin::show(ui, self.session, self.bin, self.textures, self.tokens)
                {
                    self.bin_action = Some(a);
                }
            }
            Panel::Preview => {
                let rect = ui.available_rect_before_wrap();
                ui.painter()
                    .rect_filled(rect, 0.0, self.tokens.preview_backdrop);
                placeholder(ui, *tab, self.tokens);
            }
            _ => placeholder(ui, *tab, self.tokens),
        }
    }

    fn is_closeable(&self, _tab: &Panel) -> bool {
        false
    }
}

fn placeholder(ui: &mut egui::Ui, tab: Panel, tokens: Tokens) {
    ui.centered_and_justified(|ui| {
        ui.label(RichText::new(tab.placeholder()).color(tokens.text_muted));
    });
}

/// Shows the docked editor layout filling `ui`.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut DockState<Panel>,
    session: &Session,
    bin: &mut BinState,
    textures: &mut Textures,
    tokens: Tokens,
) -> Option<BinAction> {
    let style = egui_dock::Style::from_egui(ui.style().as_ref());
    let mut viewer = Viewer {
        tokens,
        session,
        bin,
        textures,
        bin_action: None,
    };
    DockArea::new(state)
        .style(style)
        .show_close_buttons(false)
        .show_leaf_close_all_buttons(false)
        .show_leaf_collapse_buttons(false)
        .show_inside(ui, &mut viewer);
    viewer.bin_action
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
