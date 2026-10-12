//! App settings, stored as JSON in the platform config directory.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::theme::ThemeMode;

/// Most recently opened projects to remember.
pub const MAX_RECENT: usize = 20;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeSetting,
    /// Project folders opened recently, newest first. Includes projects
    /// outside the projects folder.
    pub recent: Vec<PathBuf>,
    /// Folder of the last export.
    pub export_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeSetting {
    Light,
    #[default]
    Dark,
}

impl From<ThemeSetting> for ThemeMode {
    fn from(t: ThemeSetting) -> Self {
        match t {
            ThemeSetting::Light => ThemeMode::Light,
            ThemeSetting::Dark => ThemeMode::Dark,
        }
    }
}

impl From<ThemeMode> for ThemeSetting {
    fn from(t: ThemeMode) -> Self {
        match t {
            ThemeMode::Light => ThemeSetting::Light,
            ThemeMode::Dark => ThemeSetting::Dark,
        }
    }
}

impl Settings {
    /// Loads settings, falling back to defaults if the file is missing or
    /// unreadable.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)
    }

    /// Moves `dir` to the front of the recent list.
    pub fn touch_recent(&mut self, dir: &Path) {
        self.recent.retain(|d| d != dir);
        self.recent.insert(0, dir.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }

    pub fn forget_recent(&mut self, dir: &Path) {
        self.recent.retain(|d| d != dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_recent_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());
        let mut s = Settings::default();
        for i in 0..25 {
            s.touch_recent(Path::new(&format!("/p/{i}")));
        }
        s.touch_recent(Path::new("/p/3"));
        assert_eq!(s.recent.len(), MAX_RECENT);
        assert_eq!(s.recent[0], Path::new("/p/3"));
        s.theme = ThemeSetting::Light;
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
    }

    #[test]
    fn unknown_fields_and_garbage_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"theme":"light","future":1}"#).unwrap();
        assert_eq!(Settings::load(&path).theme, ThemeSetting::Light);
        std::fs::write(&path, "nope").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }
}
