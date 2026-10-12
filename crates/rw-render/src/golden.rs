//! Comparing rendered frames with stored reference images (golden
//! frames), shared by the test suites of several crates.
//!
//! `RW_UPDATE_GOLDEN=1` rewrites the references instead of comparing;
//! `RW_GOLDEN_STATS=1` prints how close each frame came.

use rw_media::RgbaImage;
use std::path::Path;

/// Per-pixel tolerance: GPUs differ slightly in filtering and rounding.
pub const PIXEL_TOL: u8 = 6;
/// Share of pixels allowed beyond `PIXEL_TOL` (edges, rotated sampling).
pub const OUTLIER_SHARE: f64 = 0.01;
/// Largest average shift of any channel, in 8-bit levels.
pub const MEAN_TOL: f64 = 0.75;

/// Checks `got` against `dir/name.png` within the tolerances above.
pub fn compare(dir: &Path, name: &str, got: &RgbaImage) -> Result<(), String> {
    let path = dir.join(format!("{name}.png"));
    if std::env::var("RW_UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        got.save_png(&path).map_err(|e| e.to_string())?;
        return Ok(());
    }
    let want = RgbaImage::load_png(&path)
        .map_err(|e| format!("{name}: no golden image ({e}); see the module docs"))?;
    if (want.width, want.height) != (got.width, got.height) {
        return Err(format!(
            "{name}: size {}x{} vs {}x{}",
            got.width, got.height, want.width, want.height
        ));
    }
    let mut outliers = 0usize;
    let mut worst = 0u8;
    // Signed mean per channel catches small uniform shifts (a slightly
    // different grade) that stay under the per-pixel tolerance.
    let mut sum = [0i64; 3];
    for (a, b) in got.pixels.chunks(4).zip(want.pixels.chunks(4)) {
        let d = (0..3).map(|k| a[k].abs_diff(b[k])).max().unwrap_or(0);
        worst = worst.max(d);
        if d > PIXEL_TOL {
            outliers += 1;
        }
        for k in 0..3 {
            sum[k] += i64::from(a[k]) - i64::from(b[k]);
        }
    }
    let n = f64::from(got.width * got.height);
    let mean = sum.map(|s| s as f64 / n);
    let mean_abs = mean.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    if std::env::var("RW_GOLDEN_STATS").is_ok() {
        eprintln!("{name}: worst {worst}, outliers {outliers}, mean shift {mean_abs:.3}");
    }
    if mean_abs > MEAN_TOL {
        return Err(format!(
            "{name}: average colour shifted by {mean_abs:.2} levels (allowed {MEAN_TOL})"
        ));
    }
    let share = outliers as f64 / n;
    if share > OUTLIER_SHARE {
        let diff = dir.join(format!("{name}.actual.png"));
        let _ = got.save_png(&diff);
        return Err(format!(
            "{name}: {:.2} % of pixels differ by more than {PIXEL_TOL} (worst {worst}); actual saved to {}",
            share * 100.0,
            diff.display()
        ));
    }
    Ok(())
}
