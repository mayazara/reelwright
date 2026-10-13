//! The keyboard shortcut list, built from the shortcuts the editor
//! actually handles, with key names as this computer's keyboard shows them.

use crate::theme::Tokens;
use eframe::egui::{self, KeyboardShortcut, RichText};

pub const TITLE: &str = "Keyboard shortcuts";
pub const OPEN_LABEL: &str = "Shortcuts";

/// Rows of the sheet: what it does, and the keys (alternatives joined),
/// in the editor's order with repeated actions merged.
pub fn rows(ctx: &egui::Context) -> Vec<(&'static str, String)> {
    let mut rows: Vec<(&'static str, Vec<KeyboardShortcut>)> = Vec::new();
    let mut add = |what: &'static str, keys: KeyboardShortcut| match rows
        .iter_mut()
        .find(|(w, _)| *w == what)
    {
        Some((_, k)) => k.push(keys),
        None => rows.push((what, vec![keys])),
    };
    for (keys, _, what) in crate::shortcut_table() {
        add(what, keys);
    }
    for (keys, what) in crate::other_shortcuts() {
        for k in keys {
            add(what, k);
        }
    }
    rows.into_iter()
        .map(|(what, keys)| {
            let text: Vec<String> = keys.iter().map(|k| ctx.format_shortcut(k)).collect();
            (what, text.join("  or  "))
        })
        .collect()
}

/// Shows the sheet while `open`.
pub fn show(ctx: &egui::Context, open: &mut bool, tokens: Tokens) {
    let rows = rows(ctx);
    egui::Window::new(TITLE)
        .open(open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Grid::new("cheat-sheet")
                .num_columns(2)
                .spacing([24.0, 6.0])
                .striped(true)
                .show(ui, |ui| {
                    for (what, keys) in rows {
                        ui.label(what);
                        ui.label(RichText::new(keys).monospace().color(tokens.text));
                        ui.end_row();
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_plan_shortcut_is_listed() {
        let ctx = egui::Context::default();
        let rows = rows(&ctx);
        let what: Vec<&str> = rows.iter().map(|r| r.0).collect();
        for needed in [
            "Play / pause",
            "Split at the playhead",
            "Delete",
            "Delete and close the gap",
            "Undo",
            "Redo",
            "Copy",
            "Paste",
            "Duplicate",
            "Group",
            "Ungroup",
            "Back one frame",
            "Forward one frame",
            "Previous cut",
            "Next cut",
            "Zoom the timeline in",
            "Zoom the timeline out",
            "Fit the timeline",
            "Full-screen preview",
            "Export",
        ] {
            assert!(what.contains(&needed), "{needed} missing");
        }
        // Alternatives are merged into one row.
        let delete = rows.iter().find(|r| r.0 == "Delete").unwrap();
        assert!(delete.1.contains("  or  "), "{}", delete.1);
        assert_eq!(what.iter().filter(|w| **w == "Delete").count(), 1);
    }
}
