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

#[test]
fn enter_selects_clips_at_the_playhead_and_escape_clears() {
    use rw_model::{Item, ShapeKind, ShapeSpec, Time};
    use rw_ui::actions::EditorAction;
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    for kind in [ShapeKind::Star, ShapeKind::Ellipse] {
        let mut shape = Item::shape_item(ShapeSpec::new(kind), Time::ZERO);
        shape.duration = Time::from_secs(2);
        h.state_mut()
            .editor_mut()
            .apply(EditorAction::AddGraphic(shape));
        h.run_ok();
    }
    let ed = h.state_mut().editor_mut();
    ed.session.as_mut().unwrap().selection.clear();
    ed.session.as_mut().unwrap().seek(Time::from_secs(1));
    h.run_ok();
    let selected = |h: &Harness<'_, App>| {
        let s = h.state().editor().session.as_ref().unwrap();
        s.selection.iter().copied().collect::<Vec<_>>()
    };
    let shape_of = |h: &Harness<'_, App>, id| {
        let p = &h.state().editor().session.as_ref().unwrap().project;
        p.find_item(id).unwrap().1.clone()
    };
    h.key_press(egui::Key::Enter);
    h.run_ok();
    let first = selected(&h);
    assert_eq!(first.len(), 1);
    // The top one first (added last), then the next track down, then round.
    assert!(format!("{:?}", shape_of(&h, first[0]).kind).contains("Ellipse"));
    h.key_press(egui::Key::Enter);
    h.run_ok();
    let second = selected(&h);
    assert!(format!("{:?}", shape_of(&h, second[0]).kind).contains("Star"));
    h.key_press(egui::Key::Enter);
    h.run_ok();
    assert_eq!(selected(&h), first);
    // Keyboard editing works on it: delete with the keyboard.
    h.key_press(egui::Key::Delete);
    h.run_ok();
    assert_eq!(
        h.state()
            .editor()
            .session
            .as_ref()
            .unwrap()
            .project
            .items()
            .count(),
        1
    );
    h.key_press(egui::Key::Enter);
    h.run_ok();
    assert_eq!(selected(&h).len(), 1);
    h.key_press(egui::Key::Escape);
    h.run_ok();
    assert!(selected(&h).is_empty());
}
