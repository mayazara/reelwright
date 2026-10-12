//! M6 acceptance: brand-kit fonts and colours apply to new text in one
//! click, and to selected clips in one click.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rw_model::{Canvas, Rgba, TrackKind};
use rw_ui::actions::EditorAction;
use rw_ui::brand_kit::ADD_BRAND_TEXT_LABEL;
use rw_ui::{App, Config};

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
        .create_project("Brand", Canvas::default());
    h.run_ok();
    h
}

fn project(h: &Harness<'_, App>) -> rw_model::Project {
    h.state().editor().session.as_ref().unwrap().project.clone()
}

/// The default colour the "Add colour" picker starts with.
const TEAL: Rgba = Rgba([14, 124, 116, 255]);

#[test]
fn brand_style_applies_in_one_click() {
    let tmp = tempfile::tempdir().unwrap();
    // A kit saved earlier with a font in it is loaded for the project.
    let kit_dir = tmp.path().join("brand-kit");
    let mut kit = rw_assets::brand::BrandKit::default();
    kit.add_font("Lora");
    kit.save(&kit_dir).unwrap();
    let mut h = harness(tmp.path());
    h.get_by_label("Brand kit").click();
    h.run_ok();
    h.get_by_label("Add colour").click();
    h.run_ok();
    // Saved for every project.
    let kit = rw_assets::brand::BrandKit::load(&kit_dir);
    assert_eq!(kit.colours, [TEAL]);
    assert_eq!(kit.fonts, ["Lora"]);

    // One click: new text in the brand font and colour.
    h.get_by_label(ADD_BRAND_TEXT_LABEL).click();
    h.run_ok();
    let text: Vec<rw_model::Item> = project(&h)
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Text)
        .flat_map(|t| t.items.clone())
        .collect();
    assert_eq!(text.len(), 1);
    let style = &text[0].text().unwrap().style;
    assert_eq!((style.font.as_str(), style.color), ("Lora", TEAL));

    // One click on a swatch colours the selected shape.
    let mut shape = rw_model::Item::shape_item(
        rw_model::ShapeSpec::new(rw_model::ShapeKind::Star),
        rw_model::Time::ZERO,
    );
    shape.duration = rw_model::Time::from_secs(2);
    h.state_mut()
        .editor_mut()
        .apply(EditorAction::AddGraphic(shape));
    h.run_ok();
    h.get_by_label("Brand kit").click();
    h.run_ok();
    h.get_by_label(&format!("Brand colour {}", rw_ui::inspector::to_hex(TEAL)))
        .click();
    h.run_ok();
    let star = project(&h)
        .items()
        .find(|i| matches!(i.kind, rw_model::ItemKind::Shape { .. }))
        .cloned()
        .unwrap();
    let rw_model::ItemKind::Shape { shape } = &star.kind else {
        unreachable!()
    };
    assert_eq!(shape.fill, Some(TEAL));
}
