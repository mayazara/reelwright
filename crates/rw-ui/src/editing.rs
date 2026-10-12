//! Applies [`EditorAction`]s to a session.

use crate::actions::EditorAction;
use crate::session::Session;
use crate::timeline::logic::{self, DropRow};
use rw_edit::timeline::{Placement, TimelineEdit, TimelineOp};
use rw_edit::{Command, Compound};
use rw_model::{
    GroupId, Item, ItemId, MediaKind, Project, Time, TrackId, TrackKind, track_kind_for,
};
use std::collections::HashMap;

/// How long a new freeze frame holds.
pub const FREEZE_DURATION: Time = Time::from_secs(2);

fn placement(project: &Project, row: DropRow) -> Placement {
    match row {
        DropRow::Track(i) => Placement::Track(project.tracks[i].id),
        DropRow::New { kind, index } => Placement::NewTrack { kind, index },
    }
}

impl Session {
    fn op(&mut self, op: TimelineOp) -> bool {
        self.execute(Box::new(TimelineEdit::new(op))).is_ok()
    }

    fn selected(&self) -> Vec<ItemId> {
        self.selection.iter().copied().collect()
    }

    /// Size the preview renders at; playback decodes for it.
    pub fn preview_size(&self) -> (u32, u32) {
        self.preview_size
    }

    pub fn seek(&mut self, t: Time) {
        let end = self.project.duration();
        let size = self.preview_size;
        self.player
            .seek(t.max(Time::ZERO).min(end.max(Time::ZERO)), size, true);
    }

    pub fn toggle_play(&mut self) {
        if self.player.is_playing() {
            self.player.pause();
        } else {
            let end = self.project.duration();
            if end <= Time::ZERO {
                return;
            }
            let mut from = self.player.position();
            if from >= end - self.project.canvas.frame_rate.frame_duration() {
                from = Time::ZERO;
            }
            let size = self.preview_size;
            self.player.play(from, size, true);
        }
    }

    /// Stops at the end of the timeline. Call once per frame.
    pub fn tick_playback(&mut self) {
        if self.player.is_playing() {
            let end = self.project.duration();
            if self.player.position() >= end {
                self.player.pause();
                let size = self.preview_size;
                self.player.seek(end, size, true);
            }
        }
    }

    fn copy_selection(&mut self) {
        let mut items: Vec<(TrackId, TrackKind, Item)> = self
            .project
            .tracks
            .iter()
            .flat_map(|t| t.items.iter().map(move |i| (t.id, t.kind, i.clone())))
            .filter(|(_, _, i)| self.selection.contains(&i.id))
            .collect();
        items.sort_by_key(|(_, _, i)| i.start);
        self.clipboard.items = items;
    }

    /// Inserts copies of the clipboard so its earliest clip starts at `at`.
    fn paste_at(&mut self, at: Time) {
        let items = self.clipboard.items.clone();
        if items.is_empty() {
            return;
        }
        let first = items
            .iter()
            .map(|(_, _, i)| i.start)
            .min()
            .unwrap_or(Time::ZERO);
        // Fresh ids; grouped clips stay grouped with each other (not with
        // the originals).
        let mut groups: HashMap<GroupId, GroupId> = HashMap::new();
        let mut by_track: Vec<(TrackId, TrackKind, Vec<Item>)> = Vec::new();
        for (track, kind, item) in items {
            let mut copy = item.clone();
            copy.id = ItemId::new();
            copy.group = item.group.map(|g| *groups.entry(g).or_default()); // default = fresh random id
            copy.start = item.start - first;
            match by_track.iter_mut().find(|(t, _, _)| *t == track) {
                Some((_, _, v)) => v.push(copy),
                None => by_track.push((track, kind, vec![copy])),
            }
        }
        let mut new_ids = Vec::new();
        let cmds: Vec<Box<dyn Command>> = by_track
            .into_iter()
            .map(|(track, kind, items)| {
                let offset = items.iter().map(|i| i.start).min().unwrap_or(Time::ZERO);
                new_ids.extend(items.iter().map(|i| i.id));
                let placement = if self.project.track(track).is_some() {
                    Placement::Track(track)
                } else {
                    Placement::NewTrack {
                        kind,
                        index: self.project.tracks.len(),
                    }
                };
                Box::new(TimelineEdit::new(TimelineOp::Insert {
                    placement,
                    at: at + offset,
                    items,
                })) as Box<dyn Command>
            })
            .collect();
        if self.execute(Box::new(Compound::new("Paste", cmds))).is_ok() {
            self.selection = new_ids.into_iter().collect();
        }
    }

    /// Adds media at the playhead: visual media onto the main track,
    /// sound onto the first audio track with room (or a new one).
    fn append_media(&mut self, media: rw_model::MediaId) {
        let Some(m) = self.project.media(media).cloned() else {
            return;
        };
        let at = self.playhead();
        let kind = track_kind_for(m.kind());
        let row = if kind == TrackKind::Audio {
            let item_end = at + rw_model::default_duration(&m);
            self.project
                .tracks
                .iter()
                .position(|t| {
                    t.kind == TrackKind::Audio
                        && !t.locked
                        && !t.items.iter().any(|i| i.start < item_end && i.end() > at)
                })
                .map_or(
                    DropRow::New {
                        kind: TrackKind::Audio,
                        index: self.project.tracks.len(),
                    },
                    DropRow::Track,
                )
        } else {
            match self.project.tracks.iter().position(|t| t.main) {
                Some(i) => DropRow::Track(i),
                None => DropRow::New {
                    kind: TrackKind::Video,
                    index: 0,
                },
            }
        };
        self.apply(EditorAction::InsertMedia { media, row, at });
    }

    pub fn apply(&mut self, action: EditorAction) {
        let fps = self.project.canvas.frame_rate;
        match action {
            EditorAction::Op(op) => {
                self.op(op);
            }
            EditorAction::Seek(t) => self.seek(t),
            EditorAction::PlayPause => self.toggle_play(),
            EditorAction::SplitAtPlayhead => {
                let at = self.playhead();
                let items = self.selected();
                self.op(TimelineOp::Split { at, items });
            }
            EditorAction::Delete { ripple } => {
                if let Some(id) = self.timeline.selected_transition.take() {
                    self.op(TimelineOp::RemoveTransition { id });
                    return;
                }
                let items = self.selected();
                if !items.is_empty() && self.op(TimelineOp::Delete { items, ripple }) {
                    self.selection.clear();
                }
            }
            EditorAction::Copy => self.copy_selection(),
            EditorAction::Paste => {
                let at = self.playhead();
                self.paste_at(at);
            }
            EditorAction::Duplicate => {
                if self.selection.is_empty() {
                    return;
                }
                let end = self
                    .project
                    .items()
                    .filter(|i| self.selection.contains(&i.id))
                    .map(Item::end)
                    .max()
                    .unwrap_or(Time::ZERO);
                let saved = std::mem::take(&mut self.clipboard);
                self.copy_selection();
                self.paste_at(end);
                self.clipboard = saved;
            }
            EditorAction::Group => {
                let items = self.selected();
                self.op(TimelineOp::Group { items });
            }
            EditorAction::Ungroup => {
                let items = self.selected();
                self.op(TimelineOp::Ungroup { items });
            }
            EditorAction::DetachAudio => {
                let targets: Vec<ItemId> = self
                    .project
                    .items()
                    .filter(|i| self.selection.contains(&i.id) && !i.audio_detached)
                    .filter(|i| {
                        i.media()
                            .and_then(|m| self.project.media(m))
                            .is_some_and(|m| m.kind() == MediaKind::Video && m.info.audio.is_some())
                    })
                    .map(|i| i.id)
                    .collect();
                let cmds: Vec<Box<dyn Command>> = targets
                    .into_iter()
                    .map(|item| {
                        Box::new(TimelineEdit::new(TimelineOp::DetachAudio { item }))
                            as Box<dyn Command>
                    })
                    .collect();
                if !cmds.is_empty() {
                    let _ = self.execute(Box::new(Compound::new("Detach audio", cmds)));
                }
            }
            EditorAction::StepFrames(n) => {
                let frame = fps.time_to_frame(self.playhead()) + n;
                self.seek(fps.frame_to_time(frame.max(0)));
            }
            EditorAction::PrevEdit => {
                let t = logic::prev_edit(&self.project, self.playhead());
                self.seek(t);
            }
            EditorAction::NextEdit => {
                let t = logic::next_edit(&self.project, self.playhead());
                self.seek(t);
            }
            EditorAction::ZoomIn | EditorAction::ZoomOut => {
                let factor = if action == EditorAction::ZoomIn {
                    1.25
                } else {
                    0.8
                };
                let x = self.timeline.view.x_of(self.playhead());
                self.timeline.view.zoom_about(factor, x);
            }
            EditorAction::ZoomFit => self.timeline.fit_pending = true,
            EditorAction::InsertMedia { media, row, at } => {
                let Some(m) = self.project.media(media).cloned() else {
                    return;
                };
                let item = Item::new(&m, Time::ZERO);
                let id = item.id;
                let placement = placement(&self.project, row);
                if self.op(TimelineOp::Insert {
                    placement,
                    at,
                    items: vec![item],
                }) {
                    self.selection = [id].into_iter().collect();
                }
            }
            EditorAction::AppendMedia(media) => self.append_media(media),
            EditorAction::AppendGenerated(fill) => {
                let row = match self.project.tracks.iter().position(|t| t.main) {
                    Some(i) => DropRow::Track(i),
                    None => DropRow::New {
                        kind: TrackKind::Video,
                        index: 0,
                    },
                };
                let at = self.playhead();
                self.apply(EditorAction::InsertGenerated { fill, row, at });
            }
            EditorAction::InsertGenerated { fill, row, at } => {
                let item = Item::generated(fill, Time::ZERO);
                let id = item.id;
                let placement = placement(&self.project, row);
                if self.op(TimelineOp::Insert {
                    placement,
                    at,
                    items: vec![item],
                }) {
                    self.selection = [id].into_iter().collect();
                }
            }
            // Export is run by the UI, which owns the GPU and dialogs.
            EditorAction::Export => {}
            EditorAction::AddMarker => {
                let at = self.playhead();
                let _ = self.execute(Box::new(rw_edit::commands::AddMarker::new(at)));
            }
            EditorAction::RemoveMarker(id) => {
                let _ = self.execute(Box::new(rw_edit::commands::RemoveMarker::new(id)));
            }
            EditorAction::MoveMarker(id, at) => {
                let _ = self.execute(Box::new(rw_edit::commands::MoveMarker::new(id, at)));
            }
            EditorAction::FreezeFrame => {
                let at = self.playhead();
                let target = self
                    .project
                    .items()
                    .find(|i| {
                        self.selection.contains(&i.id)
                            && i.contains(at)
                            && i.held_frame().is_none()
                            && i.media()
                                .and_then(|m| self.project.media(m))
                                .is_some_and(|m| m.kind() == MediaKind::Video)
                    })
                    .map(|i| i.id);
                match target {
                    Some(item) => {
                        self.op(TimelineOp::Freeze {
                            item,
                            at,
                            duration: FREEZE_DURATION,
                        });
                    }
                    None => {
                        self.notify("Select a video clip under the playhead to freeze a frame.")
                    }
                }
            }
            EditorAction::AddTransition(kind) => {
                let cuts = logic::cuts(&self.project);
                match logic::cut_for_selection(&cuts, &self.selection) {
                    Some(c) => {
                        if self.op(TimelineOp::AddTransition {
                            a: c.a,
                            kind,
                            duration: rw_model::DEFAULT_TRANSITION,
                        }) {
                            self.timeline.selected_transition = logic::cuts(&self.project)
                                .into_iter()
                                .find(|x| x.a == c.a)
                                .and_then(|x| x.transition);
                            self.selection.clear();
                        }
                    }
                    None => self.notify(
                        "Select a clip that touches another, or drag the transition onto a cut.",
                    ),
                }
            }
            EditorAction::Notice(text) => self.notify(text),
        }
    }
}
