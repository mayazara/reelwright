//! M5 text through the UI: presets, editing, dragging to the timeline.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rw_model::{Canvas, TextBehaviour, TrackKind};
use rw_ui::{App, Config, UNDO_LABEL};

fn harness(root: &std::path::Path) -> Harness<'static, App> {
    let config = Config {
        projects_dir: root.join("Projects"),
        settings_file: root.join("settings.json"),
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 950.0))
        .build_eframe(move |cc| App::new(cc, config));
    h.run_ok();
    h.state_mut()
        .editor_mut()
        .create_project("Words", Canvas::default());
    h.run_ok();
    h
}

fn project(h: &Harness<'_, App>) -> rw_model::Project {
    h.state().editor().session.as_ref().unwrap().project.clone()
}

/// The leftmost control called `label`: the library's, where the
/// inspector (further right) has one of the same name.
fn in_library<'h>(h: &'h Harness<'_, App>, label: &'h str) -> egui_kittest::Node<'h> {
    h.get_all_by_label(label)
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .unwrap()
}

fn text_items(h: &Harness<'_, App>) -> Vec<rw_model::Item> {
    project(h)
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Text)
        .flat_map(|t| t.items.clone())
        .collect()
}

#[test]
fn presets_add_text_that_can_be_edited_and_undone() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    h.get_by_label("Text").click();
    h.run_ok();
    h.get_by_label("Lower third").click();
    h.run_ok();
    let items = text_items(&h);
    assert_eq!(items.len(), 1, "added on a text track");
    assert!(items[0].text().unwrap().style.background.is_some());

    // The Text tab shows for the new (selected) item; retype the text.
    h.get_by_role(egui::accesskit::Role::MultilineTextInput)
        .click();
    h.run_ok();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput)
        .type_text(" and more");
    h.run_ok();
    // Typing previews without committing; leaving the field commits.
    assert_eq!(
        text_items(&h)[0].text().unwrap().text,
        "Name Surname\nRole or place",
        "not committed while typing"
    );
    h.key_press(egui::Key::Escape);
    h.run_ok();
    in_library(&h, "Text").click();
    h.run_ok();
    let edited = text_items(&h)[0].text().unwrap().text.clone();
    assert!(edited.ends_with(" and more"), "committed: {edited:?}");

    h.get_by_label("Rolling credits").click();
    h.run_ok();
    assert_eq!(
        text_items(&h)[0].text().unwrap().behaviour,
        TextBehaviour::Roll
    );
    h.get_by_label(UNDO_LABEL).click();
    h.run_ok();
    assert_eq!(
        text_items(&h)[0].text().unwrap().behaviour,
        TextBehaviour::Static
    );

    // A second preset at the same time stacks on a new text track.
    in_library(&h, "Text").click();
    h.run_ok();
    in_library(&h, "Title").click();
    h.run_ok();
    let text_tracks = project(&h)
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Text)
        .count();
    assert_eq!(text_tracks, 2);
}
