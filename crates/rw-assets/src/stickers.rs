//! Sticker packs: the bundled emoji set and packs people add from disk.
//!
//! A user pack is a folder of SVG or PNG pictures inside the packs folder;
//! the folder name is the pack's name and each file's name its id.

use serde::Deserialize;
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/openmoji.rs"));

/// The bundled pack's name.
pub const BUNDLED: &str = "openmoji";

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StickerInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub emoji: String,
    pub group: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl StickerInfo {
    /// Whether every word of `query` appears in the name, tags or group.
    pub fn matches(&self, query: &str) -> bool {
        let hay = format!(
            "{} {} {}",
            self.name.to_lowercase(),
            self.tags.join(" ").to_lowercase(),
            self.group.to_lowercase()
        );
        query
            .split_whitespace()
            .all(|w| hay.contains(&w.to_lowercase()))
    }
}

/// The bundled stickers, in index order.
pub fn bundled() -> &'static [StickerInfo] {
    static LIST: OnceLock<Vec<StickerInfo>> = OnceLock::new();
    LIST.get_or_init(|| serde_json::from_str(OPENMOJI_INDEX).unwrap_or_default())
}

static USER_PACKS: OnceLock<PathBuf> = OnceLock::new();

/// Where user packs live (call once at start-up).
pub fn set_user_packs_dir(dir: PathBuf) {
    let _ = USER_PACKS.set(dir);
}

pub fn user_packs_dir() -> Option<&'static PathBuf> {
    USER_PACKS.get()
}

/// Picture file formats a pack may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Svg,
    Png,
}

/// User packs and their pictures, sorted by name.
pub fn user_packs() -> Vec<(String, Vec<StickerInfo>)> {
    let Some(root) = user_packs_dir() else {
        return Vec::new();
    };
    let Ok(dirs) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut packs: Vec<(String, Vec<StickerInfo>)> = dirs
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let pack = e.file_name().to_string_lossy().into_owned();
            if pack == BUNDLED {
                return None;
            }
            let mut items: Vec<StickerInfo> = std::fs::read_dir(e.path())
                .ok()?
                .filter_map(Result::ok)
                .map(|f| f.path())
                .filter(|p| {
                    p.extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| matches!(x.to_lowercase().as_str(), "svg" | "png"))
                })
                .map(|p| {
                    let id = p
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    StickerInfo {
                        name: id.replace(['-', '_'], " "),
                        id,
                        emoji: String::new(),
                        group: pack.clone(),
                        tags: Vec::new(),
                    }
                })
                .collect();
            items.sort_by(|a, b| a.name.cmp(&b.name));
            (!items.is_empty()).then_some((pack, items))
        })
        .collect();
    packs.sort_by(|a, b| a.0.cmp(&b.0));
    packs
}

/// The picture's file contents and format.
pub fn data(pack: &str, id: &str) -> Option<(Cow<'static, [u8]>, Format)> {
    if pack == BUNDLED {
        return OPENMOJI
            .binary_search_by(|(k, _)| (*k).cmp(id))
            .ok()
            .map(|i| (Cow::Borrowed(OPENMOJI[i].1), Format::Svg));
    }
    let dir = user_packs_dir()?.join(pack);
    for (ext, format) in [
        ("svg", Format::Svg),
        ("png", Format::Png),
        ("PNG", Format::Png),
        ("SVG", Format::Svg),
    ] {
        let p = dir.join(format!("{id}.{ext}"));
        if let Ok(bytes) = std::fs::read(&p) {
            return Some((Cow::Owned(bytes), format));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_index_matches_the_files() {
        let list = bundled();
        assert!(list.len() > 300, "{} stickers", list.len());
        for s in list {
            assert!(data(BUNDLED, &s.id).is_some(), "no file for {}", s.id);
        }
        assert!(
            OPENMOJI.windows(2).all(|w| w[0].0 < w[1].0),
            "sorted for lookup"
        );
        let smile = list.iter().find(|s| s.matches("grinning")).unwrap();
        assert!(smile.matches("GRINNING face"));
        assert!(!smile.matches("rocket"));
    }
}
