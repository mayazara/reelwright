//! Frame cache (GPU + CPU LRU) and on-disk proxy and peak files.
//!
//! M1 provides the on-disk layout of a project's `cache/` folder. Everything
//! in it can be regenerated from the source media, so deleting it is safe.

use rw_model::ContentHash;
use rw_model::file::CACHE_DIR;
use rw_model::library::THUMBNAIL_FILE;
use std::io;
use std::path::{Path, PathBuf};

/// Paths inside one project's cache folder. Files are keyed by the media's
/// content hash, so duplicated projects and re-imports share entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectCache {
    root: PathBuf,
}

impl ProjectCache {
    /// Cache for the project stored in `project_dir`.
    pub fn for_project(project_dir: &Path) -> Self {
        ProjectCache {
            root: project_dir.join(CACHE_DIR),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Media thumbnail (PNG).
    pub fn thumbnail(&self, hash: &ContentHash) -> PathBuf {
        self.root
            .join("thumbs")
            .join(format!("{}.png", hash.as_str()))
    }

    /// Preview proxy (MJPEG in Matroska).
    pub fn proxy(&self, hash: &ContentHash) -> PathBuf {
        self.root
            .join("proxies")
            .join(format!("{}.mkv", hash.as_str()))
    }

    /// Thumbnail shown for the whole project on the home screen.
    pub fn project_thumbnail(&self) -> PathBuf {
        self.root.join(THUMBNAIL_FILE)
    }

    /// Removes everything in the cache.
    pub fn clear(&self) -> io::Result<()> {
        match std::fs::remove_dir_all(&self.root) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_keyed_by_hash() {
        let c = ProjectCache::for_project(Path::new("/p/Holiday"));
        let h = ContentHash("abc".into());
        assert_eq!(
            c.thumbnail(&h),
            Path::new("/p/Holiday/cache/thumbs/abc.png")
        );
        assert_eq!(c.proxy(&h), Path::new("/p/Holiday/cache/proxies/abc.mkv"));
    }

    #[test]
    fn clear_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let c = ProjectCache::for_project(dir.path());
        std::fs::create_dir_all(c.root().join("thumbs")).unwrap();
        c.clear().unwrap();
        assert!(!c.root().exists());
        c.clear().unwrap();
    }
}
