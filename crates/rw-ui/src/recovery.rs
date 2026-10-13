//! Crash recovery.
//!
//! While the app runs, a small marker file beside the settings says which
//! project is open; a clean exit removes it. Finding it at start-up means
//! the app didn't close properly, so the home screen offers to reopen that
//! project (autosave keeps it within a few seconds of the last edit). A
//! panic hook writes a crash report to the same folder.
//!
//! Recordings are written crash-safe into the project's `Recordings`
//! folder; any that never reached the project are offered when it opens.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Marker file name, beside the settings.
pub const MARKER: &str = "running.json";
/// Folder of crash reports, beside the settings.
pub const REPORTS: &str = "crash-reports";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Marker {
    /// The project open when the marker was last written.
    project: Option<PathBuf>,
}

/// What a previous run left behind when it didn't close properly.
#[derive(Debug, Clone, PartialEq)]
pub struct Unclean {
    /// The project that was open, if any (and still exists).
    pub project: Option<PathBuf>,
    /// The newest crash report, if the app crashed (rather than being
    /// killed or losing power).
    pub report: Option<PathBuf>,
}

/// Tracks this run in the marker file in `dir` (the settings folder).
#[derive(Debug)]
pub struct Recovery {
    dir: PathBuf,
}

impl Recovery {
    /// Starts tracking. Returns what an unclean previous run left, if any.
    pub fn start(dir: &Path) -> (Self, Option<Unclean>) {
        let marker = dir.join(MARKER);
        let unclean = std::fs::read_to_string(&marker).ok().map(|text| {
            let m: Marker = serde_json::from_str(&text).unwrap_or_default();
            let modified = std::fs::metadata(&marker).and_then(|m| m.modified()).ok();
            Unclean {
                project: m.project.filter(|p| p.is_dir()),
                // A report written after the marker belongs to that run.
                report: newest_report(dir)
                    .filter(|(_, at)| modified.is_none_or(|m| *at >= m))
                    .map(|(p, _)| p),
            }
        });
        let r = Recovery {
            dir: dir.to_path_buf(),
        };
        r.write(None);
        (r, unclean)
    }

    /// Records which project is open.
    pub fn set_project(&self, project: Option<&Path>) {
        self.write(project);
    }

    /// A clean exit: nothing to recover next time.
    pub fn finish(&self) {
        let _ = std::fs::remove_file(self.dir.join(MARKER));
    }

    fn write(&self, project: Option<&Path>) {
        let m = Marker {
            project: project.map(Path::to_path_buf),
        };
        if std::fs::create_dir_all(&self.dir).is_ok()
            && let Ok(text) = serde_json::to_string(&m)
        {
            let _ = std::fs::write(self.dir.join(MARKER), text);
        }
    }
}

fn newest_report(dir: &Path) -> Option<(PathBuf, std::time::SystemTime)> {
    std::fs::read_dir(dir.join(REPORTS))
        .ok()?
        .flatten()
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .max_by_key(|(_, t)| *t)
}

/// Writes a crash report into `dir`/crash-reports when the app panics,
/// then runs the usual panic handling.
pub fn install_panic_hook(dir: PathBuf, version: &'static str) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let report = crash_report(info, version);
        let folder = dir.join(REPORTS);
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if std::fs::create_dir_all(&folder).is_ok() {
            let _ = std::fs::write(folder.join(format!("crash-{secs}.txt")), report);
        }
        previous(info);
    }));
}

fn crash_report(info: &std::panic::PanicHookInfo<'_>, version: &str) -> String {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown".into());
    let location = info
        .location()
        .map(|l| format!("{}:{}", l.file(), l.line()))
        .unwrap_or_default();
    let thread = std::thread::current();
    format!(
        "version: {version}\nos: {} {}\nthread: {}\nmessage: {message}\nat: {location}\n\n{}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        thread.name().unwrap_or("unnamed"),
        std::backtrace::Backtrace::force_capture()
    )
}

/// Recordings in `project_dir`/Recordings that `known` (the project's
/// media paths) doesn't include, oldest first.
pub fn stray_recordings(project_dir: &Path, known: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(project_dir.join("Recordings"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e, "mp4" | "m4a"))
        })
        .filter(|p| !known.iter().any(|k| k == p))
        // Empty files are recordings that never got a picture or sound.
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() > 0))
        .collect();
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_exit_leaves_nothing_to_recover() {
        let tmp = tempfile::tempdir().unwrap();
        let (r, unclean) = Recovery::start(tmp.path());
        assert!(unclean.is_none());
        r.set_project(Some(tmp.path()));
        r.finish();
        let (_, unclean) = Recovery::start(tmp.path());
        assert!(unclean.is_none());
    }

    #[test]
    fn unclean_exit_names_the_open_project() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("Holiday");
        std::fs::create_dir(&project).unwrap();
        let (r, _) = Recovery::start(tmp.path());
        r.set_project(Some(&project));
        drop(r); // killed: no finish()
        let (_, unclean) = Recovery::start(tmp.path());
        let unclean = unclean.expect("an unclean exit");
        assert_eq!(unclean.project.as_deref(), Some(project.as_path()));
        assert!(unclean.report.is_none());
        // The new run has its own marker now; a second start would see
        // this run as unclean too, but with no project.
        let (_, again) = Recovery::start(tmp.path());
        assert_eq!(again.unwrap().project, None);
    }

    #[test]
    fn a_crash_report_from_that_run_is_found() {
        let tmp = tempfile::tempdir().unwrap();
        let (_r, _) = Recovery::start(tmp.path());
        std::fs::create_dir_all(tmp.path().join(REPORTS)).unwrap();
        let report = tmp.path().join(REPORTS).join("crash-1.txt");
        std::fs::write(&report, "boom").unwrap();
        let (_, unclean) = Recovery::start(tmp.path());
        assert_eq!(unclean.unwrap().report, Some(report));
    }

    #[test]
    fn stray_recordings_are_the_unknown_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("Recordings");
        std::fs::create_dir(&rec).unwrap();
        for (name, body) in [
            ("Recording 1 - Screen.mp4", "x"),
            ("Recording 2 - Screen.mp4", "x"),
            ("Recording 2 - Camera.mp4", ""),
            ("notes.txt", "x"),
        ] {
            std::fs::write(rec.join(name), body).unwrap();
        }
        let known = vec![rec.join("Recording 1 - Screen.mp4")];
        assert_eq!(
            stray_recordings(tmp.path(), &known),
            [rec.join("Recording 2 - Screen.mp4")]
        );
    }
}
