//! The editor's docked panel layout: library left, preview centre,
//! inspector right, timeline along the bottom.

use crate::actions::EditorAction;
use crate::media_bin::{self, BinAction, BinState};
use crate::session::Session;
use crate::textures::Textures;
use crate::theme::Tokens;
use crate::timeline::widget;
use crate::{format, icons, inspector, preview};
use eframe::egui::{self, CornerRadius, RichText, WidgetText};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};

/// A dockable editor panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    Library,
    Preview,
    Inspector,
    Timeline,
}

impl Panel {
    pub const ALL: [Panel; 4] = [
        Panel::Library,
        Panel::Preview,
        Panel::Inspector,
        Panel::Timeline,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Panel::Library => "Library",
            Panel::Preview => "Preview",
            Panel::Inspector => "Inspector",
            Panel::Timeline => "Timeline",
        }
    }

    /// Text shown while the panel has no content.
    pub fn placeholder(self) -> &'static str {
        match self {
            Panel::Library => media_bin::EMPTY_LABEL,
            Panel::Preview => PREVIEW_EMPTY,
            Panel::Inspector => inspector::EMPTY_LABEL,
            Panel::Timeline => widget::EMPTY_LABEL,
        }
    }
}

pub const PREVIEW_EMPTY: &str = "Add something to the timeline to see it here.";

/// Builds the default layout.
pub fn default_layout() -> DockState<Panel> {
    let mut state = DockState::new(vec![Panel::Preview]);
    let tree = state.main_surface_mut();
    let [top, _timeline] = tree.split_below(NodeIndex::root(), 0.58, vec![Panel::Timeline]);
    let [centre, _library] = tree.split_left(top, 0.28, vec![Panel::Library]);
    let [_preview, _inspector] = tree.split_right(centre, 0.72, vec![Panel::Inspector]);
    state
}

struct Viewer<'a> {
    tokens: Tokens,
    session: &'a mut Session,
    bin: &'a mut BinState,
    textures: &'a mut Textures,
    bin_action: Option<BinAction>,
    actions: Vec<EditorAction>,
}

/// Label of the transport button that enters full-screen preview.
pub const FULL_SCREEN_LABEL: &str = "Full screen (F)";
/// Label of the same button while in full screen.
pub const LEAVE_FULL_SCREEN_LABEL: &str = "Leave full screen (Esc)";

/// The preview alone, filling the window (full-screen preview).
pub fn full_screen_preview(
    ui: &mut egui::Ui,
    s: &mut Session,
    tokens: Tokens,
) -> Vec<EditorAction> {
    let mut out = Vec::new();
    egui::Frame::new()
        .fill(tokens.preview_backdrop)
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            preview_panel(ui, s, tokens, &mut out, true);
        });
    out
}

fn preview_panel(
    ui: &mut egui::Ui,
    s: &mut Session,
    tokens: Tokens,
    out: &mut Vec<EditorAction>,
    full_screen: bool,
) {
    const CONTROLS: f32 = 34.0;
    let full = ui.available_rect_before_wrap();
    let area = egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, full.max.y - CONTROLS));
    ui.painter().rect_filled(area, 0.0, tokens.preview_backdrop);
    let canvas = s.project.canvas;
    let size = preview::target_size(
        area.size() - egui::vec2(16.0, 16.0),
        ui.ctx().pixels_per_point(),
        canvas,
    );
    s.preview_size = size;
    let playing = s.player.is_playing();
    let playhead = s.playhead();
    // Where the canvas sits on screen (with a small margin).
    let (_, _, w, h) = rw_render::fit_contain(
        (canvas.width, canvas.height),
        (area.width() as u32, area.height() as u32),
    );
    let img = egui::Rect::from_center_size(area.center(), egui::vec2(w, h) * 0.97);
    // Taken out while rendering so the session can be read alongside.
    let mut preview = s.preview.take();
    if let Some(pv) = preview.as_mut() {
        if playing {
            if let Some(frame) = s.player.take_frame() {
                pv.show_layers(&s.project, &frame.layers, size);
            }
            ui.ctx().request_repaint();
        } else {
            let shown = s.preview_project();
            pv.show_paused(&shown, playhead, s.revision, size);
        }
        if let Some(tex) = pv.texture() {
            ui.painter().image(
                tex,
                img,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }
    s.preview = preview;
    // Full screen is for watching: no editing handles.
    if !full_screen {
        let mut tools = std::mem::take(&mut s.canvas_tools);
        out.extend(crate::canvas_tools::show(
            ui, area, img, s, &mut tools, tokens,
        ));
        s.canvas_tools = tools;
    }
    if s.project.items().next().is_none() {
        ui.put(
            area,
            egui::Label::new(RichText::new(PREVIEW_EMPTY).color(tokens.text_muted)),
        );
    }
    // Transport.
    let controls = egui::Rect::from_min_max(egui::pos2(full.min.x, area.max.y), full.max);
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(controls.shrink2(egui::vec2(8.0, 4.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let label = if playing { "Pause" } else { "Play" };
    if icons::button(&mut bar, label, false, |p, r, c| {
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
    let fps = canvas.frame_rate;
    crate::layout::steady_text(
        &mut bar,
        &format!(
            "{} / {}",
            format::timecode(playhead, fps),
            format::timecode(s.project.duration(), fps)
        ),
        "00:00:00:00 / 00:00:00:00",
        egui::FontId::proportional(14.0),
        tokens.text,
    );
    if playing {
        let st = s.player.stats();
        meter(&mut bar, st.level, tokens);
        if st.dropped > 0 {
            crate::layout::steady_text(
                &mut bar,
                &format!("{} dropped", st.dropped),
                "0000 dropped",
                egui::FontId::proportional(11.0),
                tokens.text_muted,
            );
        }
    }
    bar.with_layout(egui::Layout::right_to_left(egui::Align::Center), |bar| {
        let label = if full_screen {
            LEAVE_FULL_SCREEN_LABEL
        } else {
            FULL_SCREEN_LABEL
        };
        if icons::button(bar, label, false, |p, r, c| {
            icons::full_screen(p, r, full_screen, c)
        })
        .clicked()
        {
            out.push(EditorAction::ToggleFullScreen);
        }
    });
    ui.allocate_rect(full, egui::Sense::hover());
}

/// Stereo level meter with a peak marker that holds, then falls.
fn meter(ui: &mut egui::Ui, level: [f32; 2], tokens: Tokens) {
    const HOLD: f64 = 1.5;
    let now = ui.input(|i| i.time);
    let id = ui.make_persistent_id("master-meter");
    let mut held: [(f32, f64); 2] = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    for (h, l) in held.iter_mut().zip(level) {
        if l >= h.0 || now - h.1 > HOLD {
            *h = (l, now);
        }
    }
    ui.data_mut(|d| d.insert_temp(id, held));
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(90.0, 14.0), egui::Sense::hover());
    let db = |v: f32| {
        if v <= 1e-4 {
            -60.0
        } else {
            (20.0 * v.log10()).clamp(-60.0, 0.0)
        }
    };
    let frac = |v: f32| (db(v) + 60.0) / 60.0;
    for (ch, (l, (peak, _))) in level.iter().zip(held).enumerate() {
        let row = egui::Rect::from_min_size(
            rect.min + egui::vec2(0.0, ch as f32 * 7.0),
            egui::vec2(rect.width(), 6.0),
        );
        ui.painter().rect_filled(row, 1.0, tokens.surface_raised);
        let color = if db(*l) > -1.0 {
            tokens.danger
        } else if db(*l) > -9.0 {
            tokens.highlight
        } else {
            tokens.accent
        };
        let fill =
            egui::Rect::from_min_size(row.min, egui::vec2(row.width() * frac(*l), row.height()));
        ui.painter().rect_filled(fill, 1.0, color);
        let px = row.left() + row.width() * frac(peak);
        ui.painter().line_segment(
            [egui::pos2(px, row.top()), egui::pos2(px, row.bottom())],
            egui::Stroke::new(2.0, tokens.text),
        );
    }
    resp.on_hover_text(format!(
        "Output level: L {:.1} dB, R {:.1} dB",
        db(level[0]),
        db(level[1])
    ));
}

impl TabViewer for Viewer<'_> {
    type Tab = Panel;

    fn title(&mut self, tab: &mut Panel) -> WidgetText {
        tab.title().into()
    }

    fn id(&mut self, tab: &mut Panel) -> egui::Id {
        egui::Id::new(("dock-panel", *tab))
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Panel) {
        match tab {
            Panel::Library => {
                if let Some(a) =
                    media_bin::show(ui, self.session, self.bin, self.textures, self.tokens)
                {
                    self.bin_action = Some(a);
                }
            }
            Panel::Preview => {
                preview_panel(ui, self.session, self.tokens, &mut self.actions, false)
            }
            Panel::Inspector => {
                let a = inspector::show(ui, &mut *self.session, self.tokens);
                self.actions.extend(a);
            }
            Panel::Timeline => {
                let s = &mut *self.session;
                let playhead = s.playhead();
                let playing = s.player.is_playing();
                let a = widget::show(
                    ui,
                    &mut s.timeline,
                    widget::Ctx {
                        project: &s.project,
                        playhead,
                        selection: &mut s.selection,
                        textures: self.textures,
                        peaks: &mut s.peaks_cache,
                        strips: &mut s.strips,
                        cache: &s.cache,
                        tokens: self.tokens,
                        playing,
                    },
                );
                self.actions.extend(a);
            }
        }
    }

    fn is_closeable(&self, _tab: &Panel) -> bool {
        false
    }
}

/// What the panels asked for this frame.
#[derive(Default)]
pub struct Output {
    pub bin: Option<BinAction>,
    pub actions: Vec<EditorAction>,
}

/// Shows the docked editor layout filling `ui`.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut DockState<Panel>,
    session: &mut Session,
    bin: &mut BinState,
    textures: &mut Textures,
    tokens: Tokens,
) -> Output {
    let mut style = egui_dock::Style::from_egui(ui.style().as_ref());
    style.tab_bar.corner_radius = CornerRadius::same(tokens.radius);
    let mut viewer = Viewer {
        tokens,
        session,
        bin,
        textures,
        bin_action: None,
        actions: Vec::new(),
    };
    DockArea::new(state)
        .style(style)
        .show_close_buttons(false)
        .show_leaf_close_all_buttons(false)
        .show_leaf_collapse_buttons(false)
        .show_inside(ui, &mut viewer);
    Output {
        bin: viewer.bin_action,
        actions: viewer.actions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_has_each_panel_once() {
        let state = default_layout();
        for panel in Panel::ALL {
            let count = state.iter_all_tabs().filter(|(_, t)| **t == panel).count();
            assert_eq!(count, 1, "{panel:?}");
        }
    }
}
