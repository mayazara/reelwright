//! Timeline editing through the UI: shortcuts, clipboard, playback, export.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rw_model::{Canvas, Time, TrackKind};
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
        .with_size(egui::vec2(1400.0, 900.0))
        .build_eframe(move |cc| App::new(cc, config));
    h.run_ok();
    h
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

/// A project with `names` imported, ready for editing.
fn project_with(h: &mut Harness<'_, App>, names: &[&str]) {
    let fx = rw_media::fixtures::ensure();
    h.state_mut()
        .editor_mut()
        .create_project("Edit", Canvas::default());
    h.run_ok();
    let paths = names.iter().map(|n| fx.path(n)).collect();
    h.state_mut().editor_mut().import_paths(paths);
    wait(h, 60, |a| {
        let s = a.editor().session.as_ref().unwrap();
        !s.importing() && a.editor().jobs().pending() == 0
    });
}

fn media_id(h: &Harness<'_, App>, name: &str) -> rw_model::MediaId {
    let s = h.state().editor().session.as_ref().unwrap();
    s.project.media.iter().find(|m| m.name == name).unwrap().id
}

fn main_items(h: &Harness<'_, App>) -> Vec<(Time, Time)> {
    let s = h.state().editor().session.as_ref().unwrap();
    s.project
        .main_track()
        .unwrap()
        .items
        .iter()
        .map(|i| (i.start, i.duration))
        .collect()
}

#[test]
fn split_delete_undo_with_keyboard() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    project_with(&mut h, &["h264_1080p30_aac.mp4", "still.png"]);
    let clip = media_id(&h, "h264_1080p30_aac.mp4");
    let still = media_id(&h, "still.png");
    h.state_mut()
        .editor_mut()
        .apply(EditorAction::AppendMedia(clip));
    h.run_ok();
    // Inserting at the playhead (still 0) puts the image first and pushes the
    // clip right: the main track is magnetic.
    h.state_mut()
        .editor_mut()
        .apply(EditorAction::AppendMedia(still));
    h.run_ok();
    let items = main_items(&h);
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].1,
        Time::from_secs(5),
        "still images default to 5 s"
    );
    assert_eq!(items[1].0, Time::from_secs(5));

    // Seek into the clip and split with S.
    h.state_mut()
        .editor_mut()
        .apply(EditorAction::Seek(Time::from_secs(6)));
    h.state_mut()
        .editor_mut()
        .session
        .as_mut()
        .unwrap()
        .selection
        .clear();
    h.run_ok();
    h.key_press(egui::Key::S);
    h.run_ok();
    assert_eq!(main_items(&h).len(), 3, "split at playhead");

    // Select the middle piece and delete it: the gap closes.
    let mid = {
        let s = h.state().editor().session.as_ref().unwrap();
        s.project.main_track().unwrap().items[1].id
    };
    h.state_mut()
        .editor_mut()
        .session
        .as_mut()
        .unwrap()
        .selection = [mid].into_iter().collect();
    h.key_press(egui::Key::Delete);
    h.run_ok();
    let items = main_items(&h);
    assert_eq!(items.len(), 2);
    assert_eq!(items[1].0, Time::from_secs(5), "gap closed");

    h.get_by_label(UNDO_LABEL).click();
    h.run_ok();
    assert_eq!(main_items(&h).len(), 3);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_ok();
    assert_eq!(main_items(&h).len(), 2, "Ctrl+Z undoes the split");
}

#[test]
fn copy_paste_duplicate_and_detach() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    project_with(&mut h, &["h264_1080p30_aac.mp4"]);
    let clip = media_id(&h, "h264_1080p30_aac.mp4");
    let ed = h.state_mut().editor_mut();
    ed.apply(EditorAction::AppendMedia(clip));
    ed.apply(EditorAction::Copy);
    ed.apply(EditorAction::Seek(Time::from_secs(2)));
    ed.apply(EditorAction::Paste);
    h.run_ok();
    assert_eq!(main_items(&h).len(), 2);
    let ids: Vec<_> = h
        .state()
        .editor()
        .session
        .as_ref()
        .unwrap()
        .project
        .items()
        .map(|i| i.id)
        .collect();
    assert_ne!(ids[0], ids[1], "pasted clips get new ids");

    h.state_mut().editor_mut().apply(EditorAction::Duplicate);
    h.run_ok();
    assert_eq!(main_items(&h).len(), 3);

    h.state_mut().editor_mut().apply(EditorAction::DetachAudio);
    h.run_ok();
    let s = h.state().editor().session.as_ref().unwrap();
    let audio = s
        .project
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Audio)
        .expect("audio track");
    assert_eq!(audio.items.len(), 1);
    assert!(s.project.items().filter(|i| i.audio_detached).count() == 1);
}

#[test]
fn space_plays_and_pauses() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    project_with(&mut h, &["h264_1080p30_aac.mp4"]);
    let clip = media_id(&h, "h264_1080p30_aac.mp4");
    h.state_mut()
        .editor_mut()
        .apply(EditorAction::AppendMedia(clip));
    h.run_ok();
    h.key_press(egui::Key::Space);
    h.step();
    assert!(
        h.state()
            .editor()
            .session
            .as_ref()
            .unwrap()
            .player
            .is_playing()
    );
    std::thread::sleep(Duration::from_millis(400));
    h.step();
    h.key_press(egui::Key::Space);
    h.step();
    let s = h.state().editor().session.as_ref().unwrap();
    assert!(!s.player.is_playing());
    let pos = s.playhead();
    assert!(
        pos > Time::from_millis(200) && pos < Time::from_secs(2),
        "played to {pos}"
    );
}

#[test]
fn export_from_the_editor() {
    if rw_render::Gpu::headless().is_err() {
        eprintln!("skipping: no GPU");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    project_with(&mut h, &["still.png"]);
    let still = media_id(&h, "still.png");
    let ed = h.state_mut().editor_mut();
    ed.apply(EditorAction::AppendMedia(still));
    ed.apply(EditorAction::Op(
        rw_edit::timeline::TimelineOp::SetDuration {
            items: ed
                .session
                .as_ref()
                .unwrap()
                .project
                .items()
                .map(|i| i.id)
                .collect(),
            duration: Time::from_millis(500),
        },
    ));
    let out = tmp.path().join("out.mp4");
    h.state_mut().editor_mut().export_to(out.clone());
    assert!(h.state().editor().exporting().is_some());
    wait(&mut h, 120, |a| a.editor().exporting().is_none());
    assert!(out.exists(), "exported file");
    let notices = &h.state().editor().session.as_ref().unwrap().notices;
    assert!(
        notices.iter().any(|n| n.text.starts_with("Exported")),
        "{notices:?}"
    );
}
