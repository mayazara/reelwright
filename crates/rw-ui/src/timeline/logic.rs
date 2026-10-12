//! Timeline geometry, row layout, snapping and selection: everything the
//! widget decides that doesn't need a window.

use rw_model::{FrameRate, Item, ItemId, Project, Time, TrackId, TrackKind, TransitionId};
use std::collections::BTreeSet;

/// Horizontal mapping between time and pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Zoom: pixels per second.
    pub px_per_sec: f32,
    /// Time at the left edge of the track area.
    pub scroll: Time,
}

impl View {
    pub const MIN_ZOOM: f32 = 2.0;
    pub const MAX_ZOOM: f32 = 2400.0;

    pub fn x_of(&self, t: Time) -> f32 {
        ((t - self.scroll).as_secs_f64() * f64::from(self.px_per_sec)) as f32
    }

    pub fn time_at(&self, x: f32) -> Time {
        self.scroll + Time::from_secs_f64(f64::from(x) / f64::from(self.px_per_sec))
    }

    pub fn duration_of(&self, px: f32) -> Time {
        Time::from_secs_f64(f64::from(px) / f64::from(self.px_per_sec))
    }

    /// Zooms by `factor` keeping the time under `anchor_x` fixed.
    pub fn zoom_about(&mut self, factor: f32, anchor_x: f32) {
        let anchor = self.time_at(anchor_x);
        self.px_per_sec = (self.px_per_sec * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        self.scroll = (anchor - self.duration_of(anchor_x)).max(Time::ZERO);
    }

    /// Fits `duration` into `width` pixels with a little room at the end.
    pub fn fit(&mut self, duration: Time, width: f32) {
        let secs = duration.as_secs_f64().max(1.0) * 1.05;
        self.px_per_sec = ((f64::from(width) / secs) as f32).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        self.scroll = Time::ZERO;
    }

    /// Ruler tick spacing: the smallest "nice" interval at least `min_px`
    /// apart, and how many minor ticks it holds.
    pub fn tick_step(&self, fps: FrameRate, min_px: f32) -> Time {
        let frame = fps.frame_duration();
        let candidates = [
            frame,
            frame + frame,
            Time(frame.ticks() * 5),
            Time(frame.ticks() * 10),
            Time::from_secs(1),
            Time::from_secs(2),
            Time::from_secs(5),
            Time::from_secs(10),
            Time::from_secs(30),
            Time::from_secs(60),
            Time::from_secs(120),
            Time::from_secs(300),
            Time::from_secs(600),
            Time::from_secs(1800),
            Time::from_secs(3600),
        ];
        candidates
            .into_iter()
            .find(|s| self.x_of(self.scroll + *s) >= min_px)
            .unwrap_or(Time::from_secs(3600))
    }
}

impl Default for View {
    fn default() -> Self {
        View {
            px_per_sec: 60.0,
            scroll: Time::ZERO,
        }
    }
}

/// One horizontal lane on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    /// Index into `project.tracks`.
    pub track: usize,
    pub id: TrackId,
    pub kind: TrackKind,
    pub main: bool,
    pub y: f32,
    pub height: f32,
}

pub const VIDEO_ROW_HEIGHT: f32 = 56.0;
pub const AUDIO_ROW_HEIGHT: f32 = 44.0;
pub const ROW_GAP: f32 = 4.0;
/// Strip above the top track: dropping a clip there puts it on a new track.
pub const NEW_TRACK_ZONE: f32 = 30.0;

/// Screen order: overlay/video tracks top-down (last drawn on top first),
/// then the main track, then audio tracks in list order.
pub fn rows(project: &Project) -> Vec<Row> {
    let visual: Vec<usize> = (0..project.tracks.len())
        .filter(|&i| project.tracks[i].kind != TrackKind::Audio)
        .collect();
    let audio: Vec<usize> = (0..project.tracks.len())
        .filter(|&i| project.tracks[i].kind == TrackKind::Audio)
        .collect();
    let mut y = NEW_TRACK_ZONE;
    let mut out = Vec::new();
    for &i in visual.iter().rev().chain(audio.iter()) {
        let t = &project.tracks[i];
        let height = if t.kind == TrackKind::Audio {
            AUDIO_ROW_HEIGHT
        } else {
            VIDEO_ROW_HEIGHT
        };
        out.push(Row {
            track: i,
            id: t.id,
            kind: t.kind,
            main: t.main,
            y,
            height,
        });
        y += height + ROW_GAP;
    }
    out
}

/// Where something dragged to height `y` would land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropRow {
    Track(usize),
    /// A new track inserted at this index in `project.tracks`.
    New {
        kind: TrackKind,
        index: usize,
    },
}

/// Picks the target for an item of `kind` dropped at `y`. A row of the
/// wrong kind, or space above/below the rows, means a new track nearby.
pub fn drop_row(project: &Project, rows: &[Row], y: f32, kind: TrackKind) -> DropRow {
    let hit = rows
        .iter()
        .find(|r| y >= r.y && y < r.y + r.height + ROW_GAP);
    if let Some(r) = hit
        && compatible(r.kind, kind)
    {
        return DropRow::Track(r.track);
    }
    let visual_kind = kind != TrackKind::Audio;
    if visual_kind {
        // Above the topmost visual row (or over an incompatible row): new
        // track on top of the visual stack, i.e. after the last visual track.
        let above = rows.first().is_none_or(|r| y < r.y) || hit.is_some();
        let last_visual = project
            .tracks
            .iter()
            .rposition(|t| t.kind != TrackKind::Audio);
        let index = match (above, last_visual) {
            (_, Some(i)) => i + 1,
            (_, None) => 0,
        };
        DropRow::New {
            kind: TrackKind::Video,
            index,
        }
    } else {
        DropRow::New {
            kind: TrackKind::Audio,
            index: project.tracks.len(),
        }
    }
}

pub fn compatible(track: TrackKind, item: TrackKind) -> bool {
    match item {
        TrackKind::Audio => track == TrackKind::Audio,
        _ => matches!(track, TrackKind::Video | TrackKind::Overlay),
    }
}

/// Edit points: item starts and ends on every track, markers, and zero.
pub fn edit_points(project: &Project) -> Vec<Time> {
    let mut v: Vec<Time> = std::iter::once(Time::ZERO)
        .chain(project.items().flat_map(|i| [i.start, i.end()]))
        .chain(project.markers.iter().map(|m| m.at))
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Snaps `t` to the nearest candidate within `threshold`.
pub fn snap(t: Time, candidates: &[Time], threshold: Time) -> Time {
    candidates
        .iter()
        .copied()
        .filter(|c| (*c - t).ticks().abs() <= threshold.ticks())
        .min_by_key(|c| (*c - t).ticks().abs())
        .unwrap_or(t)
}

/// Snaps a moving span `[start, start + len)` by whichever edge lands
/// closest to a candidate. Returns the adjusted start.
pub fn snap_span(start: Time, len: Time, candidates: &[Time], threshold: Time) -> Time {
    let a = snap(start, candidates, threshold);
    let b = snap(start + len, candidates, threshold) - len;
    let da = (a - start).ticks().abs();
    let db = (b - start).ticks().abs();
    match (a != start, b != start) {
        (true, true) if db < da => b,
        (true, _) => a,
        (false, true) => b,
        _ => start,
    }
}

/// Selection change from a click on `item`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickMode {
    Replace,
    Toggle,
    Range,
}

pub fn click_select(
    project: &Project,
    selection: &mut BTreeSet<ItemId>,
    anchor: &mut Option<ItemId>,
    item: ItemId,
    mode: ClickMode,
) {
    match mode {
        ClickMode::Replace => {
            selection.clear();
            selection.insert(item);
            *anchor = Some(item);
        }
        ClickMode::Toggle => {
            if !selection.remove(&item) {
                selection.insert(item);
            }
            *anchor = Some(item);
        }
        ClickMode::Range => {
            let Some(a) = anchor
                .and_then(|a| project.find_item(a))
                .map(|(_, i)| i.clone())
            else {
                return click_select(project, selection, anchor, item, ClickMode::Replace);
            };
            let Some((_, b)) = project.find_item(item) else {
                return;
            };
            let (from, to) = (a.start.min(b.start), a.end().max(b.end()));
            selection.clear();
            selection.extend(
                project
                    .items()
                    .filter(|i| i.start < to && i.end() > from)
                    .map(|i| i.id),
            );
        }
    }
}

/// Items overlapping a time range on the given rows (marquee selection).
pub fn marquee(
    project: &Project,
    rows: &[Row],
    from: Time,
    to: Time,
    y0: f32,
    y1: f32,
) -> Vec<ItemId> {
    let (from, to) = (from.min(to), from.max(to));
    let (y0, y1) = (y0.min(y1), y0.max(y1));
    rows.iter()
        .filter(|r| r.y < y1 && r.y + r.height > y0)
        .flat_map(|r| &project.tracks[r.track].items)
        .filter(|i| i.start < to && i.end() > from)
        .map(|i| i.id)
        .collect()
}

/// Nearest edit point strictly before / after `t`.
pub fn prev_edit(project: &Project, t: Time) -> Time {
    edit_points(project)
        .into_iter()
        .filter(|p| *p < t)
        .max()
        .unwrap_or(Time::ZERO)
}

pub fn next_edit(project: &Project, t: Time) -> Time {
    edit_points(project)
        .into_iter()
        .find(|p| *p > t)
        .unwrap_or(t)
}

/// Gaps on a free track: `(start, end)` of empty stretches between items.
pub fn gaps(items: &[Item]) -> Vec<(Time, Time)> {
    let mut sorted: Vec<&Item> = items.iter().collect();
    sorted.sort_by_key(|i| i.start);
    let mut out = Vec::new();
    let mut end = Time::ZERO;
    for i in sorted {
        if i.start > end {
            out.push((end, i.start));
        }
        end = end.max(i.end());
    }
    out
}

/// A cut between two back-to-back clips on a picture track, where a
/// transition can go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cut {
    /// Index into `project.tracks`.
    pub track: usize,
    pub a: ItemId,
    pub b: ItemId,
    pub at: Time,
    /// The transition already on this cut.
    pub transition: Option<TransitionId>,
}

/// Every cut on unlocked picture tracks.
pub fn cuts(project: &Project) -> Vec<Cut> {
    let mut out = Vec::new();
    for (t, track) in project.tracks.iter().enumerate() {
        if track.locked || !matches!(track.kind, TrackKind::Video | TrackKind::Overlay) {
            continue;
        }
        for a in &track.items {
            if let Some(b) = track.items.iter().find(|b| b.start == a.end()) {
                out.push(Cut {
                    track: t,
                    a: a.id,
                    b: b.id,
                    at: b.start,
                    transition: track
                        .transitions
                        .iter()
                        .find(|tr| tr.a == a.id && tr.b == b.id)
                        .map(|tr| tr.id),
                });
            }
        }
    }
    out
}

/// The cut on `track` nearest to `t`, if within `threshold`.
pub fn nearest_cut(cuts: &[Cut], track: usize, t: Time, threshold: Time) -> Option<Cut> {
    cuts.iter()
        .filter(|c| c.track == track && (c.at - t).ticks().abs() <= threshold.ticks())
        .min_by_key(|c| (c.at - t).ticks().abs())
        .copied()
}

/// The cut leaving one of `selected` clips (for "add transition" without a
/// drag): the first selected clip's outgoing cut, else its incoming one.
pub fn cut_for_selection(cuts: &[Cut], selected: &BTreeSet<ItemId>) -> Option<Cut> {
    cuts.iter()
        .find(|c| selected.contains(&c.a))
        .or_else(|| cuts.iter().find(|c| selected.contains(&c.b)))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rw_model::{Canvas, ContentHash, MediaId, MediaInfo, MediaKind, MediaRef, Track};

    const S: fn(i64) -> Time = Time::from_secs;

    fn media() -> MediaRef {
        MediaRef {
            id: MediaId::new(),
            name: "m".into(),
            path: "m".into(),
            hash: ContentHash("m".into()),
            size_bytes: 0,
            info: MediaInfo {
                kind: MediaKind::Video,
                container: "mp4".into(),
                duration: S(10),
                video: None,
                audio: None,
            },
            imported_at: 0,
        }
    }

    fn item(m: &MediaRef, start: i64, dur: i64) -> Item {
        let mut i = Item::new(m, S(start));
        i.duration = S(dur);
        i
    }

    fn project() -> Project {
        let m = media();
        let mut p = Project::new("p", Canvas::default());
        let mut main = Track::new_main();
        main.items = vec![item(&m, 0, 4), item(&m, 4, 3)];
        let mut over = Track::new(TrackKind::Video);
        over.items = vec![item(&m, 1, 2), item(&m, 6, 1)];
        let mut audio = Track::new(TrackKind::Audio);
        audio.items = vec![item(&m, 2, 5)];
        p.media.push(m);
        p.tracks = vec![main, over, audio];
        p
    }

    #[test]
    fn cuts_on_picture_tracks_only() {
        let mut p = project();
        let all = cuts(&p);
        // Main track 4 s; overlay clips aren't adjacent; audio is skipped.
        assert_eq!(all.len(), 1);
        assert_eq!((all[0].track, all[0].at), (0, S(4)));
        assert_eq!(
            nearest_cut(
                &all,
                0,
                S(4) + Time::from_millis(80),
                Time::from_millis(100)
            ),
            Some(all[0])
        );
        assert_eq!(nearest_cut(&all, 0, S(5), Time::from_millis(100)), None);
        assert_eq!(nearest_cut(&all, 1, S(4), S(1)), None);
        let sel: BTreeSet<ItemId> = [p.tracks[0].items[1].id].into_iter().collect();
        assert_eq!(cut_for_selection(&all, &sel), Some(all[0]));
        p.tracks[0].locked = true;
        assert!(cuts(&p).is_empty());
    }

    #[test]
    fn view_maps_and_zooms_about_anchor() {
        let mut v = View {
            px_per_sec: 100.0,
            scroll: S(2),
        };
        assert_eq!(v.x_of(S(3)), 100.0);
        assert_eq!(v.time_at(250.0), Time::from_millis(4500));
        let before = v.time_at(300.0);
        v.zoom_about(2.0, 300.0);
        assert_eq!(v.px_per_sec, 200.0);
        assert!((v.time_at(300.0) - before).ticks().abs() < 1000);
        v.fit(S(20), 1050.0);
        assert!((v.px_per_sec - 50.0).abs() < 0.01);
    }

    #[test]
    fn ticks_grow_with_zoom_out() {
        let fps = FrameRate::FPS_30;
        let near = View {
            px_per_sec: 2000.0,
            scroll: Time::ZERO,
        };
        let far = View {
            px_per_sec: 5.0,
            scroll: Time::ZERO,
        };
        assert!(near.tick_step(fps, 60.0) < S(1));
        assert!(far.tick_step(fps, 60.0) >= S(10));
    }

    #[test]
    fn rows_put_overlays_on_top_and_audio_below() {
        let p = project();
        let r = rows(&p);
        let kinds: Vec<_> = r.iter().map(|r| (r.track, r.main)).collect();
        assert_eq!(kinds, [(1, false), (0, true), (2, false)]);
        assert!(r[0].y < r[1].y && r[1].y < r[2].y);
    }

    #[test]
    fn drop_targets() {
        let p = project();
        let r = rows(&p);
        // On the overlay row with a video clip: that track.
        assert_eq!(
            drop_row(&p, &r, r[0].y + 5.0, TrackKind::Video),
            DropRow::Track(1)
        );
        // In the strip above the tracks: new track on top of the visual stack.
        assert_eq!(
            drop_row(&p, &r, NEW_TRACK_ZONE / 2.0, TrackKind::Video),
            DropRow::New {
                kind: TrackKind::Video,
                index: 2
            }
        );
        assert_eq!(
            drop_row(&p, &r, -20.0, TrackKind::Video),
            DropRow::New {
                kind: TrackKind::Video,
                index: 2
            }
        );
        // Audio onto a video row: new audio track.
        assert_eq!(
            drop_row(&p, &r, r[1].y + 5.0, TrackKind::Audio),
            DropRow::New {
                kind: TrackKind::Audio,
                index: 3
            }
        );
        // Audio onto the audio row.
        assert_eq!(
            drop_row(&p, &r, r[2].y + 5.0, TrackKind::Audio),
            DropRow::Track(2)
        );
    }

    #[test]
    fn snapping() {
        let pts = [S(0), S(4), S(7)];
        let thr = Time::from_millis(200);
        assert_eq!(snap(Time::from_millis(3900), &pts, thr), S(4));
        assert_eq!(
            snap(Time::from_millis(3700), &pts, thr),
            Time::from_millis(3700)
        );
        // A 2 s span whose end lands near 7 s snaps by its end.
        assert_eq!(snap_span(Time::from_millis(4950), S(2), &pts, thr), S(5));
        assert_eq!(
            edit_points(&project()),
            [S(0), S(1), S(2), S(3), S(4), S(6), S(7)]
        );
    }

    #[test]
    fn selection_modes() {
        let p = project();
        let ids: Vec<ItemId> = p.items().map(|i| i.id).collect();
        let (mut sel, mut anchor) = (BTreeSet::new(), None);
        click_select(&p, &mut sel, &mut anchor, ids[0], ClickMode::Replace);
        click_select(&p, &mut sel, &mut anchor, ids[3], ClickMode::Toggle);
        assert_eq!(sel.len(), 2);
        click_select(&p, &mut sel, &mut anchor, ids[3], ClickMode::Toggle);
        assert_eq!(sel.len(), 1);
        // Range from main item 0 (0..4) to overlay item at 1..3: everything
        // overlapping 0..4.
        click_select(&p, &mut sel, &mut anchor, ids[0], ClickMode::Replace);
        click_select(&p, &mut sel, &mut anchor, ids[2], ClickMode::Range);
        assert_eq!(sel.len(), 3, "main 0..4, overlay 1..3, audio 2..7");

        let r = rows(&p);
        let m = marquee(&p, &r, S(5), Time::from_millis(6500), r[0].y, r[1].y + 1.0);
        assert_eq!(m.len(), 2, "overlay 6..7 and main 4..7");
    }

    #[test]
    fn edit_navigation_and_gaps() {
        let p = project();
        assert_eq!(next_edit(&p, Time::from_millis(4500)), S(6));
        assert_eq!(prev_edit(&p, S(4)), S(3));
        assert_eq!(prev_edit(&p, S(0)), S(0));
        assert_eq!(gaps(&p.tracks[1].items), [(S(0), S(1)), (S(3), S(6))]);
    }
}
