//! M10 accessibility: every control a screen reader can reach has a name,
//! on every main screen, panel, tab and dialog.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use rw_model::Canvas;
use rw_ui::actions::EditorAction;
use rw_ui::media_bin::LibraryTab;
use rw_ui::{App, Config};
use std::time::{Duration, Instant};

fn harness(root: &std::path::Path) -> Harness<'static, App> {
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

/// Controls on screen now that have no name, as "screen: role at (x, y)".
fn unnamed(h: &Harness<'_, App>, screen: &str) -> Vec<String> {
    use egui::accesskit::Role;
    let mut out = Vec::new();
    for node in h.root().children_recursive() {
        let n = node.accesskit_node();
        let interactive = matches!(
            n.role(),
            Role::Button
                | Role::CheckBox
                | Role::ComboBox
                | Role::Slider
                | Role::SpinButton
                | Role::TextInput
                | Role::MultilineTextInput
                | Role::Tab
                | Role::RadioButton
                | Role::Link
                | Role::MenuItem
                | Role::Switch
        );
        // A slider's own value box is announced with the slider before it
        // (egui doesn't let it be named separately); see DECISIONS.
        let slider_value = n.role() == Role::SpinButton
            && n.preceding_siblings()
                .next()
                .is_some_and(|s| s.role() == Role::Slider && s.label().is_some());
        if interactive
            && !slider_value
            && !n.is_hidden()
            && n.label().is_none_or(|l| l.trim().is_empty())
        {
            let at = n
                .bounding_box()
                .map(|b| format!("({:.0}, {:.0})", b.x0, b.y0))
                .unwrap_or_default();
            out.push(format!("{screen}: {:?} at {at}", n.role()));
        }
    }
    out
}

#[test]
fn every_control_has_a_name() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    let mut problems = Vec::new();

    problems.extend(unnamed(&h, "home"));
    h.get_by_label(rw_ui::home::NEW_PROJECT_LABEL).click();
    h.run_ok();
    problems.extend(unnamed(&h, "new project dialog"));
    h.state_mut().editor_mut().home.new_dialog = None;
    h.state_mut().editor_mut().open_settings();
    h.run_ok();
    problems.extend(unnamed(&h, "settings"));
    h.state_mut().editor_mut().settings_window = None;
    h.state_mut().editor_mut().home.about = true;
    h.run_ok();
    problems.extend(unnamed(&h, "about"));
    h.state_mut().editor_mut().home.about = false;

    // An editor with a video clip (selected) and a title.
    h.state_mut()
        .editor_mut()
        .create_project("A11y", Canvas::default());
    h.run_ok();
    let fx = rw_media::fixtures::ensure();
    let clip = fx.path("h264_1080p30_aac.mp4");
    if clip.exists() {
        h.state_mut().editor_mut().import_paths(vec![clip]);
        wait(&mut h, 60, |a| {
            let s = a.editor().session.as_ref().unwrap();
            !s.importing() && a.editor().jobs().pending() == 0
        });
        let media = h.state().editor().session.as_ref().unwrap().project.media[0].id;
        h.state_mut()
            .editor_mut()
            .apply(EditorAction::AppendMedia(media));
        h.run_ok();
    }
    problems.extend(unnamed(&h, "editor"));
    for tab in [
        "Transform",
        "Adjust colours",
        "Filters",
        "Effects",
        "Speed",
        "Fade",
        "Audio",
    ] {
        if let Some(t) = h.query_by_label(tab) {
            t.click();
            h.run_ok();
            problems.extend(unnamed(&h, &format!("inspector {tab}")));
        }
    }
    for tab in [
        LibraryTab::Media,
        LibraryTab::Text,
        LibraryTab::Graphics,
        LibraryTab::Sounds,
        LibraryTab::Brand,
        LibraryTab::Backgrounds,
        LibraryTab::Transitions,
        LibraryTab::Record,
    ] {
        let ed = h.state_mut().editor_mut();
        ed.bin.tab = tab;
        if tab == LibraryTab::Record {
            ed.bin.record.set_devices(rw_ui::record::Devices::default());
        }
        h.run_ok();
        problems.extend(unnamed(&h, &format!("library {tab:?}")));
    }
    h.state_mut().editor_mut().apply(EditorAction::Export);
    h.run_ok();
    problems.extend(unnamed(&h, "export dialog"));
    h.state_mut().editor_mut().close_export_dialog();
    // Titles, shapes and visualisers have their own inspector tabs.
    let preset = rw_text::presets::all().remove(0);
    let mut shape = rw_model::Item::shape_item(
        rw_model::ShapeSpec::new(rw_model::ShapeKind::Star),
        rw_model::Time::ZERO,
    );
    shape.duration = rw_model::Time::from_secs(2);
    let mut bars = rw_model::Item::visualiser_item(
        rw_model::VisualiserSpec::new(rw_model::VisualiserStyle::Bars),
        rw_model::Time::ZERO,
    );
    bars.duration = rw_model::Time::from_secs(2);
    for (what, action) in [
        ("text", EditorAction::AddText(preset.into())),
        ("shape", EditorAction::AddGraphic(shape)),
        ("visualiser", EditorAction::AddGraphic(bars)),
    ] {
        h.state_mut().editor_mut().apply(action);
        h.run_ok();
        for tab in ["Text", "Shape", "Visualiser", "Transform", "Fade"] {
            // The inspector's tab: the right-most control of that name.
            let rightmost = h.query_all_by_label(tab).max_by(|a, b| {
                let x = |n: &egui_kittest::Node<'_>| {
                    n.accesskit_node().bounding_box().map_or(0.0, |b| b.x0)
                };
                x(a).total_cmp(&x(b))
            });
            if let Some(t) = rightmost {
                t.click();
                h.run_ok();
                problems.extend(unnamed(&h, &format!("{what} inspector {tab}")));
            }
        }
    }
    h.state_mut().editor_mut().cheat_sheet = true;
    h.run_ok();
    problems.extend(unnamed(&h, "shortcuts"));

    problems.sort();
    problems.dedup();
    assert!(
        problems.is_empty(),
        "{} unnamed controls:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

/// Tab reaches the home screen's buttons and Enter presses them: a project
/// can be started without a mouse.
#[test]
fn keyboard_alone_starts_a_project() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(tmp.path());
    let mut reached = false;
    for _ in 0..20 {
        h.key_press(egui::Key::Tab);
        h.run_ok();
        if h.get_by_label(rw_ui::home::NEW_PROJECT_LABEL)
            .accesskit_node()
            .is_focused()
        {
            reached = true;
            break;
        }
    }
    assert!(reached, "Tab never reached New project");
    h.key_press(egui::Key::Enter);
    h.run_ok();
    assert!(h.state().editor().home.new_dialog.is_some());
}
