//! Parsing and checking of `assets/ATTRIBUTION.md`.
//!
//! The file holds one Markdown table whose rows are
//! `| path | source | licence | author |`, with `path` relative to `assets/`.
//! Every file under `assets/` needs a row, and every row needs a file.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Name of the attribution file at the root of the assets directory.
pub const ATTRIBUTION_FILE: &str = "ATTRIBUTION.md";

/// Files that describe the asset tree rather than being assets themselves.
const EXEMPT: &[&str] = &[ATTRIBUTION_FILE, "manifest.json"];

/// One row of the attribution table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    pub path: String,
    pub source: String,
    pub licence: String,
    pub author: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AttributionError {
    #[error("could not read assets: {0}")]
    Io(#[from] io::Error),
    #[error("line {line}: expected 4 columns, found {found}")]
    BadRow { line: usize, found: usize },
    #[error("line {line}: empty {column} for {path}")]
    EmptyField {
        line: usize,
        column: &'static str,
        path: String,
    },
    #[error("attribution out of sync; missing rows: {missing:?}; rows without files: {orphaned:?}")]
    OutOfSync {
        missing: Vec<String>,
        orphaned: Vec<String>,
    },
}

/// Parses the attribution table. Header and separator rows are skipped;
/// text outside the table is ignored.
pub fn parse(markdown: &str) -> Result<Vec<Attribution>, AttributionError> {
    let mut rows = Vec::new();
    let mut seen_header = false;
    for (idx, raw) in markdown.lines().enumerate() {
        let line = raw.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if !seen_header {
            seen_header = true;
            continue;
        }
        if cells
            .iter()
            .all(|c| c.chars().all(|ch| ch == '-' || ch == ':'))
        {
            continue;
        }
        if cells.len() != 4 {
            return Err(AttributionError::BadRow {
                line: idx + 1,
                found: cells.len(),
            });
        }
        let path = cells[0].trim_matches('`').to_owned();
        for (column, value) in [
            ("path", cells[0]),
            ("source", cells[1]),
            ("licence", cells[2]),
            ("author", cells[3]),
        ] {
            if value.is_empty() {
                return Err(AttributionError::EmptyField {
                    line: idx + 1,
                    column,
                    path: path.clone(),
                });
            }
        }
        rows.push(Attribution {
            path,
            source: cells[1].to_owned(),
            licence: cells[2].to_owned(),
            author: cells[3].to_owned(),
        });
    }
    Ok(rows)
}

/// Lists every asset file under `assets_dir` as a `/`-separated relative path,
/// excluding the exempt index files.
pub fn asset_files(assets_dir: &Path) -> io::Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    let mut stack: Vec<PathBuf> = vec![assets_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(assets_dir)
                .expect("walked path is under assets_dir")
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if !EXEMPT.contains(&rel.as_str()) {
                out.insert(rel);
            }
        }
    }
    Ok(out)
}

/// Checks that `assets_dir/ATTRIBUTION.md` has exactly one row per asset file.
pub fn check(assets_dir: &Path) -> Result<Vec<Attribution>, AttributionError> {
    let markdown = fs::read_to_string(assets_dir.join(ATTRIBUTION_FILE))?;
    let rows = parse(&markdown)?;
    let files = asset_files(assets_dir)?;
    let listed: BTreeSet<String> = rows.iter().map(|r| r.path.clone()).collect();
    let missing: Vec<String> = files.difference(&listed).cloned().collect();
    let orphaned: Vec<String> = listed.difference(&files).cloned().collect();
    if missing.is_empty() && orphaned.is_empty() {
        Ok(rows)
    } else {
        Err(AttributionError::OutOfSync { missing, orphaned })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Attribution\n\n\
        | Path | Source | Licence | Author |\n\
        | --- | --- | --- | --- |\n\
        | `branding/logo.svg` | original | MIT OR Apache-2.0 | Project |\n";

    #[test]
    fn parses_rows_and_skips_header() {
        let rows = parse(SAMPLE).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "branding/logo.svg");
        assert_eq!(rows[0].licence, "MIT OR Apache-2.0");
    }

    #[test]
    fn rejects_empty_licence() {
        let md = "| a | b | c | d |\n|---|---|---|---|\n| x.png | src |  | me |\n";
        assert!(matches!(
            parse(md),
            Err(AttributionError::EmptyField {
                column: "licence",
                ..
            })
        ));
    }

    #[test]
    fn rejects_wrong_column_count() {
        let md = "| a | b | c | d |\n|---|---|---|---|\n| x.png | src |\n";
        assert!(matches!(parse(md), Err(AttributionError::BadRow { .. })));
    }

    #[test]
    fn detects_missing_and_orphaned_rows() {
        let dir = std::env::temp_dir().join(format!("rw-assets-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/a.svg"), "<svg/>").unwrap();
        fs::write(
            dir.join(ATTRIBUTION_FILE),
            "| p | s | l | a |\n|-|-|-|-|\n| gone.png | s | l | a |\n",
        )
        .unwrap();
        match check(&dir) {
            Err(AttributionError::OutOfSync { missing, orphaned }) => {
                assert_eq!(missing, vec!["sub/a.svg".to_owned()]);
                assert_eq!(orphaned, vec!["gone.png".to_owned()]);
            }
            other => panic!("unexpected result: {other:?}"),
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The real repository assets must stay in sync with their attribution file.
    #[test]
    fn repository_assets_are_attributed() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let rows = check(&assets).unwrap_or_else(|e| panic!("{e}"));
        assert!(!rows.is_empty());
    }
}
