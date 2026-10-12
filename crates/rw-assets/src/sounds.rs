//! Bundled sound effects: original, synthesised by
//! `examples/make_sounds.rs`. Embedded, and written out to a folder the
//! first time one is used so it can be imported like any other file.

use std::path::{Path, PathBuf};

macro_rules! sounds {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../../../assets/sounds/", $name, ".wav")) as &[u8])),*]
    };
}

/// Name and WAV contents of every bundled sound.
pub const SOUNDS: &[(&str, &[u8])] = sounds!(
    "chime",
    "click",
    "ding",
    "notify",
    "pop",
    "riser",
    "swoosh-down",
    "thump",
    "whoosh"
);

/// A readable name: `swoosh-down` → `Swoosh down`.
pub fn label(name: &str) -> String {
    let s = name.replace('-', " ");
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// Writes sound `name` into `dir` if it isn't there yet; returns its path.
pub fn materialise(name: &str, dir: &Path) -> std::io::Result<PathBuf> {
    let (_, data) = SOUNDS
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| std::io::Error::other("no such sound"))?;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{name}.wav"));
    let same = std::fs::read(&path).is_ok_and(|d| d == *data);
    if !same {
        std::fs::write(&path, data)?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_are_wav_and_written_once() {
        assert!(SOUNDS.len() >= 8);
        assert!(
            SOUNDS
                .iter()
                .all(|(_, d)| d.starts_with(b"RIFF") && &d[8..12] == b"WAVE")
        );
        assert_eq!(label("swoosh-down"), "Swoosh down");
        let dir = tempfile::tempdir().unwrap();
        let p = materialise("pop", dir.path()).unwrap();
        let modified = std::fs::metadata(&p).unwrap().modified().unwrap();
        assert_eq!(materialise("pop", dir.path()).unwrap(), p);
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), modified);
    }
}
