//! Home screen: create, open and pick recent projects.

use crate::theme::{HEADING_FAMILY, Tokens};
use eframe::egui::{self, Button, FontFamily, FontId, RichText};
use rw_model::app;

/// What the user asked for on the home screen this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeAction {
    NewProject,
    ToggleTheme,
}

/// Label of the primary button; also used by UI tests.
pub const NEW_PROJECT_LABEL: &str = "New project";
/// Message shown when the recent-projects list is empty.
pub const EMPTY_RECENTS_LABEL: &str = "No projects yet. Start one and it will show up here.";

pub fn show(ui: &mut egui::Ui, tokens: Tokens) -> Option<HomeAction> {
    let mut action = None;
    ui.add_space(32.0);
    ui.vertical_centered(|ui| {
        ui.set_max_width(720.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(app::NAME)
                    .font(FontId::new(28.0, FontFamily::Name(HEADING_FAMILY.into())))
                    .color(tokens.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Toggle light/dark").clicked() {
                    action = Some(HomeAction::ToggleTheme);
                }
            });
        });
        ui.add_space(24.0);
        ui.horizontal(|ui| {
            let new = Button::new(
                RichText::new(NEW_PROJECT_LABEL)
                    .color(tokens.on_accent)
                    .size(16.0),
            )
            .fill(tokens.accent)
            .min_size(egui::vec2(160.0, 40.0));
            if ui.add(new).clicked() {
                action = Some(HomeAction::NewProject);
            }
        });
        ui.add_space(32.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Recent projects")
                    .font(FontId::new(18.0, FontFamily::Name(HEADING_FAMILY.into()))),
            );
        });
        ui.separator();
        ui.add_space(16.0);
        ui.label(RichText::new(EMPTY_RECENTS_LABEL).color(tokens.text_muted));
    });
    action
}
