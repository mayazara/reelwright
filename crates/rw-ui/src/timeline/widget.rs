//! The timeline widget: ruler, track headers, clips and all pointer
//! interaction. Edits come out as [`EditorAction`]s.

use super::logic::{self, ClickMode, DropRow, Row, View};
use crate::actions::EditorAction;
use crate::icons;
use crate::textures::Textures;
use crate::theme::Tokens;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, StrokeKind,
    pos2, vec2,
};
use rw_cache::ProjectCache;
use rw_edit::timeline::{Edge, Placement, TimelineOp, TrackChange};
use rw_media::Peaks;
use rw_model::{Item, ItemId, MediaId, MediaKind, Project, Time, TrackKind, track_kind_for};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const HEADER_WIDTH: f32 = 150.0;
pub const RULER_HEIGHT: f32 = 26.0;
/// Pointer distance from a clip edge that grabs the edge for trimming.
const EDGE_GRAB: f32 = 6.0;
/// Snap distance in pixels.
const SNAP_PX: f32 = 8.0;

pub const EMPTY_LABEL: &str = "Drag media here to start editing.";

/// Waveform peaks loaded from the cache on demand.
#[derive(Default)]
pub struct PeaksCache {
    loaded: HashMap<PathBuf, Option<Arc<Peaks>>>,
}

impl PeaksCache {
    pub fn get(&mut self, path: &Path) -> Option<Arc<Peaks>> {
        self.loaded
            .entry(path.to_path_buf())
            .or_insert_with(|| Peaks::load(path).ok().map(Arc::new))
            .clone()
    }

    pub fn forget(&mut self, path: &Path) {
        self.loaded.remove(path);
    }
}

#[derive(Debug, Clone)]
enum Drag {
    Playhead,
    Move {
        items: Vec<ItemId>,
        anchor: Item,
        anchor_track: usize,
        /// Pointer time minus the anchor's start, at grab.
        grab: Time,
        delta: Time,
        target: Option<DropRow>,
    },
    Trim {
        item: Item,
        edge: Edge,
        ripple: bool,
        to: Time,
    },
    Slip {
        item: Item,
        grab_x: f32,
        source_in: Time,
    },
    Marquee {
        from: Pos2,
        to: Pos2,
    },
}

/// Per-editor timeline view state.
#[derive(Debug, Default)]
pub struct TimelineState {
    pub view: View,
    pub snapping: bool,
    pub anchor: Option<ItemId>,
    pub scroll_y: f32,
    drag: Option<Drag>,
    context_item: Option<ItemId>,
    /// Fit the whole project on the next frame.
    pub fit_pending: bool,
}

impl TimelineState {
    pub fn new() -> Self {
        TimelineState {
            snapping: true,
            fit_pending: true,
            ..Default::default()
        }
    }
}

pub struct Ctx<'a> {
    pub project: &'a Project,
    pub playhead: Time,
    pub selection: &'a mut BTreeSet<ItemId>,
    pub textures: &'a mut Textures,
    pub peaks: &'a mut PeaksCache,
    pub cache: &'a ProjectCache,
    pub tokens: Tokens,
    pub playing: bool,
}

fn placement(project: &Project, row: DropRow) -> Placement {
    match row {
        DropRow::Track(i) => Placement::Track(project.tracks[i].id),
        DropRow::New { kind, index } => Placement::NewTrack { kind, index },
    }
}

fn track_names(rows: &[Row]) -> HashMap<usize, String> {
    let mut names = HashMap::new();
    let (mut v, mut a) = (1, 1);
    // Number visual tracks bottom-up, audio top-down.
    for r in rows.iter().rev().filter(|r| r.kind != TrackKind::Audio) {
        let name = if r.main {
            "Main".to_owned()
        } else {
            v += 1;
            format!("Video {v}")
        };
        names.insert(r.track, name);
    }
    for r in rows.iter().filter(|r| r.kind == TrackKind::Audio) {
        names.insert(r.track, format!("Audio {a}"));
        a += 1;
    }
    names
}

/// Draws the toolbar above the tracks.
fn toolbar(
    ui: &mut egui::Ui,
    state: &mut TimelineState,
    ctx: &Ctx<'_>,
    out: &mut Vec<EditorAction>,
) {
    ui.horizontal(|ui| {
        let (play_label, playing) = if ctx.playing {
            ("Pause", true)
        } else {
            ("Play", false)
        };
        if icons::button(ui, play_label, false, |p, r, c| {
            if playing {
                icons::pause(p, r, c)
            } else {
                icons::play(p, r, c)
            }
        })
        .clicked()
        {
            out.push(EditorAction::PlayPause);
        }
        ui.label(
            egui::RichText::new(crate::format::timecode(
                ctx.playhead,
                ctx.project.canvas.frame_rate,
            ))
            .monospace()
            .color(ctx.tokens.text),
        );
        ui.separator();
        if ui
            .button("Split")
            .on_hover_text("Split at playhead (S)")
            .clicked()
        {
            out.push(EditorAction::SplitAtPlayhead);
        }
        let any = !ctx.selection.is_empty();
        if ui
            .add_enabled(any, egui::Button::new("Delete"))
            .on_hover_text("Delete (Del)")
            .clicked()
        {
            out.push(EditorAction::Delete { ripple: false });
        }
        ui.separator();
        if ui.button("−").on_hover_text("Zoom out").clicked() {
            out.push(EditorAction::ZoomOut);
        }
        let mut zoom = state.view.px_per_sec.ln();
        let slider = egui::Slider::new(&mut zoom, View::MIN_ZOOM.ln()..=View::MAX_ZOOM.ln())
            .show_value(false);
        if ui
            .add_sized([120.0, 18.0], slider)
            .on_hover_text("Zoom")
            .changed()
        {
            state.view.px_per_sec = zoom.exp();
        }
        if ui.button("+").on_hover_text("Zoom in").clicked() {
            out.push(EditorAction::ZoomIn);
        }
        if ui
            .button("Fit")
            .on_hover_text("Zoom to fit (Shift+Z)")
            .clicked()
        {
            out.push(EditorAction::ZoomFit);
        }
        let snap = state.snapping;
        if icons::button(ui, "Snapping", snap, |p, r, c| icons::magnet(p, r, snap, c)).clicked() {
            state.snapping = !state.snapping;
        }
    });
}

pub fn show(ui: &mut egui::Ui, state: &mut TimelineState, ctx: Ctx<'_>) -> Vec<EditorAction> {
    let mut out = Vec::new();
    toolbar(ui, state, &ctx, &mut out);
    ui.add_space(2.0);
    let t = ctx.tokens;
    let project = ctx.project;
    let rows = logic::rows(project);
    let names = track_names(&rows);

    let avail = ui.available_size();
    let (area, body_resp) = ui.allocate_exact_size(avail, Sense::click_and_drag());
    let headers = Rect::from_min_size(
        area.min + vec2(0.0, RULER_HEIGHT),
        vec2(HEADER_WIDTH, area.height() - RULER_HEIGHT),
    );
    let ruler = Rect::from_min_size(
        area.min + vec2(HEADER_WIDTH, 0.0),
        vec2(area.width() - HEADER_WIDTH, RULER_HEIGHT),
    );
    let tracks = Rect::from_min_max(pos2(ruler.left(), ruler.bottom()), area.max);
    let painter = ui.painter_at(area);
    painter.rect_filled(area, CornerRadius::ZERO, t.surface);

    if state.fit_pending && tracks.width() > 50.0 {
        state.view.fit(
            project.duration().max(Time::from_secs(10)),
            tracks.width() - 20.0,
        );
        state.fit_pending = false;
    }

    // Scrolling and zooming with the wheel.
    if body_resp.hovered() {
        let (scroll, zoom, cmd) =
            ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.command));
        let anchor_x = ui
            .input(|i| i.pointer.hover_pos())
            .map_or(0.0, |p| p.x - tracks.left());
        if zoom != 1.0 {
            state.view.zoom_about(zoom, anchor_x);
        } else if !cmd {
            let dx = if scroll.x != 0.0 {
                scroll.x
            } else if ui.input(|i| i.modifiers.shift) {
                scroll.y
            } else {
                0.0
            };
            if dx != 0.0 {
                state.view.scroll =
                    (state.view.scroll - state.view.duration_of(dx)).max(Time::ZERO);
            } else if scroll.y != 0.0 {
                let content = rows.last().map_or(0.0, |r| r.y + r.height);
                let max = (content - tracks.height() + 20.0).max(0.0);
                state.scroll_y = (state.scroll_y - scroll.y).clamp(0.0, max);
            }
        }
    }
    let view = state.view;
    let row_y = |r: &Row| tracks.top() + r.y - state.scroll_y;
    let row_rect = |r: &Row| {
        Rect::from_min_size(
            pos2(tracks.left(), row_y(r)),
            vec2(tracks.width(), r.height),
        )
    };
    let x_of = |time: Time| tracks.left() + view.x_of(time);
    let time_at = |x: f32| view.time_at(x - tracks.left()).max(Time::ZERO);
    let fps = project.canvas.frame_rate;

    // ---- Ruler ----
    painter.rect_filled(ruler, CornerRadius::ZERO, t.surface_raised);
    let step = view.tick_step(fps, 70.0);
    let first = Time((view.scroll.ticks() / step.ticks()) * step.ticks());
    let mut tick = first;
    while x_of(tick) < ruler.right() {
        let x = x_of(tick);
        if x >= ruler.left() {
            painter.line_segment(
                [pos2(x, ruler.bottom() - 8.0), pos2(x, ruler.bottom())],
                Stroke::new(1.0, t.text_muted),
            );
            let label = if step < Time::from_secs(1) {
                crate::format::timecode(tick, fps)
            } else {
                crate::format::duration(tick)
            };
            painter.text(
                pos2(x + 3.0, ruler.top() + 4.0),
                Align2::LEFT_TOP,
                label,
                FontId::proportional(10.0),
                t.text_muted,
            );
        }
        for k in 1..5 {
            let mx = x_of(tick + Time(step.ticks() * k / 5));
            if mx > ruler.left() && mx < ruler.right() {
                painter.line_segment(
                    [pos2(mx, ruler.bottom() - 4.0), pos2(mx, ruler.bottom())],
                    Stroke::new(1.0, t.border),
                );
            }
        }
        tick += step;
    }

    // ---- Track headers and rows ----
    let body_painter = ui.painter_at(Rect::from_min_max(
        pos2(area.left(), tracks.top()),
        area.max,
    ));
    if rows.is_empty() || project.items().next().is_none() {
        body_painter.text(
            tracks.center(),
            Align2::CENTER_CENTER,
            EMPTY_LABEL,
            FontId::proportional(14.0),
            t.text_muted,
        );
        // Keep an accessible node for tests and screen readers.
        ui.put(
            Rect::from_center_size(tracks.center() + vec2(0.0, 24.0), vec2(1.0, 1.0)),
            egui::Label::new(egui::RichText::new(EMPTY_LABEL).size(0.1)),
        );
    }
    let pointer = ui.input(|i| i.pointer.hover_pos());
    for r in &rows {
        let rr = row_rect(r);
        if rr.bottom() < tracks.top() || rr.top() > tracks.bottom() {
            continue;
        }
        let track = &project.tracks[r.track];
        let lane = if r.main {
            t.surface_hover
        } else {
            t.surface_raised
        };
        body_painter.rect_filled(rr, CornerRadius::same(3), lane.gamma_multiply(0.6));
        if track.locked {
            // Diagonal hatching marks a locked track.
            let mut x = rr.left() - rr.height();
            while x < rr.right() {
                body_painter.line_segment(
                    [pos2(x, rr.bottom()), pos2(x + rr.height(), rr.top())],
                    Stroke::new(1.0, t.border),
                );
                x += 12.0;
            }
        }
        // Header.
        let hr = Rect::from_min_size(
            pos2(headers.left(), rr.top()),
            vec2(HEADER_WIDTH - 4.0, r.height),
        );
        body_painter.rect_filled(hr, CornerRadius::same(3), t.surface_raised);
        body_painter.text(
            hr.left_top() + vec2(8.0, 6.0),
            Align2::LEFT_TOP,
            &names[&r.track],
            FontId::proportional(12.0),
            t.text,
        );
        let mut header_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    hr.left_bottom() + vec2(4.0, -26.0),
                    vec2(hr.width() - 8.0, 24.0),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let id = track.id;
        let lock_label = if track.locked {
            "Unlock track"
        } else {
            "Lock track"
        };
        if icons::button(&mut header_ui, lock_label, track.locked, |p, rc, c| {
            icons::lock(p, rc, track.locked, c)
        })
        .clicked()
        {
            out.push(EditorAction::Op(TimelineOp::SetTrack {
                track: id,
                change: TrackChange::Locked(!track.locked),
            }));
        }
        if track.kind != TrackKind::Audio {
            let label = if track.hidden {
                "Show track"
            } else {
                "Hide track"
            };
            if icons::button(&mut header_ui, label, track.hidden, |p, rc, c| {
                icons::eye(p, rc, !track.hidden, c)
            })
            .clicked()
            {
                out.push(EditorAction::Op(TimelineOp::SetTrack {
                    track: id,
                    change: TrackChange::Hidden(!track.hidden),
                }));
            }
        }
        let label = if track.muted {
            "Unmute track"
        } else {
            "Mute track"
        };
        if icons::button(&mut header_ui, label, track.muted, |p, rc, c| {
            icons::speaker(p, rc, track.muted, c)
        })
        .clicked()
        {
            out.push(EditorAction::Op(TimelineOp::SetTrack {
                track: id,
                change: TrackChange::Muted(!track.muted),
            }));
        }

        // Gaps on free tracks, with a remove button on hover.
        if !r.main && !track.locked {
            for (g0, g1) in logic::gaps(&track.items) {
                let gr = Rect::from_x_y_ranges(x_of(g0)..=x_of(g1), rr.y_range());
                if gr.width() > 28.0
                    && pointer.is_some_and(|p| gr.contains(p))
                    && state.drag.is_none()
                {
                    body_painter.rect_filled(
                        gr.shrink(2.0),
                        CornerRadius::same(3),
                        t.surface_hover,
                    );
                    let b = Rect::from_center_size(gr.center(), vec2(18.0, 18.0));
                    body_painter.circle_filled(b.center(), 9.0, t.border);
                    body_painter.text(
                        b.center(),
                        Align2::CENTER_CENTER,
                        "×",
                        FontId::proportional(13.0),
                        t.text,
                    );
                    if body_resp.clicked() && pointer.is_some_and(|p| b.contains(p)) {
                        out.push(EditorAction::Op(TimelineOp::CloseGap {
                            track: id,
                            at: Time((g0.ticks() + g1.ticks()) / 2),
                        }));
                    }
                }
            }
        }

        // Clips.
        let paint = Paint {
            tokens: t,
            project,
            cache: ctx.cache,
        };
        for item in &track.items {
            let moving = match &state.drag {
                Some(Drag::Move { items, .. }) => items.contains(&item.id),
                Some(Drag::Trim { item: it, .. }) | Some(Drag::Slip { item: it, .. }) => {
                    it.id == item.id
                }
                _ => false,
            };
            draw_item(
                &body_painter,
                &paint,
                item,
                rr,
                r.kind == TrackKind::Audio,
                x_of(item.start),
                x_of(item.end()),
                ctx.selection.contains(&item.id),
                track.hidden || moving,
                ctx.textures,
                ctx.peaks,
                ui.ctx(),
            );
        }
    }

    // ---- Interaction ----
    let mods = ui.input(|i| i.modifiers);
    let snap_on = state.snapping && !mods.command;
    let snap_threshold = view.duration_of(SNAP_PX);
    let hit_item = |p: Pos2| -> Option<(usize, &Item, Rect)> {
        rows.iter().find_map(|r| {
            let rr = row_rect(r);
            if !rr.contains(p) {
                return None;
            }
            project.tracks[r.track]
                .items
                .iter()
                .find(|i| p.x >= x_of(i.start) && p.x < x_of(i.end()))
                .map(|i| {
                    (
                        r.track,
                        i,
                        Rect::from_x_y_ranges(x_of(i.start)..=x_of(i.end()), rr.y_range()),
                    )
                })
        })
    };

    if let Some(p) = pointer
        && state.drag.is_none()
        && body_resp.hovered()
    {
        if ruler.contains(p) {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        } else if let Some((ti, _, rect)) = hit_item(p)
            && !project.tracks[ti].locked
        {
            let near_edge = rect.width() > 2.0 * EDGE_GRAB + 4.0
                && ((p.x - rect.left()).abs() < EDGE_GRAB
                    || (rect.right() - p.x).abs() < EDGE_GRAB);
            ui.ctx().set_cursor_icon(if near_edge {
                CursorIcon::ResizeHorizontal
            } else {
                CursorIcon::Grab
            });
        }
    }

    if body_resp.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        state.drag = if ruler.contains(p) {
            Some(Drag::Playhead)
        } else if let Some((ti, item, rect)) = hit_item(p) {
            if project.tracks[ti].locked {
                None
            } else if rect.width() > 2.0 * EDGE_GRAB + 4.0 && (p.x - rect.left()).abs() < EDGE_GRAB
            {
                Some(Drag::Trim {
                    item: item.clone(),
                    edge: Edge::Start,
                    ripple: mods.shift,
                    to: item.start,
                })
            } else if rect.width() > 2.0 * EDGE_GRAB + 4.0 && (rect.right() - p.x).abs() < EDGE_GRAB
            {
                Some(Drag::Trim {
                    item: item.clone(),
                    edge: Edge::End,
                    ripple: mods.shift,
                    to: item.end(),
                })
            } else if mods.alt {
                Some(Drag::Slip {
                    item: item.clone(),
                    grab_x: p.x,
                    source_in: item.source_in,
                })
            } else {
                if !ctx.selection.contains(&item.id) {
                    ctx.selection.clear();
                    ctx.selection.insert(item.id);
                    state.anchor = Some(item.id);
                }
                Some(Drag::Move {
                    items: ctx.selection.iter().copied().collect(),
                    anchor: item.clone(),
                    anchor_track: ti,
                    grab: time_at(p.x) - item.start,
                    delta: Time::ZERO,
                    target: None,
                })
            }
        } else if tracks.contains(p) {
            Some(Drag::Marquee { from: p, to: p })
        } else {
            None
        };
    }

    if let (Some(drag), Some(p)) = (state.drag.as_mut(), ui.input(|i| i.pointer.interact_pos())) {
        let mut candidates = logic::edit_points(project);
        candidates.push(ctx.playhead);
        match drag {
            Drag::Playhead => out.push(EditorAction::Seek(time_at(p.x))),
            Drag::Move {
                items,
                anchor,
                anchor_track,
                grab,
                delta,
                target,
            } => {
                candidates.retain(|c| {
                    !project
                        .items()
                        .any(|i| items.contains(&i.id) && (i.start == *c || i.end() == *c))
                });
                let mut start = (time_at(p.x) - *grab).max(Time::ZERO);
                if snap_on {
                    start = logic::snap_span(start, anchor.duration, &candidates, snap_threshold);
                }
                *delta = start - anchor.start;
                let single_track = items.iter().all(|id| {
                    project
                        .find_item(*id)
                        .is_some_and(|(t, _)| t == *anchor_track)
                });
                let kind = anchor
                    .media()
                    .and_then(|id| project.media(id))
                    .map_or(TrackKind::Video, |m| track_kind_for(m.kind()));
                let row =
                    logic::drop_row(project, &rows, p.y - tracks.top() + state.scroll_y, kind);
                *target = (single_track && row != DropRow::Track(*anchor_track)).then_some(row);
                // Ghost of the dragged clips.
                for id in items.iter() {
                    if let Some((ti, it)) = project.find_item(*id) {
                        let r = match target {
                            Some(DropRow::Track(t2)) => rows.iter().find(|r| r.track == *t2),
                            Some(DropRow::New { .. }) => None,
                            None => rows.iter().find(|r| r.track == ti),
                        };
                        let rr = match r {
                            Some(r) => row_rect(r),
                            None => Rect::from_min_size(
                                pos2(tracks.left(), p.y - 20.0),
                                vec2(tracks.width(), 40.0),
                            ),
                        };
                        let ghost = Rect::from_x_y_ranges(
                            x_of(it.start + *delta)..=x_of(it.end() + *delta),
                            rr.y_range(),
                        )
                        .shrink2(vec2(0.0, 3.0));
                        body_painter.rect(
                            ghost,
                            CornerRadius::same(4),
                            t.accent.gamma_multiply(0.45),
                            Stroke::new(2.0, t.accent),
                            StrokeKind::Inside,
                        );
                    }
                }
            }
            Drag::Trim { item, edge, to, .. } => {
                let mut tt = time_at(p.x);
                if snap_on {
                    candidates.retain(|c| *c != item.start && *c != item.end());
                    tt = logic::snap(tt, &candidates, snap_threshold);
                }
                *to = tt;
                let (a, b) = match edge {
                    Edge::Start => (tt.min(item.end()), item.end()),
                    Edge::End => (item.start, tt.max(item.start)),
                };
                if let Some(r) = rows.iter().find(|r| {
                    project.tracks[r.track]
                        .items
                        .iter()
                        .any(|i| i.id == item.id)
                }) {
                    let rr = row_rect(r);
                    let ghost = Rect::from_x_y_ranges(x_of(a)..=x_of(b), rr.y_range())
                        .shrink2(vec2(0.0, 3.0));
                    body_painter.rect(
                        ghost,
                        CornerRadius::same(4),
                        t.accent.gamma_multiply(0.45),
                        Stroke::new(2.0, t.accent),
                        StrokeKind::Inside,
                    );
                }
            }
            Drag::Slip {
                item,
                grab_x,
                source_in,
            } => {
                *source_in = (item.source_in - view.duration_of(p.x - *grab_x)).max(Time::ZERO);
                body_painter.text(
                    p + vec2(12.0, -12.0),
                    Align2::LEFT_BOTTOM,
                    format!("Source {}", crate::format::timecode(*source_in, fps)),
                    FontId::proportional(11.0),
                    t.text,
                );
            }
            Drag::Marquee { from, to } => {
                *to = p;
                let r = Rect::from_two_pos(*from, *to);
                body_painter.rect(
                    r,
                    CornerRadius::ZERO,
                    t.accent.gamma_multiply(0.15),
                    Stroke::new(1.0, t.accent),
                    StrokeKind::Inside,
                );
            }
        }
    }

    if body_resp.drag_stopped()
        && let Some(drag) = state.drag.take()
    {
        match drag {
            Drag::Playhead => {}
            Drag::Move {
                items,
                delta,
                target,
                ..
            } => {
                if delta != Time::ZERO || target.is_some() {
                    out.push(EditorAction::Op(TimelineOp::Move {
                        items,
                        delta,
                        to: target.map(|r| placement(project, r)),
                    }));
                }
            }
            Drag::Trim {
                item,
                edge,
                ripple,
                to,
            } => {
                let unchanged = match edge {
                    Edge::Start => to == item.start,
                    Edge::End => to == item.end(),
                };
                if !unchanged {
                    out.push(EditorAction::Op(TimelineOp::Trim {
                        item: item.id,
                        edge,
                        to,
                        ripple,
                    }));
                }
            }
            Drag::Slip {
                item, source_in, ..
            } => {
                if source_in != item.source_in {
                    out.push(EditorAction::Op(TimelineOp::Slip {
                        item: item.id,
                        source_in,
                    }));
                }
            }
            Drag::Marquee { from, to } => {
                let ids = logic::marquee(
                    project,
                    &rows,
                    time_at(from.x),
                    time_at(to.x),
                    from.y - tracks.top() + state.scroll_y,
                    to.y - tracks.top() + state.scroll_y,
                );
                if !(mods.shift || mods.command) {
                    ctx.selection.clear();
                }
                ctx.selection.extend(ids);
            }
        }
    }

    if body_resp.clicked()
        && let Some(p) = body_resp.interact_pointer_pos()
    {
        if ruler.contains(p) {
            out.push(EditorAction::Seek(time_at(p.x)));
        } else if let Some((_, item, _)) = hit_item(p) {
            let mode = if mods.shift {
                ClickMode::Range
            } else if mods.command {
                ClickMode::Toggle
            } else {
                ClickMode::Replace
            };
            logic::click_select(project, ctx.selection, &mut state.anchor, item.id, mode);
        } else if tracks.contains(p)
            && !out
                .iter()
                .any(|a| matches!(a, EditorAction::Op(TimelineOp::CloseGap { .. })))
        {
            ctx.selection.clear();
            out.push(EditorAction::Seek(time_at(p.x)));
        }
    }

    if body_resp.secondary_clicked()
        && let Some(p) = body_resp.interact_pointer_pos()
    {
        state.context_item = hit_item(p).map(|(_, i, _)| i.id);
        if let Some(id) = state.context_item
            && !ctx.selection.contains(&id)
        {
            ctx.selection.clear();
            ctx.selection.insert(id);
        }
    }
    if let Some(id) = state.context_item {
        body_resp.context_menu(|ui| {
            item_menu(ui, project, id, &mut out);
        });
    }

    // Media dragged from the bin.
    if let Some(p) = pointer.filter(|p| tracks.contains(*p) || ruler.contains(*p)) {
        if let Some(media) = body_resp.dnd_hover_payload::<MediaId>()
            && let Some(m) = project.media(*media)
        {
            let at = drop_time(project, time_at(p.x), snap_on, snap_threshold, ctx.playhead);
            let ghost = Rect::from_min_size(
                pos2(x_of(at), p.y - 18.0),
                vec2(
                    view.x_of(view.scroll + rw_model::default_duration(m))
                        .max(8.0),
                    36.0,
                ),
            );
            body_painter.rect(
                ghost,
                CornerRadius::same(4),
                t.accent.gamma_multiply(0.4),
                Stroke::new(2.0, t.accent),
                StrokeKind::Inside,
            );
        }
        if let Some(media) = body_resp.dnd_release_payload::<MediaId>()
            && let Some(m) = project.media(*media)
        {
            let kind = track_kind_for(m.kind());
            let row = logic::drop_row(project, &rows, p.y - tracks.top() + state.scroll_y, kind);
            let at = drop_time(project, time_at(p.x), snap_on, snap_threshold, ctx.playhead);
            out.push(EditorAction::InsertMedia {
                media: *media,
                row,
                at,
            });
        }
    }

    // Playhead on top of everything.
    let px = x_of(ctx.playhead);
    if px >= tracks.left() && px <= tracks.right() {
        painter.line_segment(
            [pos2(px, ruler.top()), pos2(px, area.bottom())],
            Stroke::new(2.0, t.accent),
        );
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(px - 6.0, ruler.top()),
                pos2(px + 6.0, ruler.top()),
                pos2(px, ruler.top() + 9.0),
            ],
            t.accent,
            Stroke::NONE,
        ));
    }
    out
}

fn drop_time(project: &Project, t: Time, snap_on: bool, threshold: Time, playhead: Time) -> Time {
    if !snap_on {
        return t;
    }
    let mut c = logic::edit_points(project);
    c.push(playhead);
    logic::snap(t, &c, threshold)
}

fn item_menu(ui: &mut egui::Ui, project: &Project, id: ItemId, out: &mut Vec<EditorAction>) {
    let Some((_, item)) = project.find_item(id) else {
        return;
    };
    let media = item.media().and_then(|m| project.media(m));
    if ui.button("Split at playhead").clicked() {
        out.push(EditorAction::SplitAtPlayhead);
        ui.close();
    }
    if ui.button("Duplicate").clicked() {
        out.push(EditorAction::Duplicate);
        ui.close();
    }
    if ui.button("Copy").clicked() {
        out.push(EditorAction::Copy);
        ui.close();
    }
    ui.separator();
    let has_sound = media.is_some_and(|m| m.kind() == MediaKind::Video && m.info.audio.is_some())
        && !item.audio_detached;
    if ui
        .add_enabled(has_sound, egui::Button::new("Detach audio"))
        .clicked()
    {
        out.push(EditorAction::DetachAudio);
        ui.close();
    }
    let label = if item.muted { "Unmute" } else { "Mute" };
    if ui.button(label).clicked() {
        out.push(EditorAction::Op(TimelineOp::SetMuted {
            items: vec![id],
            muted: !item.muted,
        }));
        ui.close();
    }
    if ui
        .button(if item.group.is_some() {
            "Ungroup"
        } else {
            "Group"
        })
        .clicked()
    {
        out.push(if item.group.is_some() {
            EditorAction::Ungroup
        } else {
            EditorAction::Group
        });
        ui.close();
    }
    ui.separator();
    if ui.button("Delete").clicked() {
        out.push(EditorAction::Delete { ripple: false });
        ui.close();
    }
    if ui.button("Ripple delete").clicked() {
        out.push(EditorAction::Delete { ripple: true });
        ui.close();
    }
}

struct Paint<'a> {
    tokens: Tokens,
    project: &'a Project,
    cache: &'a ProjectCache,
}

#[allow(clippy::too_many_arguments)]
fn draw_item(
    p: &egui::Painter,
    paint: &Paint<'_>,
    item: &Item,
    row: Rect,
    audio_row: bool,
    x0: f32,
    x1: f32,
    selected: bool,
    dimmed: bool,
    textures: &mut Textures,
    peaks: &mut PeaksCache,
    egui_ctx: &egui::Context,
) {
    let t = paint.tokens;
    let r = Rect::from_x_y_ranges(x0..=x1.max(x0 + 2.0), row.y_range()).shrink2(vec2(0.0, 3.0));
    if r.right() < row.left() || r.left() > row.right() {
        return;
    }
    let media = item.media().and_then(|id| paint.project.media(id));
    let base = if audio_row { t.highlight } else { t.accent };
    let fill = base.gamma_multiply(if dimmed { 0.18 } else { 0.38 });
    let clip = p.with_clip_rect(r.intersect(row));
    clip.rect_filled(r, CornerRadius::same(4), fill);
    let inner = r.shrink(2.0);
    let visible = clip.clip_rect();

    // Thumbnails along visual clips.
    if !audio_row
        && let Some(m) = media.filter(|m| m.kind() != MediaKind::Audio)
        && let Some(tex) = textures.get(egui_ctx, &paint.cache.thumbnail(&m.hash))
    {
        let size = tex.size_vec2();
        let h = inner.height() - 14.0;
        let w = (h * size.x / size.y).max(8.0);
        let mut x = inner.left();
        while x < inner.right() {
            let tile = Rect::from_min_size(pos2(x, inner.top() + 14.0), vec2(w, h));
            if tile.right() > visible.left() && tile.left() < visible.right() {
                clip.image(
                    tex.id(),
                    tile.intersect(inner),
                    uv_for(tile, inner),
                    Color32::from_white_alpha(if dimmed { 90 } else { 220 }),
                );
            }
            x += w;
        }
    }

    // Waveform: full height on audio rows, a thin strip on video clips.
    if let Some(m) =
        media.filter(|m| m.info.audio.is_some() && !(item.audio_detached && !audio_row))
        && let Some(pk) = peaks.get(&paint.cache.peaks(&m.hash))
    {
        let mid = if audio_row {
            inner.center().y + 6.0
        } else {
            inner.bottom() - 6.0
        };
        let amp = if audio_row {
            (inner.height() - 16.0) / 2.0
        } else {
            5.0
        };
        let secs_per_px = item.duration.as_secs_f64() / f64::from(r.width().max(1.0));
        let color = base.gamma_multiply(if item.muted { 0.3 } else { 0.95 });
        let mut x = inner.left().max(visible.left());
        while x < inner.right().min(visible.right()) {
            let t0 = item.source_in + Time::from_secs_f64(f64::from(x - r.left()) * secs_per_px);
            let t1 = t0 + Time::from_secs_f64(secs_per_px * 2.0);
            let h = (pk.max_in(t0, t1) * item.gain.min(1.5) * amp).max(0.5);
            clip.line_segment(
                [pos2(x, mid - h), pos2(x, mid + h)],
                Stroke::new(1.0, color),
            );
            x += 2.0;
        }
    }

    let stroke = if selected {
        Stroke::new(2.0, t.text)
    } else {
        Stroke::new(1.0, base.gamma_multiply(0.8))
    };
    clip.rect_stroke(r, CornerRadius::same(4), stroke, StrokeKind::Inside);
    let mut label = media.map_or_else(|| "Missing media".to_owned(), |m| m.name.clone());
    if item.muted {
        label.push_str("  (muted)");
    }
    let mut text_x = r.left() + 6.0;
    if item.group.is_some() {
        // A small filled dot marks grouped clips.
        clip.circle_filled(pos2(text_x + 3.0, r.top() + 9.0), 3.5, t.text);
        text_x += 10.0;
    }
    clip.text(
        pos2(text_x, r.top() + 3.0),
        Align2::LEFT_TOP,
        label,
        FontId::proportional(11.0),
        t.text,
    );
}

/// UVs for drawing `tile` clipped to `within`.
fn uv_for(tile: Rect, within: Rect) -> Rect {
    let shown = tile.intersect(within);
    let u0 = (shown.left() - tile.left()) / tile.width();
    let u1 = (shown.right() - tile.left()) / tile.width();
    let v0 = (shown.top() - tile.top()) / tile.height();
    let v1 = (shown.bottom() - tile.top()) / tile.height();
    Rect::from_min_max(pos2(u0, v0), pos2(u1, v1))
}
