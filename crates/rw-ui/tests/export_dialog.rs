//! M5: the export dialog, from choosing a format to the finished file.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rw_model::{Canvas, Generated, Rgba};
use rw_ui::actions::EditorAction;
use rw_ui::export_dialog::{EXPORT_LABEL, REVEAL_LABEL, Stage};
use rw_ui::{App, Config};
use std::time::{Duration, Instant};

fn harness(root: &std::path::Path) -> Harness<'static, App> {
    let config = Config {
        projects_dir: root.join("Projects"),
        settings_file: root.join("settings.json"),
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_eframe(move |cc| App::new(cc, config));
    h.run_ok();
    h.state_mut()
        .editor_mut()
        .create_project("Clip", Canvas::custom(320, 180));
    h.run_ok();
    h.state_mut()
        .editor_mut()
        .apply(EditorAction::AppendGenerated(Generated::Solid {
            color: Rgba([200, 60, 40, 255]),
        }));
    h.run_ok();
    h
}

fn stage(h: &Harness<'_, App>) -> Option<Stage> {
    h.state().editor().export_dialog().map(|d| d.stage.clone())
}

fn wait_done(h: &mut Harness<'_, App>) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while matches!(stage(h), Some(Stage::Running)) {
        assert!(Instant::now() < deadline, "export timed out");
        h.step();
        std::thread::sleep(Duration::from_millis(30));
    }
    h.run_ok();
}

/// The dialog's own Export button (the top bar has one too, further up).
fn dialog_export<'h>(h: &'h Harness<'_, App>) -> egui_kittest::Node<'h> {
    h.get_all_by_label(EXPORT_LABEL)
        .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .unwrap()
}

#[test]
fn gif_export_then_replace_prompt() {
    if rw_render::Gpu::headless().is_err() {
        eprintln!("skipping: no GPU");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    h.get_by_label(EXPORT_LABEL).click();
    h.run_ok();
    assert_eq!(stage(&h), Some(Stage::Setup), "dialog open");
    h.get_by_label("GIF").click();
    h.run_ok();
    dialog_export(&h).click();
    h.run_ok();
    wait_done(&mut h);
    let Some(Stage::Done(report)) = stage(&h) else {
        panic!("not finished: {:?}", stage(&h));
    };
    assert_eq!(report.path, tmp.path().join("Clip.gif"));
    assert!(report.path.exists() && report.bytes > 0);
    let _ = h.get_by_label(REVEAL_LABEL);
    h.get_by_label("Done").click();
    h.run_ok();
    assert!(stage(&h).is_none(), "closed");

    // Same name again: asked before replacing.
    h.get_by_label(EXPORT_LABEL).click();
    h.run_ok();
    h.get_by_label("GIF").click();
    h.run_ok();
    dialog_export(&h).click();
    h.run_ok();
    assert!(matches!(stage(&h), Some(Stage::Overwrite(_))));
    h.get_by_label("Replace").click();
    h.run_ok();
    wait_done(&mut h);
    assert!(matches!(stage(&h), Some(Stage::Done(_))));
}
