//! An open project: the document, its history, cache and autosave state.

use rw_cache::ProjectCache;
use rw_edit::{Command, Compound, EditError, History, commands::AddMedia};
use rw_engine::{MediaEvent, MediaJobs};
use rw_model::{ContentHash, MediaKind, Project, file};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Save at least this often while there are unsaved changes.
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);
/// Save once the user has been idle this long after a change.
pub const AUTOSAVE_IDLE: Duration = Duration::from_secs(2);

/// Progress of a background job for one media file.
#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Running(f32),
    Done,
    Failed(String),
}

/// Something the user should be told about, shown as a dismissible notice.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub text: String,
    pub error: bool,
}

pub struct Session {
    pub dir: PathBuf,
    pub project: Project,
    pub history: History,
    pub cache: ProjectCache,
    pub thumbs: HashMap<ContentHash, JobState>,
    pub proxies: HashMap<ContentHash, JobState>,
    pending_batches: HashSet<u64>,
    pub notices: Vec<Notice>,
    last_save: Instant,
    last_change: Instant,
    last_seen_state: u64,
    pub save_error: Option<String>,
}

impl Session {
    pub fn new(dir: PathBuf, project: Project) -> Self {
        let cache = ProjectCache::for_project(&dir);
        let now = Instant::now();
        let history = History::new();
        Session {
            dir,
            project,
            last_seen_state: history.state(),
            history,
            cache,
            thumbs: HashMap::new(),
            proxies: HashMap::new(),
            pending_batches: HashSet::new(),
            notices: Vec::new(),
            last_save: now,
            last_change: now,
            save_error: None,
        }
    }

    pub fn open(dir: &Path) -> Result<Self, file::FileError> {
        let project = file::load(dir)?;
        Ok(Session::new(dir.to_path_buf(), project))
    }

    /// Starts thumbnail/proxy jobs for every media item whose cache files
    /// are missing.
    pub fn prepare_all(&mut self, jobs: &MediaJobs) {
        for m in &self.project.media {
            jobs.prepare(&self.cache, m);
            if m.kind() != MediaKind::Audio {
                self.thumbs
                    .entry(m.hash.clone())
                    .or_insert(JobState::Running(0.0));
            }
            if rw_media::needs_proxy(&m.info) && !self.cache.proxy(&m.hash).exists() {
                self.proxies
                    .entry(m.hash.clone())
                    .or_insert(JobState::Running(0.0));
            }
        }
    }

    pub fn execute(&mut self, command: Box<dyn Command>) -> Result<(), EditError> {
        let r = self.history.execute(&mut self.project, command);
        if let Err(e) = &r {
            self.notify_error(e.to_string());
        }
        r
    }

    pub fn undo(&mut self) -> Option<String> {
        self.history.undo(&mut self.project)
    }

    pub fn redo(&mut self) -> Option<String> {
        self.history.redo(&mut self.project)
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.notices.push(Notice {
            text: text.into(),
            error: false,
        });
    }

    pub fn notify_error(&mut self, text: impl Into<String>) {
        self.notices.push(Notice {
            text: text.into(),
            error: true,
        });
    }

    /// Starts importing files.
    pub fn import(&mut self, jobs: &MediaJobs, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let batch = jobs.import(paths);
        self.pending_batches.insert(batch);
    }

    pub fn importing(&self) -> bool {
        !self.pending_batches.is_empty()
    }

    /// Applies a finished background job. Returns paths of thumbnails that
    /// changed on disk.
    pub fn handle(&mut self, jobs: &MediaJobs, event: MediaEvent) -> Option<PathBuf> {
        match event {
            MediaEvent::Imported {
                batch,
                media,
                failed,
            } => {
                if !self.pending_batches.remove(&batch) {
                    return None;
                }
                let known: HashSet<_> = self.project.media.iter().map(|m| m.hash.clone()).collect();
                let mut seen = HashSet::new();
                let (fresh, dupes): (Vec<_>, Vec<_>) = media
                    .into_iter()
                    .partition(|m| !known.contains(&m.hash) && seen.insert(m.hash.clone()));
                if !fresh.is_empty() {
                    let label = match fresh.len() {
                        1 => format!("Import {}", fresh[0].name),
                        n => format!("Import {n} files"),
                    };
                    let cmds: Vec<Box<dyn Command>> = fresh
                        .iter()
                        .cloned()
                        .map(|m| Box::new(AddMedia::new(m)) as Box<dyn Command>)
                        .collect();
                    if self.execute(Box::new(Compound::new(label, cmds))).is_ok() {
                        for m in &fresh {
                            jobs.prepare(&self.cache, m);
                            if m.kind() != MediaKind::Audio {
                                self.thumbs.insert(m.hash.clone(), JobState::Running(0.0));
                            }
                            if rw_media::needs_proxy(&m.info) {
                                self.proxies.insert(m.hash.clone(), JobState::Running(0.0));
                            }
                        }
                    }
                }
                if !dupes.is_empty() {
                    self.notify(format!(
                        "Skipped {} file(s) already in this project.",
                        dupes.len()
                    ));
                }
                for (path, why) in failed {
                    let name = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    self.notify_error(format!("Couldn't import {name}: {why}"));
                }
                None
            }
            MediaEvent::Thumbnail { hash, result } => {
                let path = self.cache.thumbnail(&hash);
                self.thumbs.insert(
                    hash,
                    match result {
                        Ok(_) => JobState::Done,
                        Err(e) => JobState::Failed(e),
                    },
                );
                Some(path)
            }
            MediaEvent::ProxyProgress { hash, fraction } => {
                if let Some(JobState::Running(_)) | None = self.proxies.get(&hash) {
                    self.proxies.insert(hash, JobState::Running(fraction));
                }
                None
            }
            MediaEvent::Proxy { hash, result } => {
                if let Err(e) = &result {
                    tracing::warn!(error = %e, "proxy failed");
                }
                self.proxies.insert(
                    hash,
                    match result {
                        Ok(_) => JobState::Done,
                        Err(e) => JobState::Failed(e),
                    },
                );
                None
            }
        }
    }

    /// Notes user activity so idle autosave waits for a pause.
    pub fn touch(&mut self, now: Instant) {
        if self.history.state() != self.last_seen_state {
            self.last_seen_state = self.history.state();
            self.last_change = now;
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }

    /// Saves if there are changes and the autosave policy says it is time.
    pub fn autosave(&mut self, now: Instant) -> bool {
        self.touch(now);
        if !self.is_dirty() {
            return false;
        }
        let idle = now.duration_since(self.last_change) >= AUTOSAVE_IDLE;
        let overdue = now.duration_since(self.last_save) >= AUTOSAVE_INTERVAL;
        if idle || overdue {
            self.save();
            true
        } else {
            false
        }
    }

    /// Writes the project now and refreshes the home-screen thumbnail.
    pub fn save(&mut self) {
        self.last_save = Instant::now();
        match file::save(&self.dir, &self.project) {
            Ok(()) => {
                self.history.mark_saved();
                self.save_error = None;
                self.update_project_thumbnail();
            }
            Err(e) => {
                tracing::error!(error = %e, dir = %self.dir.display(), "save failed");
                self.save_error = Some(e.to_string());
            }
        }
    }

    /// Uses the first visual media's thumbnail for the project card.
    fn update_project_thumbnail(&self) {
        let target = self.cache.project_thumbnail();
        let source = self
            .project
            .media
            .iter()
            .filter(|m| m.kind() != MediaKind::Audio)
            .map(|m| self.cache.thumbnail(&m.hash))
            .find(|p| p.exists());
        match source {
            Some(src) => {
                let _ = std::fs::copy(src, target);
            }
            None => {
                let _ = std::fs::remove_file(target);
            }
        }
    }

    /// Short status for the top bar.
    pub fn status(&self) -> String {
        if let Some(e) = &self.save_error {
            format!("Not saved: {e}")
        } else if self.is_dirty() {
            "Unsaved changes".into()
        } else {
            "All changes saved".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rw_edit::commands::RenameProject;
    use rw_model::Canvas;

    #[test]
    fn autosave_waits_for_idle() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new(
            dir.path().to_path_buf(),
            Project::new("A", Canvas::default()),
        );
        s.save();
        let t0 = Instant::now();
        s.execute(Box::new(RenameProject::new("B"))).unwrap();
        assert!(!s.autosave(t0), "just changed: not idle yet");
        assert!(s.is_dirty());
        assert!(s.autosave(t0 + AUTOSAVE_IDLE + Duration::from_millis(10)));
        assert!(!s.is_dirty());
        assert_eq!(file::load(dir.path()).unwrap().name, "B");
    }

    #[test]
    fn autosave_forced_after_interval_even_when_busy() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new(
            dir.path().to_path_buf(),
            Project::new("A", Canvas::default()),
        );
        let start = Instant::now();
        let mut saved = false;
        // A change every second for 40 s never leaves 2 s of idle time.
        for i in 0..40 {
            s.execute(Box::new(RenameProject::new(format!("N{i}"))))
                .unwrap();
            saved |= s.autosave(start + Duration::from_secs(i));
        }
        assert!(saved, "interval save must happen within 30 s");
    }
}
