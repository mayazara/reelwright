//! egui app: panels, timeline widget, preview, property inspector, dialogs.

pub mod home;
pub mod theme;
pub mod workspace;

use eframe::egui;
use egui_dock::DockState;
use theme::{ThemeMode, Tokens};
use workspace::Panel;

/// Which top-level screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Editor,
}

/// All UI state. Kept separate from [`eframe::App`] so tests can drive it
/// through `egui_kittest` without a window.
pub struct EditorUi {
    pub screen: Screen,
    pub theme: ThemeMode,
    theme_applied: Option<ThemeMode>,
    dock: DockState<Panel>,
}

impl Default for EditorUi {
    fn default() -> Self {
        Self {
            screen: Screen::Home,
            theme: ThemeMode::default(),
            theme_applied: None,
            dock: workspace::default_layout(),
        }
    }
}

impl EditorUi {
    /// Installs fonts. Call once per context before the first frame.
    pub fn install(ctx: &egui::Context) {
        ctx.set_fonts(theme::fonts());
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if self.theme_applied != Some(self.theme) {
            theme::apply(ui.ctx(), self.theme);
            self.theme_applied = Some(self.theme);
        }
        let tokens = Tokens::for_mode(self.theme);
        match self.screen {
            Screen::Home => {
                egui::Frame::central_panel(ui.style()).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    match home::show(ui, tokens) {
                        Some(home::HomeAction::NewProject) => self.screen = Screen::Editor,
                        Some(home::HomeAction::ToggleTheme) => self.theme = self.theme.toggled(),
                        None => {}
                    }
                });
            }
            Screen::Editor => {
                egui::Frame::new()
                    .fill(tokens.surface)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Home").clicked() {
                                self.screen = Screen::Home;
                            }
                            ui.label(rw_model::app::NAME);
                        });
                    });
                workspace::show(ui, &mut self.dock, tokens);
            }
        }
    }
}

/// The eframe application.
pub struct App {
    ui: EditorUi,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        EditorUi::install(&cc.egui_ctx);
        Self {
            ui: EditorUi::default(),
        }
    }
}

impl App {
    pub fn editor(&self) -> &EditorUi {
        &self.ui
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui.ui(ui);
    }
}
