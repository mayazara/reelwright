//! Project model: project, tracks, items, keyframes, serde schema and migrations.
//!
//! Everything here is plain data. Changes go through `rw-edit` commands so
//! they can be undone; I/O is limited to reading and writing project folders.

pub mod file;
pub mod ids;
pub mod library;
pub mod media;
pub mod project;
pub mod time;
pub mod visual;

pub use ids::{GroupId, ItemId, MarkerId, MediaId, ProjectId, TrackId};
pub use media::{AudioInfo, ContentHash, MediaInfo, MediaKind, MediaRef, VideoInfo};
pub use project::{
    AspectPreset, Canvas, IMAGE_DURATION, Item, ItemKind, MAX_GAIN, Marker, Project, Rgba, Track,
    TrackKind, default_duration, max_duration, track_kind_for,
};
pub use time::{FrameRate, TICKS_PER_SECOND, Time};
pub use visual::{
    BlendMode, ColorAdjust, ColorControl, Crop, Filter, Fit, Generated, Layout, Look, Transform,
};

/// Product identity. The name is a placeholder; changing it here renames the app.
pub mod app {
    /// Human-readable product name shown in window titles and dialogs.
    pub const NAME: &str = "Reelwright";
    /// Lower-case identifier used for config/data directories and file names.
    pub const ID: &str = "reelwright";
    /// Reverse-DNS qualifier used by platform directory lookups.
    pub const QUALIFIER: &str = "app";
    /// Organisation component used by platform directory lookups.
    pub const ORGANIZATION: &str = "Reelwright";
    /// Crate version, shared by every workspace member.
    pub const VERSION: &str = env!("CARGO_PKG_VERSION");
}

#[cfg(test)]
mod tests {
    use super::app;

    #[test]
    fn id_is_lowercase_name() {
        assert_eq!(app::ID, app::NAME.to_lowercase());
    }
}
