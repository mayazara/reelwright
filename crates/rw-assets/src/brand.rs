//! The brand kit: colours, fonts and logos kept on this computer and
//! offered in every project.
//!
//! Stored as `brand.json` plus a `logos` folder inside the kit folder.
//! Logos are also a sticker pack named [`LOGO_PACK`].

use rw_model::Rgba;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Sticker pack name under which logos appear.
pub const LOGO_PACK: &str = "brand-kit";

/// Picture types a logo may be.
pub const LOGO_TYPES: &[&str] = &["png", "svg"];

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BrandKit {
    pub colours: Vec<Rgba>,
    pub fonts: Vec<String>,
}

impl BrandKit {
    fn file(dir: &Path) -> PathBuf {
        dir.join("brand.json")
    }

    pub fn logos_dir(dir: &Path) -> PathBuf {
        dir.join("logos")
    }

    /// The kit in `dir`, or an empty one.
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(Self::file(dir))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = dir.join("brand.json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, Self::file(dir))
    }

    pub fn add_colour(&mut self, c: Rgba) {
        if !self.colours.contains(&c) {
            self.colours.push(c);
        }
    }

    pub fn add_font(&mut self, f: &str) {
        if !f.trim().is_empty() && !self.fonts.iter().any(|x| x == f) {
            self.fonts.push(f.to_owned());
        }
    }

    /// Logo ids (file names without extension), sorted.
    pub fn logos(dir: &Path) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(Self::logos_dir(dir))
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| LOGO_TYPES.contains(&x.to_lowercase().as_str()))
            })
            .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Copies `file` into the kit's logos; returns its id. A logo with the
    /// same name gets a number added.
    pub fn add_logo(dir: &Path, file: &Path) -> std::io::Result<String> {
        let ext = file
            .extension()
            .and_then(|x| x.to_str())
            .map(str::to_lowercase)
            .filter(|x| LOGO_TYPES.contains(&x.as_str()))
            .ok_or_else(|| std::io::Error::other("logos must be PNG or SVG pictures"))?;
        let stem = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "logo".into());
        let logos = Self::logos_dir(dir);
        std::fs::create_dir_all(&logos)?;
        let existing = Self::logos(dir);
        let mut id = stem.clone();
        let mut n = 2;
        while existing.contains(&id) {
            id = format!("{stem} {n}");
            n += 1;
        }
        std::fs::copy(file, logos.join(format!("{id}.{ext}")))?;
        Ok(id)
    }

    pub fn remove_logo(dir: &Path, id: &str) -> std::io::Result<()> {
        for ext in LOGO_TYPES {
            let p = Self::logos_dir(dir).join(format!("{id}.{ext}"));
            if p.exists() {
                std::fs::remove_file(p)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kit_round_trips_and_logos_get_unique_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut kit = BrandKit::default();
        kit.add_colour(Rgba([1, 2, 3, 255]));
        kit.add_colour(Rgba([1, 2, 3, 255]));
        kit.add_font("Lora");
        kit.save(dir.path()).unwrap();
        assert_eq!(BrandKit::load(dir.path()), kit);
        assert_eq!(kit.colours.len(), 1, "no duplicates");

        let src = dir.path().join("Acme.svg");
        std::fs::write(
            &src,
            "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'/>",
        )
        .unwrap();
        assert_eq!(BrandKit::add_logo(dir.path(), &src).unwrap(), "Acme");
        assert_eq!(BrandKit::add_logo(dir.path(), &src).unwrap(), "Acme 2");
        assert_eq!(BrandKit::logos(dir.path()), ["Acme", "Acme 2"]);
        assert!(BrandKit::add_logo(dir.path(), &dir.path().join("x.bmp")).is_err());
        BrandKit::remove_logo(dir.path(), "Acme").unwrap();
        assert_eq!(BrandKit::logos(dir.path()), ["Acme 2"]);
    }
}
