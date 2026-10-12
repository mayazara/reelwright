//! What the sound looks like at a moment: loudness per frequency band and
//! the wave itself, for audio visualisers.

use crate::{CHANNELS, SAMPLE_RATE};

/// Samples analysed per frame (≈ 85 ms at 48 kHz).
pub const WINDOW: usize = 4096;

/// Levels for drawing: `bands` are 0–1 from low to high frequencies;
/// `wave` is the mono signal, −1–1, sampled across the window's middle.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Levels {
    pub bands: Vec<f32>,
    pub wave: Vec<f32>,
}

/// In-place radix-2 FFT of `re`/`im` (length a power of two).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                (cr, ci) = (cr * wr - ci * wi, cr * wi + ci * wr);
            }
        }
        len <<= 1;
    }
}

/// Analyses interleaved stereo `samples` (`WINDOW` frames) into `bands`
/// log-spaced bands between 40 Hz and 16 kHz and `wave_points` wave points.
/// `sensitivity` scales the response.
pub fn analyse(samples: &[f32], bands: usize, wave_points: usize, sensitivity: f32) -> Levels {
    let n = WINDOW;
    let mono: Vec<f32> = (0..n)
        .map(|i| {
            samples
                .get(i * CHANNELS..i * CHANNELS + CHANNELS)
                .map_or(0.0, |f| f.iter().sum::<f32>() / CHANNELS as f32)
        })
        .collect();
    // Hann window, then the spectrum.
    let mut re: Vec<f32> = mono
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
            v * w
        })
        .collect();
    let mut im = vec![0.0f32; n];
    fft(&mut re, &mut im);
    let hz_per_bin = SAMPLE_RATE as f32 / n as f32;
    let (lo, hi) = (40.0f32, 16_000.0f32);
    let mut out = Vec::with_capacity(bands);
    for b in 0..bands {
        let f0 = lo * (hi / lo).powf(b as f32 / bands as f32);
        let f1 = lo * (hi / lo).powf((b + 1) as f32 / bands as f32);
        let (k0, k1) = (
            (f0 / hz_per_bin).floor() as usize,
            ((f1 / hz_per_bin).ceil() as usize).max((f0 / hz_per_bin).floor() as usize + 1),
        );
        let peak = (k0..k1.min(n / 2))
            .map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt())
            .fold(0.0f32, f32::max);
        // Amplitude relative to a full-scale sine (Hann gain 0.5), in dB,
        // mapped so −60 dB … 0 dB spans 0 … 1.
        let amp = peak / (n as f32 * 0.25);
        let db = 20.0 * amp.max(1e-6).log10();
        out.push(((db + 60.0) / 60.0 * sensitivity).clamp(0.0, 1.0));
    }
    // The middle quarter (≈ 21 ms): a few cycles of what's playing.
    let span = n / 4;
    let wave = (0..wave_points)
        .map(|i| {
            let k = n * 3 / 8 + i * span / wave_points.max(1);
            (mono[k] * 2.0 * sensitivity).clamp(-1.0, 1.0)
        })
        .collect();
    Levels { bands: out, wave }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, amp: f32) -> Vec<f32> {
        (0..WINDOW)
            .flat_map(|i| {
                let v = (std::f32::consts::TAU * freq * i as f32 / SAMPLE_RATE as f32).sin() * amp;
                [v, v]
            })
            .collect()
    }

    #[test]
    fn a_tone_lights_its_band() {
        let l = analyse(&tone(1000.0, 0.5), 32, 64, 1.0);
        let loudest = l
            .bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        // 1 kHz sits about 55 % of the way along a 40 Hz–16 kHz log scale.
        let expected = ((1000.0f32 / 40.0).ln() / (400.0f32).ln() * 32.0) as usize;
        assert!(
            loudest.abs_diff(expected) <= 1,
            "band {loudest}, want {expected}"
        );
        assert!(l.bands[loudest] > 0.8);
        assert!(l.bands[2] < 0.3, "low bands stay low: {}", l.bands[2]);
        assert_eq!(l.wave.len(), 64);
    }

    #[test]
    fn silence_is_flat_and_louder_is_higher() {
        let quiet = analyse(&vec![0.0; WINDOW * 2], 16, 8, 1.0);
        assert!(quiet.bands.iter().all(|b| *b == 0.0));
        let soft = analyse(&tone(440.0, 0.05), 16, 8, 1.0);
        let loud = analyse(&tone(440.0, 0.5), 16, 8, 1.0);
        let max = |l: &Levels| l.bands.iter().fold(0.0f32, |a, b| a.max(*b));
        assert!(max(&loud) > max(&soft));
    }
}
