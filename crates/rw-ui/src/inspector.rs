//! Properties of the selected clips.

use crate::actions::EditorAction;
use crate::format;
use crate::session::Session;
use crate::theme::Tokens;
use eframe::egui::{self, RichText};
use rw_edit::timeline::TimelineOp;
use rw_model::{ItemId, MAX_GAIN, MediaKind, Time};

pub const EMPTY_LABEL: &str = "Select an item to edit its properties.";
pub const VOLUME_LABEL: &str = "Volume";

/// Quietest settable level; anything below is silence.
const MIN_DB: f32 = -60.0;

pub fn gain_to_db(gain: f32) -> f32 {
    if gain <= 0.0 {
        MIN_DB
    } else {
        (20.0 * gain.log10()).max(MIN_DB)
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    if db <= MIN_DB {
        0.0
    } else {
        10f32.powf(db / 20.0).min(MAX_GAIN)
    }
}

pub fn show(ui: &mut egui::Ui, session: &Session, tokens: Tokens) -> Vec<EditorAction> {
    let mut out = Vec::new();
    let project = &session.project;
    let items: Vec<_> = project
        .items()
        .filter(|i| session.selection.contains(&i.id))
        .collect();
    if items.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new(EMPTY_LABEL).color(tokens.text_muted));
        });
        return out;
    }
    let ids: Vec<ItemId> = items.iter().map(|i| i.id).collect();
    let fps = project.canvas.frame_rate;
    ui.add_space(4.0);
    if let [item] = items.as_slice() {
        let media = item.media().and_then(|m| project.media(m));
        ui.label(RichText::new(media.map_or("Missing media", |m| m.name.as_str())).strong());
        egui::Grid::new("clip-info")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Starts").color(tokens.text_muted));
                ui.label(format::timecode(item.start, fps));
                ui.end_row();
                ui.label(RichText::new("Length").color(tokens.text_muted));
                ui.label(format::timecode(item.duration, fps));
                ui.end_row();
                ui.label(RichText::new("From source").color(tokens.text_muted));
                ui.label(format::timecode(item.source_in, fps));
                ui.end_row();
            });
        if media.is_some_and(|m| m.kind() == MediaKind::Image) {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Duration");
                let id = ui.make_persistent_id(("image-duration", item.id));
                let mut secs: f64 = ui
                    .data(|d| d.get_temp(id))
                    .unwrap_or_else(|| item.duration.as_secs_f64());
                let r = ui.add(
                    egui::DragValue::new(&mut secs)
                        .range(0.1..=3600.0)
                        .speed(0.05)
                        .suffix(" s")
                        .max_decimals(2),
                );
                if r.changed() {
                    ui.data_mut(|d| d.insert_temp(id, secs));
                }
                if r.drag_stopped() || r.lost_focus() {
                    ui.data_mut(|d| d.remove::<f64>(id));
                    let duration = Time::from_secs_f64(secs);
                    if duration != item.duration {
                        out.push(EditorAction::Op(TimelineOp::SetDuration {
                            items: vec![item.id],
                            duration,
                        }));
                    }
                }
            });
        }
    } else {
        ui.label(RichText::new(format!("{} clips selected", items.len())).strong());
    }

    let audible: Vec<_> = items
        .iter()
        .filter(|i| {
            i.media()
                .and_then(|m| project.media(m))
                .is_some_and(|m| m.info.audio.is_some())
        })
        .collect();
    if !audible.is_empty() {
        ui.add_space(12.0);
        ui.label(RichText::new("Audio").strong());
        ui.horizontal(|ui| {
            ui.label(VOLUME_LABEL);
            let id = ui.make_persistent_id(("volume", &ids));
            let current = gain_to_db(audible[0].gain);
            let mut db: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(current);
            let slider = egui::Slider::new(&mut db, MIN_DB..=12.0)
                .suffix(" dB")
                .custom_formatter(|v, _| {
                    if v <= f64::from(MIN_DB) {
                        "−∞".into()
                    } else {
                        format!("{v:+.1}")
                    }
                });
            let r = ui.add(slider);
            if r.changed() {
                ui.data_mut(|d| d.insert_temp(id, db));
            }
            if (r.drag_stopped() || (r.changed() && !r.dragged())) && (db - current).abs() > 0.01 {
                ui.data_mut(|d| d.remove::<f32>(id));
                out.push(EditorAction::Op(TimelineOp::SetGain {
                    items: ids.clone(),
                    gain: db_to_gain(db),
                }));
            }
        });
        let all_muted = audible.iter().all(|i| i.muted);
        let mut muted = all_muted;
        if ui.checkbox(&mut muted, "Mute").changed() {
            out.push(EditorAction::Op(TimelineOp::SetMuted {
                items: ids.clone(),
                muted,
            }));
        }
        let can_detach = items.iter().any(|i| {
            !i.audio_detached
                && i.media()
                    .and_then(|m| project.media(m))
                    .is_some_and(|m| m.kind() == MediaKind::Video && m.info.audio.is_some())
        });
        if can_detach && ui.button("Detach audio").clicked() {
            out.push(EditorAction::DetachAudio);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_round_trip() {
        assert_eq!(db_to_gain(0.0), 1.0);
        assert!((gain_to_db(db_to_gain(-6.0)) + 6.0).abs() < 1e-4);
        assert_eq!(db_to_gain(-60.0), 0.0);
        assert_eq!(gain_to_db(0.0), -60.0);
        assert!((db_to_gain(12.0) - MAX_GAIN).abs() < 1e-4);
    }
}
