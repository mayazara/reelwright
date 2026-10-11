//! UI smoke tests driven through AccessKit, without a window or GPU.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rw_model::{AspectPreset, Canvas, MediaKind, file};
use rw_ui::{App, Config, REDO_LABEL, Screen, UNDO_LABEL, home, media_bin};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn config(root: &Path) -> Config {
    Config {
        projects_dir: root.join("Projects"),
        settings_file: root.join("config/settings.json"),
    }
}

fn harness(root: &Path) -> Harness<'static, App> {
    let config = config(root);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_eframe(move |cc| App::new(cc, config));
    h.run_ok();
    h
}

/// Steps the UI until `done` holds or `timeout` passes.
fn step_until(h: &mut Harness<'_, App>, timeout: Duration, mut done: impl FnMut(&App) -> bool) {
    let deadline = Instant::now() + timeout;
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for the UI");
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    h.step();
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

fn drop_files(h: &mut Harness<'_, App>, paths: &[PathBuf]) {
    for p in paths {
        h.input_mut()
            .dropped_files
            .push(Arc::new(Dropped(p.clone())) as egui::DroppedFileHandle);
    }
    h.step();
}

#[test]
fn opens_to_empty_home_screen() {
    let tmp = tempfile::tempdir().unwrap();
    let h = harness(tmp.path());
    assert_eq!(h.state().editor().screen(), Screen::Home);
    h.get_by_label(home::NEW_PROJECT_LABEL);
    h.get_by_label(home::EMPTY_RECENTS_LABEL);
}

#[test]
fn theme_toggle_flips_and_persists() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    let before = h.state().editor().theme;
    h.get_by_label(home::THEME_TOGGLE_LABEL).click();
    h.run_ok();
    assert_eq!(h.state().editor().theme, before.toggled());
    drop(h);
    let h = harness(tmp.path());
    assert_eq!(
        h.state().editor().theme,
        before.toggled(),
        "restored from settings"
    );
}

#[test]
fn new_project_with_vertical_preset_opens_editor() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    h.get_by_label(home::NEW_PROJECT_LABEL).click();
    h.run_ok();
    h.get_by_label(AspectPreset::Vertical9x16.ratio_label())
        .click();
    h.run_ok();
    h.get_by_label(home::CREATE_LABEL).click();
    h.run_ok();

    let ui = h.state().editor();
    assert_eq!(ui.screen(), Screen::Editor);
    let session = ui.session.as_ref().unwrap();
    assert_eq!(
        session.project.canvas,
        Canvas::from_preset(AspectPreset::Vertical9x16)
    );
    assert_eq!(file::load(&session.dir).unwrap().canvas.height, 1920);
    h.get_by_label(media_bin::EMPTY_LABEL);
}

#[test]
fn import_undo_redo_autosave_and_reopen() {
    let fixtures = rw_media::fixtures::ensure();
    let names = [
        "h264_1080p30_aac.mp4",
        "still.png",
        "tone.mp3",
        "animated.gif",
    ];
    let paths: Vec<PathBuf> = names.iter().map(|n| fixtures.path(n)).collect();
    for p in &paths {
        assert!(
            p.exists(),
            "fixture {} missing: {:?}",
            p.display(),
            fixtures.skipped
        );
    }

    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    h.state_mut()
        .editor_mut()
        .create_project("Weekend", Canvas::default());
    h.run_ok();

    // Drop three files and a folder holding the fourth.
    let folder = tmp.path().join("drop-folder");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::copy(&paths[3], folder.join(names[3])).unwrap();
    drop_files(
        &mut h,
        &[paths[0].clone(), paths[1].clone(), paths[2].clone(), folder],
    );
    step_until(&mut h, Duration::from_secs(60), |a| {
        let s = a.editor().session.as_ref().unwrap();
        s.project.media.len() == 4 && a.editor().jobs().pending() == 0
    });
    for n in names {
        h.get_by_label(n);
    }
    assert_eq!(h.get_all_by_label(media_bin::UNUSED_BADGE).count(), 4);

    // Undo removes the whole import in one step; redo brings it back.
    h.get_by_label(UNDO_LABEL).click();
    h.run_ok();
    assert!(
        h.state()
            .editor()
            .session
            .as_ref()
            .unwrap()
            .project
            .media
            .is_empty()
    );
    h.get_by_label(REDO_LABEL).click();
    h.run_ok();
    assert_eq!(
        h.state()
            .editor()
            .session
            .as_ref()
            .unwrap()
            .project
            .media
            .len(),
        4
    );

    // Dropping the same file again is skipped as a duplicate.
    drop_files(&mut h, &[paths[0].clone()]);
    step_until(&mut h, Duration::from_secs(30), |a| {
        !a.editor().session.as_ref().unwrap().importing()
    });
    assert_eq!(
        h.state()
            .editor()
            .session
            .as_ref()
            .unwrap()
            .project
            .media
            .len(),
        4
    );

    // Idle autosave writes without the user doing anything.
    std::thread::sleep(rw_ui::session::AUTOSAVE_IDLE + Duration::from_millis(100));
    step_until(&mut h, Duration::from_secs(10), |a| {
        !a.editor().session.as_ref().unwrap().is_dirty()
    });
    let session = h.state().editor().session.as_ref().unwrap();
    let dir = session.dir.clone();
    let saved = session.project.clone();
    assert_eq!(file::load(&dir).unwrap(), saved);

    // Search filters the bin.
    h.state_mut().editor_mut().bin.search = "tone".into();
    h.run_ok();
    h.get_by_label("tone.mp3");
    assert!(h.query_by_label("still.png").is_none());
    h.state_mut().editor_mut().bin.search.clear();
    h.state_mut().editor_mut().bin.filter = Some(MediaKind::Image);
    h.run_ok();
    h.get_by_label("still.png");
    assert!(h.query_by_label("tone.mp3").is_none());

    // "Restart": a brand-new app instance on the same folders.
    h.state_mut().editor_mut().shutdown();
    drop(h);
    let mut h = harness(tmp.path());
    assert_eq!(h.state().editor().screen(), Screen::Home);
    h.get_by_label("Weekend").click();
    h.run_ok();
    let session = h.state().editor().session.as_ref().unwrap();
    assert_eq!(session.project, saved, "identical state after restart");
    assert!(
        session.cache.project_thumbnail().exists(),
        "home card thumbnail"
    );
    step_until(&mut h, Duration::from_secs(30), |a| {
        a.editor().jobs().pending() == 0
    });
    for n in names {
        h.get_by_label(n);
    }
}

#[test]
fn duplicate_and_delete_from_home() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    h.state_mut()
        .editor_mut()
        .create_project("Original", Canvas::default());
    h.run_ok();
    h.get_by_label(rw_ui::HOME_LABEL).click();
    h.run_ok();
    h.get_by_label("Original");

    h.get_by_label("…").click();
    h.run_ok();
    h.get_by_label(home::DUPLICATE_LABEL).click();
    h.run_ok();
    h.get_by_label("Original (copy)");
    assert_eq!(h.state().editor().home.projects.len(), 2);

    let menus = h.get_all_by_label("…").count();
    assert_eq!(menus, 2);
    h.get_all_by_label("…").next().unwrap().click();
    h.run_ok();
    h.get_by_label(home::DELETE_LABEL).click();
    h.run_ok();
    h.get_by_label(home::DELETE_CONFIRM_LABEL).click();
    h.run_ok();
    assert_eq!(h.state().editor().home.projects.len(), 1);
}
