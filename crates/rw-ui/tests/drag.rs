//! Dragging from the library onto the timeline with the pointer.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rw_model::Canvas;
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
        .create_project("Drag", Canvas::default());
    h.run_ok();
    h
}

fn send(h: &mut Harness<'_, App>, e: egui::Event) {
    h.event(e);
    h.step();
}

/// Presses on `from`, moves in steps to `to` and lets go.
fn drag(h: &mut Harness<'_, App>, from: egui::Pos2, to: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    send(h, egui::Event::PointerMoved(from));
    send(h, button(from, true));
    for i in 1..=20 {
        send(
            h,
            egui::Event::PointerMoved(from + (to - from) * (i as f32 / 20.0)),
        );
    }
    send(h, button(to, false));
    for _ in 0..3 {
        h.step();
    }
}

fn centre(h: &Harness<'_, App>, label: &str) -> egui::Pos2 {
    let c = h.get_by_label(label).rect().center();
    egui::pos2(c.x as f32, c.y as f32)
}

fn item_count(h: &Harness<'_, App>) -> usize {
    h.state()
        .editor()
        .session
        .as_ref()
        .unwrap()
        .project
        .items()
        .count()
}

/// Somewhere over the timeline's tracks.
const ON_TIMELINE: egui::Pos2 = egui::pos2(500.0, 700.0);

#[test]
fn media_and_backgrounds_drop_onto_the_timeline() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    let fx = rw_media::fixtures::ensure();
    h.state_mut()
        .editor_mut()
        .import_paths(vec![fx.path("still.png")]);
    let deadline = Instant::now() + Duration::from_secs(30);
    while {
        let ed = h.state().editor();
        ed.session.as_ref().unwrap().importing() || ed.jobs().pending() > 0
    } {
        assert!(Instant::now() < deadline, "import timed out");
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    h.run_ok();

    let from = centre(&h, "still.png");
    drag(&mut h, from, ON_TIMELINE);
    assert_eq!(item_count(&h), 1, "media dropped on the timeline");

    h.get_by_label("Backgrounds").click();
    h.run_ok();
    let from = centre(&h, "Teal");
    drag(&mut h, from, ON_TIMELINE);
    assert_eq!(item_count(&h), 2, "background dropped on the timeline");
}
