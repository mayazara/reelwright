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
