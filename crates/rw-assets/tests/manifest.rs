//! `assets/manifest.json` lists every bundled asset with its size and
//! BLAKE3 checksum, versioned with the app. The assets are compiled into
//! the executable, so this pins exactly what each release ships; the test
//! fails when an asset changes without the manifest. Regenerate with
//! `RW_UPDATE_MANIFEST=1 cargo test -p rw-assets --test manifest`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn assets_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn files(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, root, out);
        } else {
            out.push(p.strip_prefix(root).unwrap().to_path_buf());
        }
    }
}

fn manifest() -> String {
    let root = assets_dir();
    let mut all = Vec::new();
    files(&root, &root, &mut all);
    let mut entries = BTreeMap::new();
    for rel in all {
        let key = rel.to_string_lossy().replace('\\', "/");
        if key == "manifest.json" {
            continue;
        }
        let bytes = std::fs::read(root.join(&rel)).unwrap();
        entries.insert(
            key,
            serde_json::json!({
                "bytes": bytes.len(),
                "blake3": blake3::hash(&bytes).to_hex().to_string(),
            }),
        );
    }
    let doc = serde_json::json!({
        "comment": "Every bundled asset with its size and BLAKE3 checksum. Regenerate: RW_UPDATE_MANIFEST=1 cargo test -p rw-assets --test manifest",
        "files": entries,
    });
    serde_json::to_string_pretty(&doc).unwrap() + "\n"
}

#[test]
fn manifest_matches_the_assets() {
    let path = assets_dir().join("manifest.json");
    let want = manifest();
    if std::env::var_os("RW_UPDATE_MANIFEST").is_some() {
        std::fs::write(&path, &want).unwrap();
        return;
    }
    let have = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        have == want,
        "assets/manifest.json is out of date; regenerate it with \
         RW_UPDATE_MANIFEST=1 cargo test -p rw-assets --test manifest"
    );
}
