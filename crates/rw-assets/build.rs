//! Embeds the bundled sticker pack: one `include_bytes!` per SVG, so the
//! app is a single file and needs no asset folder at run time.

use std::fmt::Write;
use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/stickers/openmoji");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("assets/stickers/openmoji")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "svg"))
        .collect();
    // By id (the stem), the order lookups binary-search in.
    files.sort_by_key(|p| p.file_stem().map(|s| s.to_os_string()));
    let mut out = String::from(
        "/// Bundled sticker SVGs by id.\npub static OPENMOJI: &[(&str, &[u8])] = &[\n",
    );
    for f in &files {
        let id = f.file_stem().unwrap().to_string_lossy();
        let path = f.canonicalize().unwrap();
        writeln!(
            out,
            "    ({id:?}, include_bytes!({:?})),",
            path.display().to_string()
        )
        .unwrap();
    }
    out.push_str("];\n");
    let index = dir.join("index.json").canonicalize().unwrap();
    writeln!(
        out,
        "/// Names, groups and tags of the bundled stickers.\npub static OPENMOJI_INDEX: &str = include_str!({:?});",
        index.display().to_string()
    )
    .unwrap();
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("openmoji.rs");
    std::fs::write(dest, out).unwrap();
}
