//! M10: crash recovery, recordings left outside a project, and Settings.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rw_model::Canvas;
use rw_ui::settings::PreviewQuality;
use rw_ui::{App, Config, EditorUi, home, settings_window};
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

#[test]
fn after_a_crash_the_project_is_offered_again() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = {
        let mut h = harness(tmp.path());
        h.state_mut()
            .editor_mut()
            .create_project("Holiday", Canvas::default());
        h.run_ok();
        h.state().editor().session.as_ref().unwrap().dir.clone()
        // Dropped without shutting down, as a crash would.
    };
    let mut h = harness(tmp.path());
    h.get_by_label_contains("didn't close properly");
    h.get_by_label_contains("Holiday\u{201d} was saved automatically");
    h.get_by_label(home::REOPEN_LABEL).click();
    h.run_ok();
    let s = h.state().editor().session.as_ref().expect("reopened");
    assert_eq!(s.dir, dir);
    assert!(h.state().editor().home.unclean.is_none());
}

#[test]
fn a_clean_exit_shows_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let mut h = harness(tmp.path());
        h.state_mut()
            .editor_mut()
            .create_project("Fine", Canvas::default());
        h.run_ok();
        h.state_mut().editor_mut().shutdown();
    }
    let h = harness(tmp.path());
    assert!(h.query_by_label_contains("didn't close properly").is_none());
}

/// Records half a second of test pictures and sound into `dir`.
fn record_into(dir: &Path) {
    use rw_capture::clock::{SystemTime, TimeSource};
    use rw_capture::sources::{TestPattern, test_tone};
    let time: std::sync::Arc<dyn TimeSource> = std::sync::Arc::new(SystemTime::new());
    let rec = rw_capture::Recorder::start(
        rw_capture::Sources {
            screen: Some(Box::new(TestPattern::new((320, 180), 30))),
            camera: None,
            audio: vec![test_tone(time.clone(), 0.0)],
        },
        rw_capture::Options {
            folder: dir.join("Recordings"),
            name: "Recording 1".into(),
            fps: 30,
            encoder: None,
        },
        time,
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(600));
    rec.finish();
}

#[test]
fn recordings_left_outside_the_project_are_offered() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    let ed = h.state_mut().editor_mut();
    ed.create_project("Rec", Canvas::default());
    let dir = ed.session.as_ref().unwrap().dir.clone();
    ed.close_project();
    // The app closed mid-recording: the file is there, the project
    // doesn't know it.
    record_into(&dir);
    h.state_mut().editor_mut().open_project(&dir);
    h.run_ok();
    assert_eq!(h.state().editor().stray_recordings.len(), 1);
    h.get_by_label_contains("aren't in the project");
    h.get_by_label(EditorUi::ADD_STRAY_LABEL).click();
    h.run_ok();
    wait(&mut h, 30, |a| {
        a.editor()
            .session
            .as_ref()
            .is_some_and(|s| s.project.media.len() == 1)
    });
    assert!(h.state().editor().stray_recordings.is_empty());
    // Known now: not offered again.
    let ed = h.state_mut().editor_mut();
    ed.close_project();
    ed.open_project(&dir);
    assert!(ed.stray_recordings.is_empty());
}

#[test]
fn settings_change_theme_preview_quality_and_clear_caches() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    // A closed project with something in its cache.
    let ed = h.state_mut().editor_mut();
    ed.create_project("Old", Canvas::default());
    let old = ed.session.as_ref().unwrap().dir.clone();
    ed.close_project();
    let cache = rw_cache::ProjectCache::for_project(&old);
    std::fs::create_dir_all(cache.root().join("proxies")).unwrap();
    std::fs::write(cache.root().join("proxies/x.mkv"), vec![0u8; 4096]).unwrap();
    h.run_ok();

    h.get_by_label(home::SETTINGS_LABEL).click();
    h.run_ok();
    h.get_by_label("Preview quality");
    h.get_by_label("Light").click();
    h.run_ok();
    assert_eq!(h.state().editor().theme, rw_ui::theme::ThemeMode::Light);
    h.get_by_label("Half").click();
    h.run_ok();
    assert_eq!(
        h.state().editor().settings().preview_quality,
        PreviewQuality::Half
    );
    assert!(
        h.state()
            .editor()
            .settings_window
            .as_ref()
            .unwrap()
            .cache_bytes
            >= 4096
    );
    h.get_by_label(settings_window::CLEAR_CACHE_LABEL).click();
    h.run_ok();
    assert!(!cache.root().join("proxies/x.mkv").exists());
    assert_eq!(
        h.state()
            .editor()
            .settings_window
            .as_ref()
            .unwrap()
            .cache_bytes,
        0
    );

    // Saved, and a project opened now previews at half size.
    let saved = rw_ui::settings::Settings::load(&tmp.path().join("settings.json"));
    assert_eq!(saved.preview_quality, PreviewQuality::Half);
    h.state_mut().editor_mut().open_project(&old);
    assert_eq!(
        h.state().editor().session.as_ref().unwrap().preview_scale,
        0.5
    );
}
