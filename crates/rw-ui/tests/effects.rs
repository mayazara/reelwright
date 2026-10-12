//! M4 through the UI: effects, transitions, speed, reverse, freeze frames.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rw_model::{Canvas, EffectKind, Generated, Rgba, Time, TransitionKind};
use rw_ui::actions::EditorAction;
use rw_ui::{App, Config, UNDO_LABEL};
use std::path::Path;
use std::time::{Duration, Instant};

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
        .create_project("Retime", Canvas::default());
    h.run_ok();
    h
}

fn project(h: &Harness<'_, App>) -> rw_model::Project {
    h.state().editor().session.as_ref().unwrap().project.clone()
}

fn two_backgrounds(h: &mut Harness<'_, App>) {
    for shade in [40, 200] {
        h.state_mut()
            .editor_mut()
            .apply(EditorAction::AppendGenerated(Generated::Solid {
                color: Rgba([shade, 90, 60, 255]),
            }));
        let end = project(h).duration();
        h.state_mut().editor_mut().apply(EditorAction::Seek(end));
    }
    h.run_ok();
}

#[test]
fn effects_are_added_tuned_and_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    two_backgrounds(&mut h);
    h.get_by_label("Effects").click();
    h.run_ok();
    h.get_by_label("Vignette").click();
    h.run_ok();
    h.get_by_label("Pixelate").click();
    h.run_ok();
    let selected = |h: &Harness<'_, App>| {
        let p = project(h);
        let s = h.state().editor().session.as_ref().unwrap();
        p.items()
            .find(|i| s.selection.contains(&i.id))
            .unwrap()
            .clone()
    };
    let kinds: Vec<EffectKind> = selected(&h).look.effects.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [EffectKind::Vignette, EffectKind::Pixelate]);
    // Apply the pixelate first.
    h.get_by_label("Apply Pixelate earlier").click();
    h.run_ok();
    let kinds: Vec<EffectKind> = selected(&h).look.effects.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [EffectKind::Pixelate, EffectKind::Vignette]);
    h.get_by_label("Remove Vignette").click();
    h.run_ok();
    assert_eq!(selected(&h).look.effects.len(), 1);
    h.get_by_label(UNDO_LABEL).click();
    h.run_ok();
    assert_eq!(selected(&h).look.effects.len(), 2);
}

#[test]
fn transition_from_library_then_edited_and_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    two_backgrounds(&mut h);
    h.get_by_label("Transitions").click();
    h.run_ok();
    h.get_by_label("Push").click();
    h.run_ok();
    let main = project(&h).main_track().unwrap().clone();
    assert_eq!(main.transitions.len(), 1, "added on the cut");
    let tr = &main.transitions[0];
    assert_eq!(tr.kind, TransitionKind::Push);
    assert_eq!((tr.a, tr.b), (main.items[0].id, main.items[1].id));
    // The new transition is selected and its panel shows.
    h.run_ok();
    h.get_by_label("Remove transition").click();
    h.run_ok();
    assert!(project(&h).main_track().unwrap().transitions.is_empty());
    h.get_by_label(UNDO_LABEL).click();
    h.run_ok();
    assert_eq!(project(&h).main_track().unwrap().transitions.len(), 1);
}

fn wait(h: &mut Harness<'_, App>, secs: u64, mut done: impl FnMut(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out");
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    h.step();
}

#[test]
fn speed_reverse_and_freeze_from_the_speed_tab() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    let fx = rw_media::fixtures::ensure();
    let name = "h264_1080p30_aac.mp4";
    if !fx.path(name).exists() {
        eprintln!("skipping: fixture {name} not available");
        return;
    }
    h.state_mut().editor_mut().import_paths(vec![fx.path(name)]);
    wait(&mut h, 60, |a| {
        let s = a.editor().session.as_ref().unwrap();
        !s.importing() && a.editor().jobs().pending() == 0
    });
    let media = project(&h).media[0].id;
    h.state_mut()
        .editor_mut()
        .apply(EditorAction::AppendMedia(media));
    h.run_ok();
    let before = project(&h).items().next().unwrap().duration;

    h.get_by_label("Speed").click();
    h.run_ok();
    h.get_by_label("2×").click();
    h.run_ok();
    let item = project(&h).items().next().unwrap().clone();
    assert_eq!(item.speed, 2.0);
    let half = Time(before.ticks() / 2);
    assert!((item.duration - half).ticks().abs() <= 1, "length halves");

    h.get_by_label("Play backwards").click();
    h.run_ok();
    assert!(project(&h).items().next().unwrap().reverse);
    h.get_by_label("Keep the pitch of the sound").click();
    h.run_ok();
    assert!(!project(&h).items().next().unwrap().keep_pitch);

    h.state_mut()
        .editor_mut()
        .apply(EditorAction::Seek(Time::from_millis(500)));
    h.run_ok();
    h.get_by_label("Freeze frame at playhead").click();
    h.run_ok();
    let main = project(&h).main_track().unwrap().clone();
    assert_eq!(main.items.len(), 3, "split around a held frame");
    assert!(main.items[1].held_frame().is_some());
    assert_eq!(main.items[1].start, Time::from_millis(500));
}
