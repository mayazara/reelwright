//! Application entry point: sets up local logging and starts the UI.

use anyhow::Context as _;
use rw_model::app;
use std::path::PathBuf;

fn log_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from(app::QUALIFIER, app::ORGANIZATION, app::ID)
        .map(|dirs| dirs.data_local_dir().join("logs"))
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

fn main() -> anyhow::Result<()> {
    let _log_guard = init_logging();
    tracing::info!(version = app::VERSION, "starting {}", app::NAME);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(app::NAME)
            .with_app_id(app::ID)
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        app::NAME,
        options,
        Box::new(|cc| Ok(Box::new(rw_ui::App::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
    .context("UI exited with an error")
}
