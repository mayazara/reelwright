//! Timeline edits.
//!
//! Every edit is a [`TimelineOp`] run by a [`TimelineEdit`] command. The op
//! works on a copy of the tracks, which is then [normalised](normalize) and
//! swapped in, so a failing op leaves the project untouched. The command
//! keeps the previous tracks for undo. Ids an op creates are recorded on
//! first apply and reused on redo, so redo reproduces the same project.
//!
//! Rules (see `docs/DECISIONS.md`):
//! - The main track is magnetic: its items sit back to back from zero, in
//!   list order. Deleting closes the gap; inserting pushes later items.
//! - Other tracks are free, but items never overlap: an item that would
//!   overlap moves to a new track directly above.
//! - Empty tracks other than the main track are removed.
//! - Items on locked tracks are never changed.
//! - Grouped items move, trim and delete together.

use crate::{Command, EditError};
use rw_model::{
    GroupId, Item, ItemId, MAX_GAIN, MediaKind, MediaRef, Project, Time, Track, TrackId, TrackKind,
    max_duration,
};
use std::collections::{HashMap, HashSet};

/// Where items go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Track(TrackId),
    /// A new track of `kind` inserted at `index` in the track list
    /// (bottom-most first). Its id is recorded for redo.
    NewTrack {
        kind: TrackKind,
        index: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackChange {
    Locked(bool),
    Hidden(bool),
    Muted(bool),
    Gain(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TimelineOp {
    /// Places items. Their `start`s are relative: the earliest lands at `at`.
    Insert {
        placement: Placement,
        at: Time,
        items: Vec<Item>,
    },
    /// Shifts items (and their group mates) by `delta`. With `to`, items all
    /// on one track move to that track.
    Move {
        items: Vec<ItemId>,
        delta: Time,
        to: Option<Placement>,
    },
    /// Moves one edge of an item (and the same edge of its group mates) to
    /// the timeline time `to`. `ripple` shifts later items on free tracks.
    Trim {
        item: ItemId,
        edge: Edge,
        to: Time,
        ripple: bool,
    },
    /// Changes which part of the source an item shows, keeping its place.
    Slip {
        item: ItemId,
        source_in: Time,
    },
    /// Cuts the given items (or, if empty, every unlocked item) at `at`.
    Split {
        at: Time,
        items: Vec<ItemId>,
    },
    Delete {
        items: Vec<ItemId>,
        ripple: bool,
    },
    /// Closes the gap containing `at` on a free track.
    CloseGap {
        track: TrackId,
        at: Time,
    },
    Group {
        items: Vec<ItemId>,
    },
    Ungroup {
        items: Vec<ItemId>,
    },
    /// Moves a video item's sound to a linked item on an audio track.
    DetachAudio {
        item: ItemId,
    },
    SetGain {
        items: Vec<ItemId>,
        gain: f32,
    },
    SetMuted {
        items: Vec<ItemId>,
        muted: bool,
    },
    /// Sets the length of still-image items.
    SetDuration {
        items: Vec<ItemId>,
        duration: Time,
    },
    SetTrack {
        track: TrackId,
        change: TrackChange,
    },
}

impl TimelineOp {
    fn label(&self) -> String {
        match self {
            TimelineOp::Insert { items, .. } if items.len() == 1 => "Add clip".into(),
            TimelineOp::Insert { items, .. } => format!("Add {} clips", items.len()),
            TimelineOp::Move { .. } => "Move".into(),
            TimelineOp::Trim { .. } => "Trim".into(),
            TimelineOp::Slip { .. } => "Slip".into(),
            TimelineOp::Split { .. } => "Split".into(),
            TimelineOp::Delete { ripple: true, .. } => "Ripple delete".into(),
            TimelineOp::Delete { .. } => "Delete".into(),
            TimelineOp::CloseGap { .. } => "Remove gap".into(),
            TimelineOp::Group { .. } => "Group".into(),
            TimelineOp::Ungroup { .. } => "Ungroup".into(),
            TimelineOp::DetachAudio { .. } => "Detach audio".into(),
            TimelineOp::SetGain { .. } => "Change volume".into(),
            TimelineOp::SetMuted { muted: true, .. } => "Mute".into(),
            TimelineOp::SetMuted { .. } => "Unmute".into(),
            TimelineOp::SetDuration { .. } => "Change duration".into(),
            TimelineOp::SetTrack { change, .. } => match change {
                TrackChange::Locked(true) => "Lock track",
                TrackChange::Locked(false) => "Unlock track",
                TrackChange::Hidden(true) => "Hide track",
                TrackChange::Hidden(false) => "Show track",
                TrackChange::Muted(true) => "Mute track",
                TrackChange::Muted(false) => "Unmute track",
                TrackChange::Gain(_) => "Change track volume",
            }
            .into(),
        }
    }
}

/// Ids created while applying, replayed on redo.
#[derive(Debug, Default)]
struct Ids {
    made: Vec<uuid::Uuid>,
    next: usize,
}

impl Ids {
    fn rewind(&mut self) {
        self.next = 0;
    }

    fn take(&mut self) -> uuid::Uuid {
        if self.next == self.made.len() {
            self.made.push(uuid::Uuid::new_v4());
        }
        self.next += 1;
        self.made[self.next - 1]
    }

    fn item(&mut self) -> ItemId {
        ItemId(self.take())
    }

    fn track(&mut self) -> TrackId {
        TrackId(self.take())
    }

    fn group(&mut self) -> GroupId {
        GroupId(self.take())
    }
}

/// Command that applies a [`TimelineOp`].
#[derive(Debug)]
pub struct TimelineEdit {
    op: TimelineOp,
    ids: Ids,
    before: Option<Vec<Track>>,
}

impl TimelineEdit {
    pub fn new(op: TimelineOp) -> Self {
        TimelineEdit {
            op,
            ids: Ids::default(),
            before: None,
        }
    }
}

impl Command for TimelineEdit {
    fn label(&self) -> String {
        self.op.label()
    }

    fn apply(&mut self, project: &mut Project) -> Result<(), EditError> {
        self.ids.rewind();
        let mut tracks = project.tracks.clone();
        let mut ctx = Ctx {
            media: &project.media,
            min: project.canvas.frame_rate.frame_duration(),
            ids: &mut self.ids,
        };
        ctx.run(&self.op, &mut tracks)?;
        // Overlap resolution may create tracks; their ids are recorded too.
        normalize_with(&mut tracks, &mut || ctx.ids.track());
        self.before = Some(std::mem::replace(&mut project.tracks, tracks));
        Ok(())
    }

    fn revert(&mut self, project: &mut Project) {
        if let Some(before) = self.before.take() {
            project.tracks = before;
        }
    }
}

struct Ctx<'a> {
    media: &'a [MediaRef],
    /// Shortest allowed item: one frame.
    min: Time,
    ids: &'a mut Ids,
}

type Loc = (usize, usize);

fn locate(tracks: &[Track], id: ItemId) -> Option<Loc> {
    tracks
        .iter()
        .enumerate()
        .find_map(|(t, tr)| tr.items.iter().position(|i| i.id == id).map(|i| (t, i)))
}

fn item_mut(tracks: &mut [Track], id: ItemId) -> Option<&mut Item> {
    tracks
        .iter_mut()
        .flat_map(|t| t.items.iter_mut())
        .find(|i| i.id == id)
}

fn track_index(tracks: &[Track], id: TrackId) -> Result<usize, EditError> {
    tracks
        .iter()
        .position(|t| t.id == id)
        .ok_or(EditError::NotFound("track"))
}

/// The given items plus every item sharing a group with them, excluding
/// items on locked tracks.
fn expand(tracks: &[Track], ids: &[ItemId]) -> Vec<ItemId> {
    let wanted: HashSet<ItemId> = ids.iter().copied().collect();
    let groups: HashSet<GroupId> = tracks
        .iter()
        .flat_map(|t| &t.items)
        .filter(|i| wanted.contains(&i.id))
        .filter_map(|i| i.group)
        .collect();
    tracks
        .iter()
        .filter(|t| !t.locked)
        .flat_map(|t| &t.items)
        .filter(|i| wanted.contains(&i.id) || i.group.is_some_and(|g| groups.contains(&g)))
        .map(|i| i.id)
        .collect()
}

fn require_some(ids: Vec<ItemId>) -> Result<Vec<ItemId>, EditError> {
    if ids.is_empty() {
        Err(EditError::Invalid(
            "nothing to change (items missing or on locked tracks)".into(),
        ))
    } else {
        Ok(ids)
    }
}

/// Whether an item may sit on a track of `kind`.
fn fits(media: Option<&MediaRef>, kind: TrackKind) -> bool {
    match media.map(MediaRef::kind) {
        Some(MediaKind::Audio) => kind == TrackKind::Audio,
        Some(_) => matches!(kind, TrackKind::Video | TrackKind::Overlay),
        None => false,
    }
}

/// Index in a magnetic track's list where something dropped at `t` goes:
/// before the first item whose midpoint is after `t`.
fn main_index(track: &Track, t: Time) -> usize {
    track
        .items
        .iter()
        .position(|i| i.start + Time(i.duration.ticks() / 2) > t)
        .unwrap_or(track.items.len())
}

impl Ctx<'_> {
    fn media(&self, item: &Item) -> Option<&MediaRef> {
        item.media()
            .and_then(|id| self.media.iter().find(|m| m.id == id))
    }

    fn resolve(
        &mut self,
        tracks: &mut Vec<Track>,
        placement: Placement,
    ) -> Result<usize, EditError> {
        match placement {
            Placement::Track(id) => track_index(tracks, id),
            Placement::NewTrack { kind, index } => {
                let mut t = Track::new(kind);
                t.id = self.ids.track();
                let index = index.min(tracks.len());
                tracks.insert(index, t);
                Ok(index)
            }
        }
    }

    fn check_fits(&self, items: &[Item], track: &Track) -> Result<(), EditError> {
        if track.locked {
            return Err(EditError::Invalid("the track is locked".into()));
        }
        for item in items {
            if !fits(self.media(item), track.kind) {
                return Err(EditError::Invalid(
                    "that kind of clip can't go on this track".into(),
                ));
            }
        }
        Ok(())
    }

    /// Puts items (with absolute starts) onto a track. On the main track they
    /// are inserted as a block at the position nearest their start.
    fn place(&self, track: &mut Track, mut items: Vec<Item>) {
        items.sort_by_key(|i| i.start);
        if track.main {
            let at = items.first().map_or(Time::ZERO, |i| i.start);
            let index = main_index(track, at);
            track.items.splice(index..index, items);
        } else {
            for i in &mut items {
                i.start = i.start.max(Time::ZERO);
            }
            track.items.extend(items);
        }
    }

    fn run(&mut self, op: &TimelineOp, tracks: &mut Vec<Track>) -> Result<(), EditError> {
        match op {
            TimelineOp::Insert {
                placement,
                at,
                items,
            } => {
                if items.is_empty() {
                    return Err(EditError::Invalid("nothing to add".into()));
                }
                let t = self.resolve(tracks, *placement)?;
                self.check_fits(items, &tracks[t])?;
                let first = items.iter().map(|i| i.start).min().unwrap_or(Time::ZERO);
                let placed: Vec<Item> = items
                    .iter()
                    .map(|i| {
                        let mut i = i.clone();
                        i.start = *at + (i.start - first);
                        i.duration = i.duration.max(self.min);
                        i
                    })
                    .collect();
                let mut seen = HashSet::new();
                for i in &placed {
                    if !seen.insert(i.id) || locate(tracks, i.id).is_some() {
                        return Err(EditError::Invalid("item ids must be new".into()));
                    }
                }
                self.place(&mut tracks[t], placed);
            }

            TimelineOp::Move { items, delta, to } => {
                let ids = require_some(expand(tracks, items))?;
                let source_tracks: HashSet<usize> = ids
                    .iter()
                    .filter_map(|id| locate(tracks, *id))
                    .map(|(t, _)| t)
                    .collect();
                let target = match to {
                    Some(p) if source_tracks.len() == 1 => Some(self.resolve(tracks, *p)?),
                    Some(_) => {
                        return Err(EditError::Invalid(
                            "clips on several tracks can only move in time".into(),
                        ));
                    }
                    None => None,
                };
                // Take the items out (target index may shift if a track empties,
                // so tracks are only cleaned up by normalisation afterwards).
                let mut taken: Vec<(usize, Item)> = Vec::new();
                for (t, track) in tracks.iter_mut().enumerate() {
                    let (out, keep): (Vec<Item>, Vec<Item>) =
                        track.items.drain(..).partition(|i| ids.contains(&i.id));
                    track.items = keep;
                    taken.extend(out.into_iter().map(|i| (t, i)));
                }
                let mut by_track: HashMap<usize, Vec<Item>> = HashMap::new();
                for (t, mut item) in taken {
                    item.start = (item.start + *delta).max(Time::ZERO);
                    by_track.entry(target.unwrap_or(t)).or_default().push(item);
                }
                for (t, items) in by_track {
                    self.check_fits(&items, &tracks[t])?;
                    self.place(&mut tracks[t], items);
                }
            }

            TimelineOp::Trim {
                item,
                edge,
                to,
                ripple,
            } => {
                let ids = require_some(expand(tracks, &[*item]))?;
                let (t0, i0) = locate(tracks, *item).ok_or(EditError::NotFound("item"))?;
                let anchor = tracks[t0].items[i0].clone();
                let delta = match edge {
                    Edge::Start => *to - anchor.start,
                    Edge::End => *to - anchor.end(),
                };
                for id in ids {
                    let (t, i) = locate(tracks, id).ok_or(EditError::NotFound("item"))?;
                    let main = tracks[t].main;
                    let it = tracks[t].items[i].clone();
                    let limit = self.media(&it).and_then(|m| max_duration(m, Time::ZERO));
                    let mut next = it.clone();
                    match edge {
                        Edge::Start => {
                            // Can't reveal source before its beginning or past the end.
                            let earliest = -next.source_in;
                            let latest = next.duration - self.min;
                            let d = delta.max(earliest).min(latest);
                            let d = if main { d } else { d.max(-next.start) };
                            next.source_in += d;
                            if !main {
                                next.start += d;
                            }
                            next.duration -= d;
                        }
                        Edge::End => {
                            let mut dur = (next.duration + delta).max(self.min);
                            if let Some(total) = limit {
                                dur = dur.min((total - next.source_in).max(self.min));
                            }
                            let change = dur - next.duration;
                            next.duration = dur;
                            if *ripple && !main {
                                for later in tracks[t].items.iter_mut() {
                                    if later.id != id && later.start >= it.end() {
                                        later.start += change;
                                    }
                                }
                            }
                        }
                    }
                    tracks[t].items[i] = next;
                }
            }

            TimelineOp::Slip { item, source_in } => {
                let (t, i) = locate(tracks, *item).ok_or(EditError::NotFound("item"))?;
                if tracks[t].locked {
                    return Err(EditError::Invalid("the track is locked".into()));
                }
                let it = &tracks[t].items[i];
                let max_in = match self.media(it).and_then(|m| max_duration(m, Time::ZERO)) {
                    Some(total) => (total - it.duration).max(Time::ZERO),
                    None => Time::ZERO,
                };
                tracks[t].items[i].source_in = (*source_in).max(Time::ZERO).min(max_in);
            }

            TimelineOp::Split { at, items } => {
                let candidates: Vec<ItemId> = if items.is_empty() {
                    tracks
                        .iter()
                        .filter(|t| !t.locked)
                        .flat_map(|t| &t.items)
                        .map(|i| i.id)
                        .collect()
                } else {
                    expand(tracks, items)
                };
                let mut new_groups: HashMap<GroupId, GroupId> = HashMap::new();
                let mut any = false;
                for id in candidates {
                    let (t, i) = locate(tracks, id).ok_or(EditError::NotFound("item"))?;
                    let it = tracks[t].items[i].clone();
                    if *at - it.start < self.min || it.end() - *at < self.min {
                        continue;
                    }
                    any = true;
                    let left = *at - it.start;
                    let mut right = it.clone();
                    right.id = self.ids.item();
                    right.start = *at;
                    right.source_in = it.source_in + left;
                    right.duration = it.duration - left;
                    right.group = it
                        .group
                        .map(|g| *new_groups.entry(g).or_insert_with(|| self.ids.group()));
                    tracks[t].items[i].duration = left;
                    tracks[t].items.insert(i + 1, right);
                }
                if !any {
                    return Err(EditError::Invalid(
                        "nothing under the playhead to split".into(),
                    ));
                }
            }

            TimelineOp::Delete { items, ripple } => {
                let ids = require_some(expand(tracks, items))?;
                for track in tracks.iter_mut() {
                    let removed: Vec<(Time, Time)> = track
                        .items
                        .iter()
                        .filter(|i| ids.contains(&i.id))
                        .map(|i| (i.start, i.duration))
                        .collect();
                    track.items.retain(|i| !ids.contains(&i.id));
                    if *ripple && !track.main {
                        for later in track.items.iter_mut() {
                            let shift: Time = removed
                                .iter()
                                .filter(|(s, _)| *s < later.start)
                                .fold(Time::ZERO, |acc, (_, d)| acc + *d);
                            later.start = (later.start - shift).max(Time::ZERO);
                        }
                    }
                }
            }

            TimelineOp::CloseGap { track, at } => {
                let t = track_index(tracks, *track)?;
                let tr = &mut tracks[t];
                if tr.locked {
                    return Err(EditError::Invalid("the track is locked".into()));
                }
                let gap_start = tr
                    .items
                    .iter()
                    .map(Item::end)
                    .filter(|e| *e <= *at)
                    .max()
                    .unwrap_or(Time::ZERO);
                let gap_end = tr
                    .items
                    .iter()
                    .map(|i| i.start)
                    .filter(|s| *s > *at)
                    .min()
                    .ok_or_else(|| EditError::Invalid("no gap here".into()))?;
                if tr.items.iter().any(|i| i.contains(*at)) {
                    return Err(EditError::Invalid("no gap here".into()));
                }
                let shift = gap_end - gap_start;
                for i in tr.items.iter_mut().filter(|i| i.start >= gap_end) {
                    i.start -= shift;
                }
            }

            TimelineOp::Group { items } => {
                let ids = expand(tracks, items);
                if ids.len() < 2 {
                    return Err(EditError::Invalid(
                        "select at least two clips to group".into(),
                    ));
                }
                let g = self.ids.group();
                for id in ids {
                    if let Some(i) = item_mut(tracks, id) {
                        i.group = Some(g);
                    }
                }
            }

            TimelineOp::Ungroup { items } => {
                let ids = require_some(expand(tracks, items))?;
                for id in ids {
                    if let Some(i) = item_mut(tracks, id) {
                        i.group = None;
                    }
                }
            }

            TimelineOp::DetachAudio { item } => {
                let (t, i) = locate(tracks, *item).ok_or(EditError::NotFound("item"))?;
                let it = tracks[t].items[i].clone();
                let has_audio = self
                    .media(&it)
                    .is_some_and(|m| m.kind() == MediaKind::Video && m.info.audio.is_some());
                if !has_audio || it.audio_detached || tracks[t].locked {
                    return Err(EditError::Invalid(
                        "this clip has no sound to detach".into(),
                    ));
                }
                let group = it.group.unwrap_or_else(|| self.ids.group());
                let mut audio = it.clone();
                audio.id = self.ids.item();
                audio.group = Some(group);
                let video = &mut tracks[t].items[i];
                video.group = Some(group);
                video.audio_detached = true;
                // First audio track with room, else a new one at the bottom of
                // the audio tracks.
                let free = tracks.iter().position(|tr| {
                    tr.kind == TrackKind::Audio
                        && !tr.locked
                        && !tr
                            .items
                            .iter()
                            .any(|o| o.start < audio.end() && audio.start < o.end())
                });
                let dest = match free {
                    Some(d) => d,
                    None => {
                        let mut tr = Track::new(TrackKind::Audio);
                        tr.id = self.ids.track();
                        tracks.push(tr);
                        tracks.len() - 1
                    }
                };
                tracks[dest].items.push(audio);
            }

            TimelineOp::SetGain { items, gain } => {
                let ids = require_some(expand(tracks, items))?;
                for id in ids {
                    if let Some(i) = item_mut(tracks, id) {
                        i.gain = gain.clamp(0.0, MAX_GAIN);
                    }
                }
            }

            TimelineOp::SetMuted { items, muted } => {
                let ids = require_some(expand(tracks, items))?;
                for id in ids {
                    if let Some(i) = item_mut(tracks, id) {
                        i.muted = *muted;
                    }
                }
            }

            TimelineOp::SetDuration { items, duration } => {
                let ids = require_some(expand(tracks, items))?;
                let mut any = false;
                for id in ids {
                    let (t, i) = locate(tracks, id).ok_or(EditError::NotFound("item"))?;
                    let is_image = self
                        .media(&tracks[t].items[i])
                        .is_some_and(|m| m.kind() == MediaKind::Image);
                    if is_image {
                        tracks[t].items[i].duration = (*duration).max(self.min);
                        any = true;
                    }
                }
                if !any {
                    return Err(EditError::Invalid(
                        "only still images have a set duration".into(),
                    ));
                }
            }

            TimelineOp::SetTrack { track, change } => {
                let t = track_index(tracks, *track)?;
                let tr = &mut tracks[t];
                match *change {
                    TrackChange::Locked(v) => tr.locked = v,
                    TrackChange::Hidden(v) => tr.hidden = v,
                    TrackChange::Muted(v) => tr.muted = v,
                    TrackChange::Gain(g) => tr.gain = g.clamp(0.0, MAX_GAIN),
                }
            }
        }
        Ok(())
    }
}

/// Restores the timeline invariants. Tracks created to resolve overlaps get
/// fresh random ids.
pub fn normalize(tracks: &mut Vec<Track>) {
    normalize_with(tracks, &mut TrackId::new);
}

fn normalize_with(tracks: &mut Vec<Track>, new_track_id: &mut dyn FnMut() -> TrackId) {
    // At most one main track.
    let mut seen_main = false;
    for t in tracks.iter_mut() {
        if t.main && seen_main {
            t.main = false;
        }
        seen_main |= t.main;
    }
    let mut t = 0;
    while t < tracks.len() {
        if tracks[t].main {
            // Magnetic: pack in list order from zero.
            let mut at = Time::ZERO;
            for i in tracks[t].items.iter_mut() {
                i.start = at;
                at += i.duration;
            }
        } else {
            let items = &mut tracks[t].items;
            for i in items.iter_mut() {
                i.start = i.start.max(Time::ZERO);
            }
            items.sort_by_key(|i| (i.start, i.end()));
            let mut keep = Vec::with_capacity(items.len());
            let mut bumped = Vec::new();
            let mut end = Time::ZERO;
            for i in items.drain(..) {
                if i.start < end {
                    bumped.push(i);
                } else {
                    end = i.end();
                    keep.push(i);
                }
            }
            *items = keep;
            if !bumped.is_empty() && !tracks[t].locked {
                let mut above = Track::new(tracks[t].kind);
                above.id = new_track_id();
                above.items = bumped;
                tracks.insert(t + 1, above);
            } else if !bumped.is_empty() {
                // A locked track never changes; this only happens with
                // hand-edited files, so keep the overlap rather than lose items.
                tracks[t].items.extend(bumped);
                tracks[t].items.sort_by_key(|i| i.start);
            }
        }
        t += 1;
    }
    tracks.retain(|t| t.main || !t.items.is_empty());
}

/// Checks the invariants [`normalize`] establishes. Used by tests.
pub fn check_invariants(tracks: &[Track]) -> Result<(), String> {
    if tracks.iter().filter(|t| t.main).count() > 1 {
        return Err("more than one main track".into());
    }
    let mut ids = HashSet::new();
    for (n, t) in tracks.iter().enumerate() {
        if !t.main && t.items.is_empty() {
            return Err(format!("track {n} is empty"));
        }
        let mut at = Time::ZERO;
        for (k, i) in t.items.iter().enumerate() {
            if !ids.insert(i.id) {
                return Err(format!("duplicate item id {}", i.id));
            }
            if i.duration <= Time::ZERO || i.source_in < Time::ZERO || i.start < Time::ZERO {
                return Err(format!("track {n} item {k} has invalid times: {i:?}"));
            }
            if t.main && i.start != at {
                return Err(format!("main track has a gap or overlap at item {k}"));
            }
            if !t.main && i.start < at && !t.locked {
                return Err(format!("track {n} items {} and {k} overlap", k - 1));
            }
            at = i.end();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
