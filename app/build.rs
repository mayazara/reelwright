//! Embeds the icon and version details in the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=../assets/branding/reelwright.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../assets/branding/reelwright.ico")
            .set("ProductName", "Reelwright")
            .set("FileDescription", "Reelwright video editor")
            .set("CompanyName", "Reelwright contributors")
            .set("LegalCopyright", "MIT OR Apache-2.0");
        if let Err(e) = res.compile() {
            // A missing resource compiler shouldn't stop the build; the app
            // just has the default icon.
            println!("cargo:warning=could not embed Windows resources: {e}");
        }
    }
}
