//! M3 editing through the UI: backgrounds, toolbar, filters, live preview.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rw_edit::timeline::LookPatch;
use rw_model::{Canvas, Fit, Generated, Time};
use rw_ui::actions::EditorAction;
use rw_ui::{App, Config, UNDO_LABEL};
use std::path::Path;

fn harness(root: &Path) -> Harness<'static, App> {
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
        .create_project("Look", Canvas::default());
    h.run_ok();
    h
}

fn first_item(h: &Harness<'_, App>) -> rw_model::Item {
    h.state()
        .editor()
        .session
        .as_ref()
        .unwrap()
        .project
        .items()
        .next()
        .unwrap()
        .clone()
}

#[test]
fn background_from_library_and_toolbar_flip() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    h.get_by_label("Backgrounds").click();
    h.run_ok();
    h.get_by_label("Teal").click();
    h.run_ok();
    let item = first_item(&h);
    assert!(matches!(
        item.generated_fill(),
        Some(Generated::Solid { .. })
    ));
    assert!(
        h.state()
            .editor()
            .session
            .as_ref()
            .unwrap()
            .selection
            .contains(&item.id),
        "new clip selected"
    );

    // The floating toolbar appears over the preview for the selected clip.
    h.get_by_label("Flip ↔").click();
    h.run_ok();
    assert!(first_item(&h).look.transform.flip_h);
    h.get_by_label(UNDO_LABEL).click();
    h.run_ok();
    assert!(!first_item(&h).look.transform.flip_h);

    h.get_by_label("Fill").click();
    h.run_ok();
    assert_eq!(first_item(&h).look.transform.fit, Fit::Cover);
}

#[test]
fn filter_swatch_sets_filter() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    h.state_mut()
        .editor_mut()
        .apply(EditorAction::AppendGenerated(Generated::Solid {
            color: rw_model::Rgba([200, 80, 40, 255]),
        }));
    h.run_ok();
    h.get_by_label("Filters").click();
    h.run_ok();
    h.get_by_label("Mono").click();
    h.run_ok();
    let f = first_item(&h).look.filter.expect("filter set");
    assert_eq!((f.id.as_str(), f.intensity), ("mono", 1.0));
    h.get_by_label("None").click();
    h.run_ok();
    assert!(first_item(&h).look.filter.is_none());
}

#[test]
fn live_preview_doesnt_touch_history_until_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    h.state_mut()
        .editor_mut()
        .apply(EditorAction::AppendGenerated(Generated::Solid {
            color: rw_model::Rgba([1, 2, 3, 255]),
        }));
    h.run_ok();
    let id = first_item(&h).id;
    let s = h.state_mut().editor_mut().session.as_mut().unwrap();
    let undo_before = s.history.undo_label();
    s.set_preview_patch(
        vec![id],
        LookPatch {
            opacity: Some(0.25),
            fade_in: Some(Time::from_secs(1)),
            ..LookPatch::default()
        },
    );
    assert_eq!(
        s.preview_project().find_item(id).unwrap().1.look.opacity,
        0.25
    );
    assert_eq!(
        s.project.find_item(id).unwrap().1.look.opacity,
        1.0,
        "project untouched"
    );
    assert_eq!(s.history.undo_label(), undo_before);
    // Committing applies it once and clears the preview.
    s.apply(EditorAction::Op(rw_edit::timeline::TimelineOp::Look {
        items: vec![id],
        patch: LookPatch {
            opacity: Some(0.25),
            ..LookPatch::default()
        },
    }));
    assert!(s.preview_patch.is_none());
    assert_eq!(s.project.find_item(id).unwrap().1.look.opacity, 0.25);
    assert_eq!(s.history.undo_label().as_deref(), Some("Blending"));
}
