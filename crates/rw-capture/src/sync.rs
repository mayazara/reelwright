//! Keeping sound and pictures in step over long recordings.
//!
//! Pictures are stamped with the recording clock when they arrive. Sound
//! arrives as a stream whose rate is set by the sound card's own crystal,
//! which runs slightly fast or slow (tens of parts per million — over a
//! minute in a day). Stamping sound by counting samples would slowly slide
//! it away from the pictures, so [`AudioFollower`] compares the sample
//! count with the clock and drops or adds a few samples whenever they
//! differ by more than a few milliseconds.

use std::time::Duration;

/// Samples a second.
pub const RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;

/// Largest difference allowed before correcting (10 ms; under a third of
/// a frame at 30 fps).
pub const TOLERANCE: i64 = RATE as i64 / 100;

/// Decides how much of each arriving chunk of sound to keep.
#[derive(Debug, Default)]
pub struct AudioFollower {
    /// Sample frames written so far.
    written: i64,
    /// Corrections made, for diagnostics: added and dropped sample frames.
    pub added: i64,
    pub dropped: i64,
}

impl AudioFollower {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sample frames written so far.
    pub fn written(&self) -> i64 {
        self.written
    }

    /// Takes a chunk (interleaved stereo) whose first sample was captured
    /// at recording time `at`, and returns the samples to write: silence is
    /// put before it if the file has fallen behind the clock, and its start
    /// is cut if the file has run ahead.
    pub fn take(&mut self, at: Duration, chunk: &[f32]) -> Vec<f32> {
        let frames = (chunk.len() / CHANNELS) as i64;
        let expected = (at.as_secs_f64() * f64::from(RATE)).round() as i64;
        let ahead = self.written - expected;
        let mut out = Vec::with_capacity(chunk.len() + CHANNELS * TOLERANCE as usize);
        if ahead < -TOLERANCE {
            // Behind: fill the gap with silence (a pause, a dropout, or a
            // slow sound card).
            let gap = (-ahead) as usize;
            out.resize(gap * CHANNELS, 0.0);
            self.added += gap as i64;
            out.extend_from_slice(chunk);
        } else if ahead > TOLERANCE {
            // Ahead: skip the overlap (a fast sound card).
            let skip = ahead.min(frames) as usize;
            self.dropped += skip as i64;
            out.extend_from_slice(&chunk[skip * CHANNELS..]);
        } else {
            out.extend_from_slice(chunk);
        }
        self.written += (out.len() / CHANNELS) as i64;
        out
    }
}

/// Which frame slot a picture arriving at recording time `at` fills, at
/// `fps` frames a second.
pub fn frame_index(at: Duration, fps: f64) -> i64 {
    (at.as_secs_f64() * fps).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sound card running `ppm` parts per million fast, delivering
    /// `block`-frame chunks stamped by the true clock, for `secs` seconds.
    fn run(ppm: f64, secs: u64, block: usize) -> AudioFollower {
        let mut f = AudioFollower::new();
        let card_rate = f64::from(RATE) * (1.0 + ppm / 1e6);
        let chunk = vec![0.25f32; block * CHANNELS];
        let mut produced = 0usize;
        while (produced as f64) < card_rate * secs as f64 {
            // When the card's sample `produced` was captured, by the clock.
            let at = Duration::from_secs_f64(produced as f64 / card_rate);
            f.take(at, &chunk);
            produced += block;
            // Check the file never drifts beyond the tolerance (+ one block
            // of jitter) from the clock.
            let now = (produced as f64 / card_rate * f64::from(RATE)) as i64;
            assert!(
                (f.written() - now).abs() <= TOLERANCE + block as i64,
                "drifted {} samples at {:.0} s",
                f.written() - now,
                produced as f64 / card_rate
            );
        }
        f
    }

    #[test]
    fn thirty_minutes_of_drifting_sound_stays_in_step() {
        // ±100 ppm is a poor sound card; 30 min would drift 180 ms unchecked.
        let fast = run(100.0, 1800, 480);
        assert!(fast.dropped > 0 && fast.added == 0);
        let slow = run(-100.0, 1800, 480);
        assert!(slow.added > 0 && slow.dropped == 0);
        // Corrections are a tiny share of the sound.
        assert!(fast.dropped < 1800 * 48 / 4);
    }

    #[test]
    fn a_gap_is_filled_with_silence() {
        let mut f = AudioFollower::new();
        let chunk = vec![1.0f32; 480 * CHANNELS];
        f.take(Duration::ZERO, &chunk);
        // The next chunk arrives a second later (the card stalled).
        let out = f.take(Duration::from_secs(1), &chunk);
        assert_eq!(out.len() / CHANNELS, 48_000 - 480 + 480);
        assert!(out[..CHANNELS].iter().all(|s| *s == 0.0));
        assert_eq!(f.written(), 48_480);
    }

    #[test]
    fn frames_land_in_their_slots() {
        assert_eq!(frame_index(Duration::from_millis(0), 30.0), 0);
        assert_eq!(frame_index(Duration::from_millis(34), 30.0), 1);
        assert_eq!(frame_index(Duration::from_secs(60), 30.0), 1800);
    }
}
