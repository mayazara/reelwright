//! Project model: project, tracks, items, keyframes, serde schema and migrations.
//!
//! The data types land in M1 (see `PLAN.md`). For now this crate owns the
//! product identity so the name lives in exactly one place.

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
