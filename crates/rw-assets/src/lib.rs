//! Bundled asset library index, brand kit and templates.
//!
//! The attribution check keeps `assets/ATTRIBUTION.md` in sync with the
//! files under `assets/`; `stickers` and `raster` provide the graphics.

pub mod attribution;
pub mod brand;
pub mod raster;
pub mod sounds;
pub mod stickers;

/// The attribution table for every bundled asset, as shipped.
pub const ATTRIBUTION: &str = include_str!("../../../assets/ATTRIBUTION.md");

/// Short credits for the About window: bundled third-party content.
pub const CREDITS: &[(&str, &str)] = &[
    (
        "Emoji and stickers",
        "OpenMoji — the open-source emoji and icon project. Licence: CC BY-SA 4.0 (openmoji.org).",
    ),
    (
        "Fonts",
        "Inter, Noto Sans Arabic and Devanagari, and the title fonts (Montserrat, Lora, Playfair Display and others) are under the SIL Open Font Licence 1.1; see the list below for each family's authors.",
    ),
    (
        "Video and audio",
        "FFmpeg, under the GNU LGPL 2.1, as replaceable shared libraries; its exact configuration ships with the app.",
    ),
    (
        "Code",
        "Rust libraries under MIT, Apache-2.0, BSD, Zlib, ISC and MPL-2.0 licences; see THIRD_PARTY_LICENSES.html beside the app.",
    ),
];
