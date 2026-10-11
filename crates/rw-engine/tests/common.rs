//! Shared helpers for integration tests.

use rw_media::fixtures::{self, Generated};

/// Generates fixtures and checks enough exist. CI sets `RW_MIN_FIXTURES`
/// so a runner whose `ffmpeg` lacks encoders cannot silently pass with a
/// thin matrix.
pub fn fixtures() -> Generated {
    let g = fixtures::ensure();
    for (name, why) in &g.skipped {
        eprintln!("fixture {name} skipped: {why}");
    }
    if let Some(min) = std::env::var("RW_MIN_FIXTURES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        assert!(
            g.available.len() >= min,
            "only {} fixtures available, need {min}; skipped: {:?}",
            g.available.len(),
            g.skipped
        );
    }
    assert!(
        !g.available.is_empty(),
        "no fixtures could be generated; is the ffmpeg CLI installed?"
    );
    g
}
