//! Reading and writing `project.rwproj`, with schema migration.

use crate::project::{Project, SCHEMA_VERSION};
use serde_json::Value;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// File name of the project document inside a project folder.
pub const PROJECT_FILE: &str = "project.rwproj";
/// Previous good copy, kept beside the project file on every save.
pub const BACKUP_FILE: &str = "project.rwproj.bak";
/// Per-project cache directory (thumbnails, proxies, peaks). Safe to delete.
pub const CACHE_DIR: &str = "cache";

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("project file is not valid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("project file has no schema version")]
    MissingVersion,
    #[error(
        "project was saved by a newer version (schema {found}, this build reads up to {SCHEMA_VERSION})"
    )]
    TooNew { found: u32 },
}

/// Upgrades a parsed document to [`SCHEMA_VERSION`], one step at a time.
pub fn migrate(mut doc: Value) -> Result<Value, FileError> {
    let version = doc
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(FileError::MissingVersion)? as u32;
    if version > SCHEMA_VERSION {
        return Err(FileError::TooNew { found: version });
    }
    // Each future bump adds an arm: `1 => { ...; doc["version"] = 2.into(); }`.
    #[allow(clippy::match_single_binding)]
    match version {
        _ => {}
    }
    doc["version"] = SCHEMA_VERSION.into();
    Ok(doc)
}

pub fn from_json(text: &str) -> Result<Project, FileError> {
    let doc: Value = serde_json::from_str(text)?;
    Ok(serde_json::from_value(migrate(doc)?)?)
}

pub fn to_json(project: &Project) -> Result<String, FileError> {
    let mut text = serde_json::to_string_pretty(project)?;
    text.push('\n');
    Ok(text)
}

/// Loads the project in `dir`, falling back to the backup if the main file is
/// missing or unreadable.
pub fn load(dir: &Path) -> Result<Project, FileError> {
    match fs::read_to_string(dir.join(PROJECT_FILE))
        .map_err(FileError::from)
        .and_then(|t| from_json(&t))
    {
        Ok(p) => Ok(p),
        Err(primary) => match fs::read_to_string(dir.join(BACKUP_FILE)) {
            Ok(text) => from_json(&text).map_err(|_| primary),
            Err(_) => Err(primary),
        },
    }
}

/// Writes the project atomically: new content goes to a temporary file that
/// replaces the old one, which is kept as the backup.
pub fn save(dir: &Path, project: &Project) -> Result<(), FileError> {
    fs::create_dir_all(dir)?;
    let text = to_json(project)?;
    let target = dir.join(PROJECT_FILE);
    let tmp = dir.join(format!("{PROJECT_FILE}.tmp"));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    if target.exists() {
        fs::copy(&target, dir.join(BACKUP_FILE))?;
    }
    fs::rename(&tmp, &target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{AspectPreset, Canvas};

    #[test]
    fn save_load_round_trip_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Project::new("Trip", Canvas::from_preset(AspectPreset::Vertical9x16));
        save(dir.path(), &p).unwrap();
        assert_eq!(load(dir.path()).unwrap(), p);

        p.name = "Trip 2".into();
        save(dir.path(), &p).unwrap();
        assert!(dir.path().join(BACKUP_FILE).exists());

        fs::write(dir.path().join(PROJECT_FILE), "{ corrupt").unwrap();
        assert_eq!(load(dir.path()).unwrap().name, "Trip");
    }

    #[test]
    fn rejects_newer_schema() {
        let p = Project::new("x", Canvas::default());
        let mut v = serde_json::to_value(&p).unwrap();
        v["version"] = (SCHEMA_VERSION + 1).into();
        assert!(matches!(
            from_json(&v.to_string()),
            Err(FileError::TooNew { .. })
        ));
    }

    #[test]
    fn rejects_missing_version() {
        assert!(matches!(from_json("{}"), Err(FileError::MissingVersion)));
    }
}
