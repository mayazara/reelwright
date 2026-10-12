//! Effects, filters, transitions and LUTs with parameter schemas.
//!
//! M3 provides the bundled LUT filters and the `.cube` reader; M4 adds the
//! effect and transition catalogue.

pub mod effects;
pub mod grades;
pub mod lut;

pub use lut::{CubeError, Lut};

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// Grid size of the bundled tables.
pub const LUT_SIZE: usize = 17;

/// A bundled filter.
#[derive(Debug, Clone)]
pub struct FilterInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub lut: Arc<Lut>,
}

macro_rules! bundled {
    ($($id:literal),* $(,)?) => {
        &[$(($id, include_str!(concat!("../../../assets/luts/", $id, ".cube")))),*]
    };
}

/// The `.cube` files shipped with the app, by id.
const BUNDLED: &[(&str, &str)] = bundled!(
    "mono",
    "mono-punch",
    "silver",
    "noir",
    "sepia",
    "warm",
    "cool",
    "golden-hour",
    "blue-hour",
    "harbor",
    "ember",
    "faded-film",
    "vintage",
    "retro",
    "pastel",
    "vivid",
    "muted",
    "matte",
    "forest",
    "desert",
    "arctic",
    "neon",
    "moonlight",
    "rose",
);

/// All bundled filters, in display order. Parsed once.
pub fn filters() -> &'static [FilterInfo] {
    static FILTERS: OnceLock<Vec<FilterInfo>> = OnceLock::new();
    FILTERS.get_or_init(|| {
        let names: HashMap<&str, &str> = grades::GRADES.iter().map(|g| (g.id, g.name)).collect();
        BUNDLED
            .iter()
            .map(|(id, text)| FilterInfo {
                id,
                name: names.get(id).copied().unwrap_or(id),
                lut: Arc::new(Lut::parse(text).unwrap_or_else(|e| panic!("bundled LUT {id}: {e}"))),
            })
            .collect()
    })
}

pub fn filter(id: &str) -> Option<&'static FilterInfo> {
    filters().iter().find(|f| f.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_files_match_their_grades() {
        assert_eq!(filters().len(), grades::GRADES.len());
        for g in grades::GRADES {
            let f = filter(g.id).unwrap_or_else(|| panic!("{} not bundled", g.id));
            assert_eq!(f.lut.size, LUT_SIZE);
            // The shipped file is what the grade function produces (so the
            // generator was re-run after any edit).
            let fresh = Lut::from_fn(LUT_SIZE, g.apply);
            for (a, b) in fresh.data.iter().zip(&f.lut.data) {
                for k in 0..3 {
                    assert!(
                        (a[k] - b[k]).abs() < 1e-4,
                        "{}: regenerate with `cargo run -p rw-fx --example make_luts`",
                        g.id
                    );
                }
            }
        }
    }
}
