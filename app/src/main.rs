//! Application entry point: sets up local logging and starts the UI.

// No console window behind the app on Windows (release builds).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adapter;
mod self_test;

use anyhow::Context as _;
use rw_model::app;
use std::path::PathBuf;

fn dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from(app::QUALIFIER, app::ORGANIZATION, app::ID)
}

fn log_dir() -> Option<PathBuf> {
    dirs().map(|d| d.data_local_dir().join("logs"))
}

/// Projects live in the user's Videos folder so they are easy to find and
/// back up; the app data folder is the fallback.
fn config() -> anyhow::Result<rw_ui::Config> {
    let dirs = dirs().context("could not determine the home directory")?;
    let projects_dir = directories::UserDirs::new()
        .and_then(|u| u.video_dir().map(|v| v.join(app::NAME)))
        .unwrap_or_else(|| dirs.data_dir().join("Projects"));
    Ok(rw_ui::Config {
        projects_dir,
        settings_file: dirs.config_dir().join("settings.json"),
    })
}

/// Logs go to a local file only; nothing ever leaves the machine.
fn init_logging() -> Option<tracing_appender_guard::Guard> {
    let dir = log_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{}.log", app::ID)))
        .ok()?;
    let (writer, guard) = tracing_appender_guard::wrap(file);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_ansi(false)
        .with_writer(writer)
        .init();
    Some(guard)
}

mod tracing_appender_guard {
    use std::fs::File;
    use std::sync::{Arc, Mutex};

    /// Keeps the log file alive for the lifetime of the app.
    pub struct Guard(#[allow(dead_code)] Arc<Mutex<File>>);

    #[derive(Clone)]
    pub struct Writer(Arc<Mutex<File>>);

    impl std::io::Write for Writer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .map_err(|_| std::io::Error::other("poisoned"))?
                .write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0
                .lock()
                .map_err(|_| std::io::Error::other("poisoned"))?
                .flush()
        }
    }

    pub fn wrap(file: File) -> (impl Fn() -> Writer + Send + Sync + 'static, Guard) {
        let shared = Arc::new(Mutex::new(file));
        let w = Writer(shared.clone());
        (move || w.clone(), Guard(shared))
    }
}

const ICON_PNG: &[u8] = include_bytes!("../../assets/branding/icon-256.png");

fn window_icon() -> Option<eframe::egui::IconData> {
    let img = image::load_from_memory(ICON_PNG).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(eframe::egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

/// Tells the user something went wrong when there's no console to print to.
fn show_error(message: &str) {
    let log = log_dir()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(app::NAME)
        .set_description(format!(
            "{} couldn't start.\n\n{message}\n\nDetails are in the log folder:\n{log}",
            app::NAME
        ))
        .show();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--self-test") {
        let report = args
            .get(i + 1)
            .map_or_else(|| PathBuf::from("self-test.txt"), PathBuf::from);
        std::process::exit(if self_test::run(&report) { 0 } else { 1 });
    }
    if let Err(e) = run() {
        tracing::error!(error = %format!("{e:#}"), "startup failed");
        show_error(&format!("{e:#}"));
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let _log_guard = init_logging();
    tracing::info!(version = app::VERSION, "starting {}", app::NAME);

    let config = config()?;
    tracing::info!(projects = %config.projects_dir.display(), "using projects folder");
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(app::NAME)
        .with_app_id(app::ID)
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([960.0, 600.0]);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: adapter::wgpu_options(),
        ..Default::default()
    };
    eframe::run_native(
        app::NAME,
        options,
        Box::new(move |cc| {
            Ok(Box::new(rw_ui::App::with_audio(
                cc,
                config,
                std::sync::Arc::new(rw_audio::device::DeviceFactory),
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
    .context("UI exited with an error")
}
