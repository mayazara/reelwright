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

/// How far ahead [`AudioFollower`] looks before placing a chunk (½ s).
pub const WINDOW: i64 = RATE as i64 / 2;

/// Places arriving chunks of sound on the recording's clock.
///
/// A chunk's stamp says when its first sample was captured, worked out
/// from when it was delivered. Delivery can only be late (the system was
/// busy, then a burst catches up), never early. So each chunk is held for
/// [`WINDOW`] and placed by the least-late of it and the chunks that
/// followed: steady drift and real gaps are followed, scheduling jitter is
/// ignored. Sound comes out [`WINDOW`] behind; [`AudioFollower::flush`]
/// returns the rest at the end.
#[derive(Debug, Default)]
pub struct AudioFollower {
    /// Sample frames given out so far.
    written: i64,
    /// Held chunks: where their stamps put them, and their samples.
    pending: std::collections::VecDeque<(i64, Vec<f32>)>,
    /// Corrections made, for diagnostics: added and dropped sample frames.
    pub added: i64,
    pub dropped: i64,
}

impl AudioFollower {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sample frames given out so far.
    pub fn written(&self) -> i64 {
        self.written
    }

    /// Takes a chunk (interleaved stereo) whose first sample was captured
    /// at recording time `at`, and returns the sound now ready: silence is
    /// put in where the file has fallen behind the clock, and sound cut
    /// where it has run ahead.
    pub fn take(&mut self, at: Duration, chunk: &[f32]) -> Vec<f32> {
        let expected = (at.as_secs_f64() * f64::from(RATE)).round() as i64;
        self.pending.push_back((expected, chunk.to_vec()));
        let mut out = Vec::new();
        while self
            .pending
            .front()
            .is_some_and(|(e, _)| *e + WINDOW <= expected)
        {
            self.release(&mut out);
        }
        out
    }

    /// The sound still held (at the end of a recording).
    pub fn flush(&mut self) -> Vec<f32> {
        let mut out = Vec::new();
        while !self.pending.is_empty() {
            self.release(&mut out);
        }
        out
    }

    /// Places the oldest held chunk.
    fn release(&mut self, out: &mut Vec<f32>) {
        let Some(&(first, _)) = self.pending.front() else {
            return;
        };
        // How far ahead of the clock the file is, by the least-late chunk
        // in the window (each placed right after the one before).
        let mut pos = self.written;
        let mut ahead = i64::MIN;
        for (e, c) in &self.pending {
            if *e > first + WINDOW {
                break;
            }
            ahead = ahead.max(pos - e);
            pos += (c.len() / CHANNELS) as i64;
        }
        let (_, chunk) = self.pending.pop_front().expect("checked above");
        let frames = (chunk.len() / CHANNELS) as i64;
        let start = out.len();
        if ahead < -TOLERANCE {
            // Behind: fill the gap with silence (a dropout, or a slow
            // sound card).
            let gap = -ahead;
            out.resize(start + gap as usize * CHANNELS, 0.0);
            self.added += gap;
            out.extend_from_slice(&chunk);
        } else if ahead > TOLERANCE {
            // Ahead (a fast sound card): cut back to the middle of the
            // tolerance, a little at a time (at most half a chunk, from its
            // end), so each cut is short.
            let skip = (ahead - TOLERANCE / 2).min(frames / 2);
            self.dropped += skip;
            out.extend_from_slice(&chunk[..(frames - skip) as usize * CHANNELS]);
        } else {
            out.extend_from_slice(&chunk);
        }
        self.written += ((out.len() - start) / CHANNELS) as i64;
    }

    /// Silence up to recording time `at`, for an input that has said
    /// nothing for a while (computer sound sends nothing while nothing
    /// plays).
    pub fn fill_to(&mut self, at: Duration) -> Vec<f32> {
        let target = (at.as_secs_f64() * f64::from(RATE)).round() as i64;
        let gap = target - self.written;
        if !self.pending.is_empty() || gap <= TOLERANCE {
            return Vec::new();
        }
        self.added += gap;
        self.written += gap;
        vec![0.0; gap as usize * CHANNELS]
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
            // The file never drifts beyond the tolerance (+ a block either
            // way) from the clock, a window behind.
            let now = (produced as f64 / card_rate * f64::from(RATE)) as i64 - WINDOW;
            assert!(
                f.written() == 0 || (f.written() - now).abs() <= TOLERANCE + 2 * block as i64,
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
    fn a_gap_is_filled_with_silence_where_it_happened() {
        let mut f = AudioFollower::new();
        let chunk = vec![1.0f32; 480 * CHANNELS];
        let mut out = Vec::new();
        let mut at = 0i64;
        while at < 48_000 * 3 {
            // The card lost the second second of sound.
            if !(48_000..96_000).contains(&at) {
                out.extend(f.take(Duration::from_secs_f64(at as f64 / 48_000.0), &chunk));
            }
            at += 480;
        }
        out.extend(f.flush());
        assert_eq!(f.written(), at);
        assert_eq!(f.added, 48_000);
        // Sound, then a second of silence exactly where it was lost.
        let frame = |i: usize| out[i * CHANNELS];
        assert_eq!(frame(47_999), 1.0);
        assert!((48_000..96_000).all(|i| frame(i) == 0.0));
        assert_eq!(frame(96_000), 1.0);
    }

    /// Chunks delivered late now and then (the system was busy), then in a
    /// burst — even right at the start: nothing is added or dropped.
    #[test]
    fn late_delivery_is_not_drift() {
        let mut f = AudioFollower::new();
        let chunk = vec![0.25f32; 480 * CHANNELS];
        let mut produced = 0i64;
        let mut n = 0u64;
        while produced < 48_000 * 60 {
            // Every 50 chunks the system stalls 120 ms; the backlog is then
            // delivered at once, each stamped as if captured just now.
            let k = n % 50;
            let late = if k < 12 { 0.12 - k as f64 * 0.01 } else { 0.0 };
            let true_at = produced as f64 / 48_000.0;
            f.take(Duration::from_secs_f64(true_at + late), &chunk);
            produced += 480;
            n += 1;
        }
        f.flush();
        assert_eq!((f.added, f.dropped), (0, 0));
        assert_eq!(f.written(), produced);
    }

    #[test]
    fn quiet_input_is_filled() {
        let mut f = AudioFollower::new();
        assert!(f.fill_to(Duration::from_millis(5)).is_empty());
        assert_eq!(f.fill_to(Duration::from_secs(1)).len(), 48_000 * CHANNELS);
        // Sound arriving after that carries on in step.
        let chunk = vec![1.0f32; 480 * CHANNELS];
        f.take(Duration::from_secs(1), &chunk);
        assert_eq!(f.flush().len(), chunk.len());
        assert_eq!(f.written(), 48_480);
    }

    #[test]
    fn frames_land_in_their_slots() {
        assert_eq!(frame_index(Duration::from_millis(0), 30.0), 0);
        assert_eq!(frame_index(Duration::from_millis(34), 30.0), 1);
        assert_eq!(frame_index(Duration::from_secs(60), 30.0), 1800);
    }
}
