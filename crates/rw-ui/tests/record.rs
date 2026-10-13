//! M7: the Record tab and the recording window. Recordings are saved in
//! the project's Recordings folder and placed on the timeline: the screen
//! on the main track, the camera in the corner above it, starting together.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use rw_capture::Sources;
use rw_capture::sources::{TestPattern, test_tone};
use rw_model::{Canvas, Layout, TrackKind, Transform};
use rw_ui::record::{self, Active, Choice, Devices, OverlayAction};
use rw_ui::{App, Config};
use std::time::Duration;

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
        .create_project("Rec", Canvas::default());
    h.run_ok();
    h
}

fn project(h: &Harness<'_, App>) -> rw_model::Project {
    h.state().editor().session.as_ref().unwrap().project.clone()
}

/// Runs a few frames (a new window is measured on its first frame and
/// can't be clicked until it's drawn).
fn settle(h: &mut Harness<'static, App>) {
    for _ in 0..3 {
        h.step();
    }
}

fn fake_screen() -> rw_capture::devices::VideoDevice {
    rw_capture::devices::VideoDevice {
        label: "Test screen".into(),
        spec: rw_media::DeviceSpec {
            format: "none".into(),
            url: "none".into(),
            options: vec![("draw_mouse".into(), "1".into())],
        },
    }
}

fn choice() -> Choice {
    Choice {
        screen: Some(fake_screen().spec),
        camera: None,
        mic: None,
        system_sound: false,
        mirror: true,
    }
}

/// Records test pictures (and a test tone) for about `secs`, then stops.
fn record(h: &mut Harness<'static, App>, camera: bool, screen: bool, secs: f64) {
    let time = h.state().editor().bin.record.time.clone();
    let sources = Sources {
        screen: screen.then(|| Box::new(TestPattern::new((320, 180), 30)) as _),
        camera: camera.then(|| Box::new(TestPattern::new((160, 120), 30)) as _),
        audio: vec![test_tone(time, 0.0)],
    };
    assert!(
        h.state_mut()
            .editor_mut()
            .begin_recording(sources, choice())
    );
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs_f64(secs) {
        h.step();
        std::thread::sleep(Duration::from_millis(50));
    }
    h.step();
    h.get_by_label(record::STOP_LABEL).click();
    h.step();
    h.step();
    assert!(h.state().editor().recording().is_none());
}

#[test]
fn record_tab_offers_devices_and_starts_a_countdown() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    h.state_mut().editor_mut().bin.record.set_devices(Devices {
        screens: vec![fake_screen()],
        cameras: vec![rw_capture::devices::VideoDevice {
            label: "Test camera".into(),
            ..fake_screen()
        }],
        mics: Vec::new(),
    });
    h.get_by_label("Record").click();
    h.run_ok();
    h.get_by_label("Show the mouse pointer");
    // No microphone chosen: no meter, so nothing was opened.
    assert!(h.query_all_by_label("Microphone").next().is_some());

    // The chosen pointer setting goes to the device.
    h.get_by_label("Show the mouse pointer").click();
    h.run_ok();
    let c = h.state_mut().editor_mut().bin.record.choice();
    assert_eq!(
        c.screen.unwrap().options,
        [("draw_mouse".to_owned(), "0".to_owned())]
    );

    // A camera brings its size and mirror choices.
    assert!(h.query_by_label("Mirror the camera").is_none());
    h.state_mut().editor_mut().bin.record.camera = Some(0);
    h.run_ok();
    h.get_by_label("Mirror the camera");
    h.state_mut().editor_mut().bin.record.camera_size = record::CameraSize::P720;
    let c = h.state_mut().editor_mut().bin.record.choice();
    assert!(c.mirror);
    assert!(
        c.camera
            .unwrap()
            .options
            .contains(&("video_size".to_owned(), "1280x720".to_owned()))
    );

    h.get_by_label(record::START_LABEL).click();
    settle(&mut h);
    assert!(matches!(
        h.state().editor().recording(),
        Some(Active::Countdown { .. })
    ));
    h.get_by_label("Recording starts in");
    h.get_by_label("Cancel").click();
    settle(&mut h);
    assert!(h.state().editor().recording().is_none());
}

#[test]
fn nothing_chosen_cannot_start() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    let p = &mut h.state_mut().editor_mut().bin.record;
    p.set_devices(Devices::default());
    p.screen = None;
    h.get_by_label("Record").click();
    h.run_ok();
    assert!(
        h.get_by_label(record::START_LABEL)
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn screen_and_camera_land_aligned_and_undo_as_one() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    record(&mut h, true, true, 1.5);

    let p = project(&h);
    let dir = h
        .state()
        .editor()
        .session
        .as_ref()
        .unwrap()
        .dir
        .join("Recordings");
    assert!(dir.join("Recording 1 - Screen.mp4").is_file());
    assert!(dir.join("Recording 1 - Camera.mp4").is_file());
    assert_eq!(p.media.len(), 2);

    let main = p.main_track().expect("a main track");
    assert_eq!(main.items.len(), 1);
    let screen = &main.items[0];
    let cam_track = p
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Video && !t.main && !t.items.is_empty())
        .expect("the camera's track");
    let cam = &cam_track.items[0];
    let main_at = p.tracks.iter().position(|t| t.main).unwrap();
    let cam_at = p.tracks.iter().position(|t| t.id == cam_track.id).unwrap();
    assert!(cam_at > main_at, "the camera is above the screen");
    assert_eq!(cam.start, screen.start);
    assert!(screen.group.is_some());
    assert_eq!(cam.group, screen.group);
    // In the corner, mirrored (the default); the screen isn't flipped.
    let mut corner = Layout::BottomRight.apply(Transform::default());
    corner.flip_h = true;
    assert_eq!(cam.look.transform, corner);
    assert!(!screen.look.transform.flip_h);
    // Both files start at the recording's zero and end together (to
    // within two frames), so the camera stays in step with the sound.
    let diff = (cam.duration.as_secs_f64() - screen.duration.as_secs_f64()).abs();
    assert!(diff < 2.0 / 30.0, "{diff}");
    // The sound went into the screen file.
    let screen_media = p.media(screen.media().unwrap()).unwrap();
    assert!(screen_media.info.audio.is_some());

    // One undo removes the clips and the files from the library.
    let s = h.state_mut().editor_mut().session.as_mut().unwrap();
    s.undo();
    assert!(s.project.media.is_empty());
    assert!(s.project.items().next().is_none());
    s.redo();
    assert_eq!(s.project.media.len(), 2);
    assert_eq!(s.project.items().count(), 2);
}

#[test]
fn camera_follows_the_screen_when_the_main_track_moves_it() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    let ed = h.state_mut().editor_mut();
    ed.apply(rw_ui::actions::EditorAction::AppendGenerated(
        rw_model::Generated::Solid {
            color: rw_model::Rgba([0, 0, 0, 255]),
        },
    ));
    let s = ed.session.as_mut().unwrap();
    // Inside the solid: the main track puts the screen at a clip edge,
    // not at the playhead, and the camera must start with it.
    s.seek(rw_model::Time::from_secs(1));
    h.run_ok();
    record(&mut h, true, true, 1.0);
    let p = project(&h);
    let main = p.main_track().unwrap();
    let screen = main
        .items
        .iter()
        .find(|i| i.generated_fill().is_none())
        .unwrap();
    assert_ne!(screen.start, rw_model::Time::from_secs(1));
    let cam = p
        .tracks
        .iter()
        .filter(|t| !t.main && t.kind == TrackKind::Video)
        .flat_map(|t| t.items.iter())
        .next()
        .expect("the camera");
    assert_eq!(cam.start, screen.start);
}

#[test]
fn sound_only_goes_on_a_sound_track() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    record(&mut h, false, false, 1.0);
    let p = project(&h);
    assert_eq!(p.media.len(), 1);
    let track = p
        .tracks
        .iter()
        .find(|t| !t.items.is_empty())
        .expect("a track with the recording");
    assert_eq!(track.kind, TrackKind::Audio);
    let dir = h
        .state()
        .editor()
        .session
        .as_ref()
        .unwrap()
        .dir
        .join("Recordings");
    assert!(dir.join("Recording 1 - Sound.m4a").is_file());
}

#[test]
fn pause_and_retake() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    let time = h.state().editor().bin.record.time.clone();
    let sources = Sources {
        screen: Some(Box::new(TestPattern::new((320, 180), 30))),
        camera: None,
        audio: vec![test_tone(time, 0.0)],
    };
    let ed = h.state_mut().editor_mut();
    assert!(ed.begin_recording(sources, choice()));
    std::thread::sleep(Duration::from_millis(300));
    settle(&mut h);
    h.get_by_label(record::PAUSE_LABEL).click();
    settle(&mut h);
    let Some(Active::Running { recorder, .. }) = h.state().editor().recording() else {
        panic!("still recording");
    };
    assert!(recorder.is_paused());
    h.get_by_label(record::RESUME_LABEL);
    h.get_by_label("Paused");
    let Some(Active::Running { recorder, .. }) = h.state().editor().recording() else {
        panic!("still recording");
    };
    let paused_at = recorder.elapsed();
    std::thread::sleep(Duration::from_millis(300));
    let Some(Active::Running { recorder, .. }) = h.state().editor().recording() else {
        panic!("still recording");
    };
    assert_eq!(recorder.elapsed(), paused_at);

    // A retake throws the take away and counts down again.
    h.state_mut()
        .editor_mut()
        .recording_action(OverlayAction::Retake);
    assert!(matches!(
        h.state().editor().recording(),
        Some(Active::Countdown { .. })
    ));
    let dir = h
        .state()
        .editor()
        .session
        .as_ref()
        .unwrap()
        .dir
        .join("Recordings");
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert!(left.is_empty(), "{left:?}");
    assert!(project(&h).media.is_empty());
    h.state_mut()
        .editor_mut()
        .recording_action(OverlayAction::Cancel);
    assert!(h.state().editor().recording().is_none());
}

#[test]
fn file_names_count_up() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(record::next_name(tmp.path()), "Recording 1");
    std::fs::write(tmp.path().join("Recording 1 - Screen.mp4"), b"").unwrap();
    assert_eq!(record::next_name(tmp.path()), "Recording 2");
}
