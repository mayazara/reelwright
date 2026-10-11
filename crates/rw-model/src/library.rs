//! The projects folder: one sub-folder per project, each holding
//! `project.rwproj` and a disposable `cache/`.

use crate::file::{self, CACHE_DIR, FileError, PROJECT_FILE};
use crate::ids::ProjectId;
use crate::project::{Canvas, Project};
use crate::time::Time;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// File name of the cached project thumbnail inside `cache/`.
pub const THUMBNAIL_FILE: &str = "project-thumb.png";

/// What the home screen needs to show one project without loading media.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSummary {
    pub dir: PathBuf,
    pub id: ProjectId,
    pub name: String,
    pub canvas: Canvas,
    pub duration: Time,
    pub media_count: usize,
    pub modified: SystemTime,
    pub thumbnail: Option<PathBuf>,
}

impl ProjectSummary {
    pub fn read(dir: &Path) -> Result<Self, FileError> {
        let project = file::load(dir)?;
        let modified = fs::metadata(dir.join(PROJECT_FILE))
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let thumb = dir.join(CACHE_DIR).join(THUMBNAIL_FILE);
        Ok(ProjectSummary {
            dir: dir.to_path_buf(),
            id: project.id,
            name: project.name.clone(),
            canvas: project.canvas,
            duration: project.duration(),
            media_count: project.media.len(),
            modified,
            thumbnail: thumb.exists().then_some(thumb),
        })
    }
}

/// A folder that holds projects.
#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
}

impl Library {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Library { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// All readable projects, most recently modified first. Unreadable
    /// folders are skipped.
    pub fn list(&self) -> io::Result<Vec<ProjectSummary>> {
        let mut out = Vec::new();
        let entries = match fs::read_dir(&self.root) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e),
        };
        for entry in entries {
            let path = entry?.path();
            if path.join(PROJECT_FILE).is_file()
                && let Ok(summary) = ProjectSummary::read(&path)
            {
                out.push(summary);
            }
        }
        out.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.name.cmp(&b.name)));
        Ok(out)
    }

    /// Creates and saves a new project in a fresh folder.
    pub fn create(&self, name: &str, canvas: Canvas) -> Result<(PathBuf, Project), FileError> {
        let project = Project::new(name.trim(), canvas);
        let dir = self.unique_dir(&project.name)?;
        file::save(&dir, &project)?;
        Ok((dir, project))
    }

    /// Copies a project (including its cache) under a new id and name.
    pub fn duplicate(&self, dir: &Path) -> Result<(PathBuf, Project), FileError> {
        let mut project = file::load(dir)?;
        project.id = ProjectId::new();
        project.name = format!("{} (copy)", project.name);
        let new_dir = self.unique_dir(&project.name)?;
        copy_dir(dir, &new_dir)?;
        file::save(&new_dir, &project)?;
        Ok((new_dir, project))
    }

    /// Deletes a project folder. Refuses paths outside the library or
    /// folders that do not hold a project.
    pub fn delete(&self, dir: &Path) -> io::Result<()> {
        let root = self.root.canonicalize()?;
        let target = dir.canonicalize()?;
        if target.parent() != Some(root.as_path()) || !target.join(PROJECT_FILE).is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a project in this library",
            ));
        }
        fs::remove_dir_all(target)
    }

    fn unique_dir(&self, name: &str) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.root)?;
        let base = sanitize(name);
        for n in 0u32.. {
            let candidate = if n == 0 {
                self.root.join(&base)
            } else {
                self.root.join(format!("{base} {}", n + 1))
            };
            match fs::create_dir(&candidate) {
                Ok(()) => return Ok(candidate),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        unreachable!("u32 range exhausted")
    }
}

/// Turns a project name into a portable folder name.
pub fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let cleaned: String = cleaned.chars().take(80).collect();
    let reserved = matches!(
        cleaned.to_ascii_uppercase().as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "LPT1" | "LPT2" | "LPT3"
    );
    if cleaned.is_empty() || reserved {
        "Untitled project".to_owned()
    } else {
        cleaned
    }
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::AspectPreset;

    #[test]
    fn create_list_duplicate_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::new(tmp.path().join("Projects"));
        assert!(lib.list().unwrap().is_empty());

        let (a_dir, a) = lib
            .create("Holiday", Canvas::from_preset(AspectPreset::Square1x1))
            .unwrap();
        let (b_dir, _) = lib.create("Holiday", Canvas::default()).unwrap();
        assert_ne!(a_dir, b_dir, "same name gets a distinct folder");

        let (c_dir, c) = lib.duplicate(&a_dir).unwrap();
        assert_ne!(c.id, a.id);
        assert_eq!(c.name, "Holiday (copy)");
        assert_eq!(c.canvas, a.canvas);

        let names: Vec<_> = lib.list().unwrap().into_iter().map(|s| s.name).collect();
        assert_eq!(names.len(), 3);

        lib.delete(&c_dir).unwrap();
        assert!(!c_dir.exists());
        assert_eq!(lib.list().unwrap().len(), 2);
    }

    #[test]
    fn delete_refuses_outside_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::new(tmp.path().join("Projects"));
        lib.create("x", Canvas::default()).unwrap();
        assert!(lib.delete(tmp.path()).is_err());
        let stray = tmp.path().join("Projects/not-a-project");
        fs::create_dir_all(&stray).unwrap();
        assert!(lib.delete(&stray).is_err());
        assert!(stray.exists());
    }

    #[test]
    fn sanitize_names() {
        assert_eq!(sanitize("a/b:c"), "a-b-c");
        assert_eq!(sanitize("  ..  "), "Untitled project");
        assert_eq!(sanitize("con"), "Untitled project");
        assert_eq!(sanitize("Mi película"), "Mi película");
    }
}
