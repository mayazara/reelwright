//! M6 through the UI: stickers, shapes, visualisers and sounds from the
//! library.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rw_model::{Canvas, ItemKind, TrackKind};
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
        .create_project("Fun", Canvas::default());
    h.run_ok();
    h
}

fn project(h: &Harness<'_, App>) -> rw_model::Project {
    h.state().editor().session.as_ref().unwrap().project.clone()
}

#[test]
fn library_graphics_and_sounds_land_on_the_timeline() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    h.get_by_label("Graphics").click();
    h.run_ok();
    h.get_by_label("Grinning face").click();
    h.run_ok();
    h.get_by_label("Shapes").click();
    h.run_ok();
    h.get_by_label("Star").click();
    h.run_ok();
    h.get_by_label("Visualisers").click();
    h.run_ok();
    h.get_by_label("Bars").click();
    h.run_ok();
    let p = project(&h);
    let kinds: Vec<&str> = p
        .items()
        .map(|i| match i.kind {
            ItemKind::Sticker { .. } => "sticker",
            ItemKind::Shape { .. } => "shape",
            ItemKind::Visualiser { .. } => "visualiser",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds.len(), 3);
    for k in ["sticker", "shape", "visualiser"] {
        assert!(kinds.contains(&k), "{k} added: {kinds:?}");
    }
    // Same time, so each went on its own track above the main one.
    let picture_tracks = p
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Video && !t.main && !t.items.is_empty())
        .count();
    assert_eq!(picture_tracks, 3);

    // The newest (selected) graphic has its panel: change the bars.
    h.get_by_label("Wave").click();
    h.run_ok();
    let changed = project(&h).items().any(|i| {
        matches!(&i.kind, ItemKind::Visualiser { visualiser } if visualiser.style == rw_model::VisualiserStyle::Wave)
    });
    assert!(changed, "visualiser style changed");
    h.get_by_label(UNDO_LABEL).click();
    h.run_ok();

    // A sound: written out, imported and placed on a sound track.
    h.get_by_label("Sounds").click();
    h.run_ok();
    h.get_by_label("Pop").click();
    h.run_ok();
    let p = project(&h);
    assert_eq!(p.media.len(), 1);
    assert!(tmp.path().join("sounds/pop.wav").exists());
    let audio = p
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Audio)
        .expect("sound track");
    assert_eq!(audio.items.len(), 1);
}
