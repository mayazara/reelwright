//! Typed identifiers. Each wraps a random UUID so ids never collide across
//! projects that are merged or copied.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_type!(
    /// Identifies a project.
    ProjectId
);
id_type!(
    /// Identifies an imported media file within a project.
    MediaId
);
id_type!(
    /// Identifies a track.
    TrackId
);
id_type!(
    /// Identifies a timeline item.
    ItemId
);
id_type!(
    /// Identifies a group of linked items.
    GroupId
);
id_type!(
    /// Identifies a timeline marker.
    MarkerId
);
