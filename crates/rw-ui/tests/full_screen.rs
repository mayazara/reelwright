//! M10: full-screen preview and the shortcut list.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rw_model::Canvas;
use rw_ui::workspace::{FULL_SCREEN_LABEL, LEAVE_FULL_SCREEN_LABEL};
use rw_ui::{App, Config, cheat_sheet};

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
        .create_project("Full", Canvas::default());
    h.run_ok();
    h
}

#[test]
fn f_and_escape_enter_and_leave_full_screen() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    h.key_press(egui::Key::F);
    h.run_ok();
    assert!(h.state().editor().is_full_screen());
    // Only the preview: no top bar, panels or editing handles.
    assert!(h.query_by_label("Undo").is_none());
    h.get_by_label(LEAVE_FULL_SCREEN_LABEL);
    h.key_press(egui::Key::Escape);
    h.run_ok();
    assert!(!h.state().editor().is_full_screen());
    h.get_by_label("Undo");

    // The transport button does the same.
    h.get_by_label(FULL_SCREEN_LABEL).click();
    h.run_ok();
    assert!(h.state().editor().is_full_screen());
    h.get_by_label(LEAVE_FULL_SCREEN_LABEL).click();
    h.run_ok();
    assert!(!h.state().editor().is_full_screen());
}

#[test]
fn going_home_leaves_full_screen() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    h.key_press(egui::Key::F);
    h.run_ok();
    h.state_mut().editor_mut().close_project();
    h.run_ok();
    assert!(!h.state().editor().is_full_screen());
}

#[test]
fn shortcut_list_opens_from_the_top_bar_and_question_mark() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    h.get_by_label(cheat_sheet::OPEN_LABEL).click();
    h.run_ok();
    h.get_by_label(cheat_sheet::TITLE);
    h.get_by_label("Split at the playhead");
    h.get_by_label("Full-screen preview");
    h.get_by_label(cheat_sheet::OPEN_LABEL).click();
    h.run_ok();
    assert!(h.query_by_label("Split at the playhead").is_none());
    h.key_press(egui::Key::Questionmark);
    h.run_ok();
    h.get_by_label(cheat_sheet::TITLE);
}
