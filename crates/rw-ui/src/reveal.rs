//! Showing a file in the system's file manager.

use std::path::Path;
use std::process::Command;

/// Opens the folder holding `path`, with the file selected where the file
/// manager supports it.
pub fn reveal(path: &Path) -> std::io::Result<()> {
    if cfg!(target_os = "windows") {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path.as_os_str());
        Command::new("explorer").arg(arg).spawn()?;
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg("-R").arg(path).spawn()?;
    } else {
        let dir = path.parent().unwrap_or(path);
        Command::new("xdg-open").arg(dir).spawn()?;
    }
    Ok(())
}

/// Opens `path` in the program the system uses for it (a local file; a
/// licence page opens in the browser without going online).
pub fn open(path: &Path) -> std::io::Result<()> {
    if cfg!(target_os = "windows") {
        Command::new("explorer").arg(path).spawn()?;
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(path).spawn()?;
    } else {
        Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}

/// The licences folder shipped with the app: beside the exe (Windows), in
/// the app's Resources (macOS) or in the package's docs (Linux).
pub fn licences_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [
        dir.join("licenses"),
        dir.join("../Resources/licenses"),
        dir.join("../share/doc/reelwright/licenses"),
        dir.join("../../share/doc/reelwright/licenses"),
    ]
    .into_iter()
    .find(|d| d.is_dir())
}
