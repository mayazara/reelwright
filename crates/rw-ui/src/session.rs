//! An open project: the document, its history, cache and autosave state.

use crate::preview::{Preview, UiGpu};
use crate::timeline::{PeaksCache, StripCache, TimelineState};
use rw_audio::output::OutputFactory;
use rw_cache::ProjectCache;
use rw_edit::{Command, Compound, EditError, History, commands::AddMedia};
use rw_engine::playback::Player;
use rw_engine::{MediaEvent, MediaJobs};
use rw_model::{
    ContentHash, Item, ItemId, MediaKind, Project, Time, Track, TrackId, TrackKind, file,
};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
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

/// Copied clips, with the track each came from.
#[derive(Debug, Clone, Default)]
pub struct Clipboard {
    pub items: Vec<(TrackId, TrackKind, Item)>,
}

pub struct Session {
    pub dir: PathBuf,
    pub project: Project,
    pub history: History,
    pub cache: ProjectCache,
    pub player: Player,
    pub selection: BTreeSet<ItemId>,
    pub timeline: TimelineState,
    pub clipboard: Clipboard,
    pub peaks_cache: PeaksCache,
    pub strips: StripCache,
    /// Present when the UI has a GPU (always in the app; not in tests).
    pub preview: Option<Preview>,
    /// Bumped on every project change; the preview re-renders on change.
    pub revision: u64,
    /// Pixel size the preview panel last asked for.
    pub(crate) preview_size: (u32, u32),
    /// A look change being dragged (slider, handle) but not yet committed;
    /// the preview shows it, the project and history don't have it yet.
    pub preview_patch: Option<(Vec<ItemId>, rw_edit::timeline::LookPatch)>,
    /// A background colour being dragged, likewise uncommitted.
    pub preview_fill: Option<(Vec<ItemId>, rw_model::Generated)>,
    /// Text being typed or styled, likewise uncommitted.
    pub preview_text: Option<(ItemId, rw_model::TextSpec)>,
    /// On-canvas handles and crop mode.
    pub canvas_tools: crate::canvas_tools::CanvasState,
    pub thumbs: HashMap<ContentHash, JobState>,
    pub proxies: HashMap<ContentHash, JobState>,
    pub peaks: HashMap<ContentHash, JobState>,
    pending_batches: HashSet<u64>,
    pub notices: Vec<Notice>,
    last_save: Instant,
    last_change: Instant,
    last_seen_state: u64,
    pub save_error: Option<String>,
}

impl Session {
    pub fn new(
        dir: PathBuf,
        mut project: Project,
        audio: Arc<dyn OutputFactory>,
        gpu: Option<UiGpu>,
    ) -> Self {
        ensure_main_track(&mut project);
        let cache = ProjectCache::for_project(&dir);
        let now = Instant::now();
        let history = History::new();
        Session {
            player: Player::new(audio, cache.clone(), Arc::new(project.clone())),
            preview: gpu.map(|g| Preview::new(g, cache.clone())),
            selection: BTreeSet::new(),
            timeline: TimelineState::new(),
            clipboard: Clipboard::default(),
            peaks_cache: PeaksCache::default(),
            strips: StripCache::default(),
            revision: 0,
            preview_size: (project.canvas.width / 2, project.canvas.height / 2),
            preview_patch: None,
            preview_fill: None,
            preview_text: None,
            canvas_tools: Default::default(),
            dir,
            project,
            last_seen_state: history.state(),
            history,
            cache,
            thumbs: HashMap::new(),
            proxies: HashMap::new(),
            peaks: HashMap::new(),
            pending_batches: HashSet::new(),
            notices: Vec::new(),
            last_save: now,
            last_change: now,
            save_error: None,
        }
    }

    pub fn open(
        dir: &Path,
        audio: Arc<dyn OutputFactory>,
        gpu: Option<UiGpu>,
    ) -> Result<Self, file::FileError> {
        let project = file::load(dir)?;
        let had_main = project.main_track().is_some();
        let mut s = Session::new(dir.to_path_buf(), project, audio, gpu);
        if !had_main {
            // Projects from before the timeline existed gain a main track.
            s.save();
        }
        Ok(s)
    }

    /// Shows an uncommitted look change in the preview.
    pub fn set_preview_patch(&mut self, items: Vec<ItemId>, patch: rw_edit::timeline::LookPatch) {
        self.preview_patch = Some((items, patch));
        self.revision += 1;
    }

    pub fn set_preview_fill(&mut self, items: Vec<ItemId>, fill: rw_model::Generated) {
        self.preview_fill = Some((items, fill));
        self.revision += 1;
    }

    pub fn set_preview_text(&mut self, item: ItemId, text: rw_model::TextSpec) {
        self.preview_text = Some((item, text));
        self.revision += 1;
    }

    pub fn clear_preview_patch(&mut self) {
        let had = self.preview_patch.take().is_some()
            | self.preview_fill.take().is_some()
            | self.preview_text.take().is_some();
        if had {
            self.revision += 1;
        }
    }

    /// The project as the preview should show it (with any uncommitted
    /// look change applied).
    pub fn preview_project(&self) -> std::borrow::Cow<'_, Project> {
        if self.preview_patch.is_none()
            && self.preview_fill.is_none()
            && self.preview_text.is_none()
        {
            return std::borrow::Cow::Borrowed(&self.project);
        }
        let mut p = self.project.clone();
        for item in p.tracks.iter_mut().flat_map(|t| t.items.iter_mut()) {
            if let Some((ids, patch)) = &self.preview_patch
                && ids.contains(&item.id)
            {
                patch.apply(&mut item.look);
                item.look = item.look.clone().sanitized(item.duration);
            }
            if let Some((ids, fill)) = &self.preview_fill
                && ids.contains(&item.id)
                && item.generated_fill().is_some()
            {
                item.kind = rw_model::ItemKind::Generated { fill: *fill };
            }
            if let Some((id, text)) = &self.preview_text
                && *id == item.id
                && item.text().is_some()
            {
                item.kind = rw_model::ItemKind::Text {
                    text: Box::new(text.clone().sanitized(item.duration)),
                };
            }
        }
        std::borrow::Cow::Owned(p)
    }

    /// Call after any change to `project`: refreshes playback and preview.
    pub fn changed(&mut self) {
        self.preview_patch = None;
        self.preview_fill = None;
        self.preview_text = None;
        self.revision += 1;
        self.player.set_project(Arc::new(self.project.clone()));
        let live: HashSet<ItemId> = self.project.items().map(|i| i.id).collect();
        self.selection.retain(|id| live.contains(id));
    }

    /// The playhead (live while playing).
    pub fn playhead(&self) -> Time {
        self.player.position()
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
        match &r {
            Ok(()) => self.changed(),
            Err(e) => self.notify_error(e.to_string()),
        }
        r
    }

    pub fn undo(&mut self) -> Option<String> {
        let r = self.history.undo(&mut self.project);
        if r.is_some() {
            self.changed();
        }
        r
    }

    pub fn redo(&mut self) -> Option<String> {
        let r = self.history.redo(&mut self.project);
        if r.is_some() {
            self.changed();
        }
        r
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

    /// Applies a finished background job. Returns the path of a cache file
    /// (thumbnail or peaks) that changed on disk.
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
            MediaEvent::Filmstrip { hash, result } => {
                if let Err(e) = result {
                    tracing::warn!(error = %e, "filmstrip failed");
                }
                self.strips.forget(&hash);
                let media = self.project.media.iter().find(|m| m.hash == hash)?;
                Some(rw_engine::filmstrip::paths(&self.cache, media).0)
            }
            MediaEvent::Peaks { hash, result } => {
                let path = self.cache.peaks(&hash);
                self.peaks.insert(
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
                if let Some(p) = self.preview.as_mut() {
                    p.invalidate();
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

/// Adds the magnetic main track if the project has none.
pub fn ensure_main_track(project: &mut Project) {
    if project.main_track().is_none() {
        project.tracks.insert(0, Track::new_main());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rw_edit::commands::RenameProject;
    use rw_model::Canvas;

    fn test_session(dir: &Path, project: Project) -> Session {
        Session::new(
            dir.to_path_buf(),
            project,
            Arc::new(rw_audio::output::NullFactory::default()),
            None,
        )
    }

    #[test]
    fn autosave_waits_for_idle() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = test_session(dir.path(), Project::new("A", Canvas::default()));
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
        let mut s = test_session(dir.path(), Project::new("A", Canvas::default()));
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
