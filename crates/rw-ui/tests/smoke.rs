//! UI smoke tests driven through AccessKit, without a window or GPU.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rw_ui::{App, Screen, home, workspace::Panel};

fn harness() -> Harness<'static, App> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| App::new(cc));
    h.run();
    h
}

#[test]
fn opens_to_empty_home_screen() {
    let h = harness();
    assert_eq!(h.state().editor().screen, Screen::Home);
    h.get_by_label(home::NEW_PROJECT_LABEL);
    h.get_by_label(home::EMPTY_RECENTS_LABEL);
}

#[test]
fn new_project_opens_docked_editor() {
    let mut h = harness();
    h.get_by_label(home::NEW_PROJECT_LABEL).click();
    h.run();
    assert_eq!(h.state().editor().screen, Screen::Editor);
    for panel in Panel::ALL {
        h.get_by_label(panel.placeholder());
    }
}

#[test]
fn theme_toggle_flips_mode() {
    let mut h = harness();
    let before = h.state().editor().theme;
    h.get_by_label("Toggle light/dark").click();
    h.run();
    assert_eq!(h.state().editor().theme, before.toggled());
}
