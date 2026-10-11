//! M1 acceptance: import the mixed media matrix without crashing, then
//! reopen the project "after a restart" and get identical state.

mod common;

use rw_cache::ProjectCache;
use rw_edit::{Command, Compound, History, commands::AddMedia};
use rw_engine::{MediaEvent, MediaJobs};
use rw_media::{Ffmpeg, needs_proxy};
use rw_model::library::Library;
use rw_model::{AspectPreset, Canvas, MediaKind, file};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn import_matrix_then_reopen_identical() {
    let g = common::fixtures();
    let tmp = tempfile::tempdir().unwrap();
    let library = Library::new(tmp.path().join("Projects"));
    let (dir, mut project) = library
        .create(
            "Acceptance",
            Canvas::from_preset(AspectPreset::Vertical9x16),
        )
        .unwrap();
    let cache = ProjectCache::for_project(&dir);
    let jobs = MediaJobs::new(Arc::new(Ffmpeg::new()), Arc::new(|| {}));
    let mut history = History::new();

    // Import everything in one batch, as a drag-drop of many files would.
    let paths: Vec<_> = g.available.iter().map(|f| g.path(f.name)).collect();
    let batch = jobs.import(paths.clone());
    let deadline = Instant::now() + Duration::from_secs(300);
    let (media, failed) = loop {
        match jobs.recv_timeout(Duration::from_secs(1)) {
            Some(MediaEvent::Imported {
                batch: b,
                media,
                failed,
            }) if b == batch => break (media, failed),
            _ => assert!(Instant::now() < deadline, "import timed out"),
        }
    };
    assert!(failed.is_empty(), "failed imports: {failed:?}");
    assert_eq!(media.len(), paths.len());
    let names: Vec<_> = media.iter().map(|m| m.name.clone()).collect();
    let expected: Vec<_> = g.available.iter().map(|f| f.name.to_owned()).collect();
    assert_eq!(names, expected, "import keeps the given order");

    let cmds: Vec<Box<dyn Command>> = media
        .iter()
        .cloned()
        .map(|m| Box::new(AddMedia::new(m)) as _)
        .collect();
    history
        .execute(&mut project, Box::new(Compound::new("Import", cmds)))
        .unwrap();

    // Thumbnails and proxies.
    let mut want_thumbs: BTreeSet<_> = project.media.iter().map(|m| m.hash.clone()).collect();
    let mut want_proxies: BTreeSet<_> = project
        .media
        .iter()
        .filter(|m| needs_proxy(&m.info))
        .map(|m| m.hash.clone())
        .collect();
    for m in &project.media {
        jobs.prepare(&cache, m);
    }
    while !want_thumbs.is_empty() || !want_proxies.is_empty() {
        assert!(
            Instant::now() < deadline,
            "jobs timed out; waiting on {want_thumbs:?} {want_proxies:?}"
        );
        match jobs.recv_timeout(Duration::from_secs(1)) {
            Some(MediaEvent::Thumbnail { hash, result }) => {
                result.unwrap_or_else(|e| panic!("thumbnail {hash:?}: {e}"));
                want_thumbs.remove(&hash);
            }
            Some(MediaEvent::Proxy { hash, result }) => {
                result.unwrap_or_else(|e| panic!("proxy {hash:?}: {e}"));
                want_proxies.remove(&hash);
            }
            _ => {}
        }
    }
    for m in &project.media {
        assert_eq!(
            cache.thumbnail(&m.hash).exists(),
            m.kind() != MediaKind::Audio,
            "{}",
            m.name
        );
        assert_eq!(
            cache.proxy(&m.hash).exists(),
            needs_proxy(&m.info),
            "{}",
            m.name
        );
    }

    // Save, drop all in-memory state, reload.
    file::save(&dir, &project).unwrap();
    history.mark_saved();
    assert!(!history.is_dirty());
    let saved = project.clone();
    drop((project, history, jobs));

    let reopened = file::load(&dir).unwrap();
    assert_eq!(reopened, saved);
    let summary = library
        .list()
        .unwrap()
        .into_iter()
        .find(|s| s.dir == dir)
        .unwrap();
    assert_eq!(summary.media_count, saved.media.len());
    assert_eq!(
        summary.canvas,
        Canvas::from_preset(AspectPreset::Vertical9x16)
    );

    // The JSON is stable: saving the reloaded project gives identical bytes.
    assert_eq!(
        file::to_json(&reopened).unwrap(),
        file::to_json(&saved).unwrap()
    );
}
