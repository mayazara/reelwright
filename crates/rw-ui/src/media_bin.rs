//! The media bin: imported files with search, type filter, sort and
//! used/unused badges.

use crate::format;
use crate::layout::grid;
use crate::session::{JobState, Session};
use crate::textures::Textures;
use crate::theme::Tokens;
use eframe::egui::{self, Color32, CornerRadius, RichText, Sense, Stroke};
use rw_model::{MediaId, MediaKind, MediaRef, Project};

pub const IMPORT_FILES_LABEL: &str = "Import files…";
pub const IMPORT_FOLDER_LABEL: &str = "Import folder…";
pub const SEARCH_HINT: &str = "Search media";
pub const EMPTY_LABEL: &str =
    "Import media to get started. You can also drop files or folders anywhere in this window.";
pub const NO_MATCHES_LABEL: &str = "Nothing matches your search.";
pub const USED_BADGE: &str = "In use";
pub const UNUSED_BADGE: &str = "Unused";
pub const MISSING_BADGE: &str = "Missing";
pub const RENAME_LABEL: &str = "Rename…";
pub const REMOVE_LABEL: &str = "Remove from project";
pub const ADD_LABEL: &str = "Add to timeline";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    /// Newest imports first.
    #[default]
    Imported,
    Name,
    Duration,
    Kind,
}

impl SortKey {
    pub const ALL: [SortKey; 4] = [
        SortKey::Imported,
        SortKey::Name,
        SortKey::Duration,
        SortKey::Kind,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Imported => "Recently imported",
            SortKey::Name => "Name",
            SortKey::Duration => "Length",
            SortKey::Kind => "Type",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibraryTab {
    #[default]
    Media,
    Backgrounds,
    Transitions,
}

#[derive(Debug, Default)]
pub struct BinState {
    pub tab: LibraryTab,
    pub search: String,
    pub filter: Option<MediaKind>,
    pub sort: SortKey,
    pub renaming: Option<(MediaId, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinAction {
    ImportFiles,
    ImportFolder,
    Remove(MediaId),
    Rename(MediaId, String),
    AddToTimeline(MediaId),
    AddGenerated(rw_model::Generated),
    AddTransition(rw_model::TransitionKind),
}

/// Media to show, filtered and sorted. Search matches the name
/// case-insensitively; every whitespace-separated word must match.
pub fn visible<'a>(project: &'a Project, state: &BinState) -> Vec<&'a MediaRef> {
    let words: Vec<String> = state
        .search
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let mut out: Vec<(usize, &MediaRef)> = project
        .media
        .iter()
        .enumerate()
        .filter(|(_, m)| state.filter.is_none_or(|k| m.kind() == k))
        .filter(|(_, m)| {
            let name = m.name.to_lowercase();
            words.iter().all(|w| name.contains(w.as_str()))
        })
        .collect();
    match state.sort {
        // Later in the list means imported later.
        SortKey::Imported => {
            out.sort_by(|a, b| b.1.imported_at.cmp(&a.1.imported_at).then(b.0.cmp(&a.0)))
        }
        SortKey::Name => out.sort_by_key(|(_, m)| m.name.to_lowercase()),
        SortKey::Duration => out.sort_by_key(|a| std::cmp::Reverse(a.1.info.duration)),
        SortKey::Kind => {
            out.sort_by_key(|(i, m)| (MediaKind::ALL.iter().position(|k| *k == m.kind()), *i))
        }
    }
    out.into_iter().map(|(_, m)| m).collect()
}

pub fn show(
    ui: &mut egui::Ui,
    session: &Session,
    state: &mut BinState,
    textures: &mut Textures,
    tokens: Tokens,
) -> Option<BinAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.tab, LibraryTab::Media, "Your media");
        ui.selectable_value(&mut state.tab, LibraryTab::Backgrounds, "Backgrounds");
        ui.selectable_value(&mut state.tab, LibraryTab::Transitions, "Transitions");
    });
    ui.separator();
    if state.tab == LibraryTab::Backgrounds {
        return crate::backgrounds::show(ui, tokens).map(BinAction::AddGenerated);
    }
    if state.tab == LibraryTab::Transitions {
        return egui::ScrollArea::vertical()
            .show(ui, |ui| crate::transitions::show(ui, tokens))
            .inner
            .map(BinAction::AddTransition);
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button(IMPORT_FILES_LABEL).clicked() {
            action = Some(BinAction::ImportFiles);
        }
        if ui.button(IMPORT_FOLDER_LABEL).clicked() {
            action = Some(BinAction::ImportFolder);
        }
        if session.importing() {
            ui.spinner();
            ui.label(RichText::new("Importing…").color(tokens.text_muted));
        }
    });
    ui.add_space(4.0);
    ui.add(
        egui::TextEdit::singleline(&mut state.search)
            .hint_text(SEARCH_HINT)
            .desired_width(f32::INFINITY),
    );
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("bin-filter")
            .selected_text(state.filter.map_or("All types", MediaKind::label))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.filter, None, "All types");
                for k in MediaKind::ALL {
                    ui.selectable_value(&mut state.filter, Some(k), k.label());
                }
            });
        egui::ComboBox::from_id_salt("bin-sort")
            .selected_text(state.sort.label())
            .show_ui(ui, |ui| {
                for k in SortKey::ALL {
                    ui.selectable_value(&mut state.sort, k, k.label());
                }
            });
    });
    ui.separator();

    let project = &session.project;
    if project.media.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(EMPTY_LABEL).color(tokens.text_muted));
        });
        return action;
    }
    let items = visible(project, state);
    if items.is_empty() {
        ui.label(RichText::new(NO_MATCHES_LABEL).color(tokens.text_muted));
        return action;
    }
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            grid(ui, &items, TILE_OUTER_W, 8.0, |ui, m| {
                if let Some(a) = tile(ui, session, m, state, textures, tokens) {
                    action = Some(a);
                }
            });
        });
    rename_modal(ui.ctx(), state).or(action)
}

const TILE_W: f32 = 148.0;
/// Tile width including its frame margin and stroke.
const TILE_OUTER_W: f32 = TILE_W + 10.0;
const TILE_THUMB_H: f32 = 84.0;

fn tile(
    ui: &mut egui::Ui,
    session: &Session,
    m: &MediaRef,
    state: &mut BinState,
    textures: &mut Textures,
    tokens: Tokens,
) -> Option<BinAction> {
    let mut action = None;
    let used = session.project.is_media_used(m.id);
    let missing = !m.path.exists();
    let resp = egui::Frame::new()
        .fill(tokens.surface_raised)
        .stroke(Stroke::new(1.0, tokens.border))
        .corner_radius(CornerRadius::same(tokens.radius))
        .inner_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            ui.set_width(TILE_W);
            ui.vertical(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(TILE_W, TILE_THUMB_H), Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, CornerRadius::same(4), tokens.preview_backdrop);
                let thumb = session.cache.thumbnail(&m.hash);
                let tex = if m.kind() == MediaKind::Audio {
                    None
                } else {
                    match session.thumbs.get(&m.hash) {
                        Some(JobState::Running(_)) => None,
                        _ => textures.get(ui.ctx(), &thumb),
                    }
                };
                if let Some(tex) = tex {
                    let size = tex.size_vec2();
                    let scale = (rect.width() / size.x).min(rect.height() / size.y);
                    painter.image(
                        tex.id(),
                        egui::Rect::from_center_size(rect.center(), size * scale),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else if m.kind() == MediaKind::Audio {
                    // A few bars suggest a waveform without needing peaks yet.
                    let mid = rect.center().y;
                    for (i, h) in [8.0, 18.0, 30.0, 14.0, 24.0, 10.0, 20.0, 6.0]
                        .iter()
                        .enumerate()
                    {
                        let x = rect.center().x - 28.0 + i as f32 * 8.0;
                        painter.line_segment(
                            [egui::pos2(x, mid - h / 2.0), egui::pos2(x, mid + h / 2.0)],
                            Stroke::new(3.0, tokens.accent),
                        );
                    }
                } else if let Some(JobState::Running(_)) | None = session.thumbs.get(&m.hash) {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "…",
                        egui::FontId::proportional(20.0),
                        tokens.text_muted,
                    );
                }
                if m.info.duration.ticks() > 0 {
                    let text = format::duration(m.info.duration);
                    let pos = rect.right_bottom() - egui::vec2(4.0, 4.0);
                    let galley = painter.layout_no_wrap(
                        text,
                        egui::FontId::proportional(11.0),
                        Color32::WHITE,
                    );
                    let bg = egui::Rect::from_min_size(
                        pos - galley.size() - egui::vec2(4.0, 2.0),
                        galley.size() + egui::vec2(6.0, 3.0),
                    );
                    painter.rect_filled(bg, CornerRadius::same(3), Color32::from_black_alpha(170));
                    painter.galley(bg.min + egui::vec2(3.0, 1.0), galley, Color32::WHITE);
                }
                if let Some(JobState::Running(f)) = session.proxies.get(&m.hash) {
                    let bar = egui::Rect::from_min_max(
                        egui::pos2(rect.min.x, rect.max.y - 3.0),
                        egui::pos2(rect.min.x + rect.width() * f.clamp(0.0, 1.0), rect.max.y),
                    );
                    painter.rect_filled(bar, CornerRadius::ZERO, tokens.highlight);
                }

                ui.add(egui::Label::new(RichText::new(&m.name).small()).truncate());
                ui.horizontal(|ui| {
                    let (badge, color) = if missing {
                        (MISSING_BADGE, tokens.danger)
                    } else if used {
                        (USED_BADGE, tokens.accent)
                    } else {
                        (UNUSED_BADGE, tokens.text_muted)
                    };
                    ui.label(RichText::new(badge).small().color(color));
                    ui.label(
                        RichText::new(m.kind().label())
                            .small()
                            .color(tokens.text_muted),
                    );
                });
            });
        })
        .response
        .interact(Sense::click_and_drag());
    // Drag onto the timeline; double-click adds at the playhead.
    if resp.dragged() {
        resp.dnd_set_drag_payload(m.id);
    }
    if resp.double_clicked() && !missing {
        action = Some(BinAction::AddToTimeline(m.id));
    }

    let mut tip = format!("{}\n{}", m.path.display(), format::bytes(m.size_bytes));
    if let Some(v) = &m.info.video {
        let (w, h) = v.display_size();
        tip.push_str(&format!(
            "\n{w}×{h} {} · {:.2} fps",
            v.codec,
            v.frame_rate.as_f64()
        ));
    }
    if let Some(a) = &m.info.audio {
        tip.push_str(&format!(
            "\n{} · {} Hz · {} ch",
            a.codec, a.sample_rate, a.channels
        ));
    }
    if let Some(JobState::Failed(e)) = session.proxies.get(&m.hash) {
        tip.push_str(&format!("\nPreview proxy failed: {e}"));
    }
    let resp = resp.on_hover_text(tip);
    resp.context_menu(|ui| {
        if ui
            .add_enabled(!missing, egui::Button::new(ADD_LABEL))
            .clicked()
        {
            action = Some(BinAction::AddToTimeline(m.id));
            ui.close();
        }
        if ui.button(RENAME_LABEL).clicked() {
            state.renaming = Some((m.id, m.name.clone()));
            ui.close();
        }
        let remove = ui
            .add_enabled(!used, egui::Button::new(REMOVE_LABEL))
            .on_disabled_hover_text("Used on the timeline. Remove those clips first.");
        if remove.clicked() {
            action = Some(BinAction::Remove(m.id));
            ui.close();
        }
    });
    action
}

fn rename_modal(ctx: &egui::Context, state: &mut BinState) -> Option<BinAction> {
    let (id, name) = state.renaming.as_mut()?;
    let mut action = None;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("rename-media")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.label("Rename media");
        let edit = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            if (ui.button("Rename").clicked() || enter) && !name.trim().is_empty() {
                action = Some(BinAction::Rename(*id, name.trim().to_owned()));
                close = true;
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });
    if close || modal.should_close() {
        state.renaming = None;
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use rw_model::*;
    use std::path::PathBuf;

    fn media(name: &str, kind: MediaKind, secs: i64, imported_at: u64) -> MediaRef {
        MediaRef {
            id: MediaId::new(),
            name: name.into(),
            path: PathBuf::from(name),
            hash: ContentHash(name.into()),
            size_bytes: 1,
            info: MediaInfo {
                kind,
                container: "x".into(),
                duration: Time::from_secs(secs),
                video: None,
                audio: None,
            },
            imported_at,
        }
    }

    fn names(v: Vec<&MediaRef>) -> Vec<&str> {
        v.into_iter().map(|m| m.name.as_str()).collect()
    }

    #[test]
    fn filter_search_and_sort() {
        let mut p = Project::new("p", Canvas::default());
        p.media = vec![
            media("Beach day.mp4", MediaKind::Video, 30, 1),
            media("theme song.mp3", MediaKind::Audio, 120, 1),
            media("Beach photo.jpg", MediaKind::Image, 0, 2),
            media("logo.gif", MediaKind::Animation, 2, 3),
        ];
        let mut s = BinState::default();
        assert_eq!(
            names(visible(&p, &s)),
            [
                "logo.gif",
                "Beach photo.jpg",
                "theme song.mp3",
                "Beach day.mp4"
            ]
        );

        s.search = "beach".into();
        assert_eq!(visible(&p, &s).len(), 2);
        s.search = "beach PHOTO".into();
        assert_eq!(names(visible(&p, &s)), ["Beach photo.jpg"]);

        s.search.clear();
        s.filter = Some(MediaKind::Audio);
        assert_eq!(names(visible(&p, &s)), ["theme song.mp3"]);

        s.filter = None;
        s.sort = SortKey::Name;
        assert_eq!(names(visible(&p, &s))[0], "Beach day.mp4");
        s.sort = SortKey::Duration;
        assert_eq!(names(visible(&p, &s))[0], "theme song.mp3");
        s.sort = SortKey::Kind;
        assert_eq!(
            names(visible(&p, &s)),
            [
                "Beach day.mp4",
                "theme song.mp3",
                "Beach photo.jpg",
                "logo.gif"
            ]
        );
    }
}
