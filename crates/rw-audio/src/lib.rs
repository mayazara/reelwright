//! Mixer, fades, gain, speed/time-stretch, meters and audio output.
//!
//! M2 provides the timeline mixer used by both playback and export, and
//! audio outputs that drive the playback clock.

mod mixer;
pub mod output;

pub use mixer::{Mixer, audible_items};
pub use rw_media::decode::{CHANNELS, SAMPLE_RATE};

use rw_model::Time;

/// Time of sample `n` at [`SAMPLE_RATE`].
pub fn sample_to_time(n: i64) -> Time {
    Time::from_rational(n, 1, SAMPLE_RATE as i32)
}

/// First sample at or after `t`.
pub fn time_to_sample(t: Time) -> i64 {
    let per = Time::from_rational(1, 1, SAMPLE_RATE as i32).ticks();
    t.ticks().div_euclid(per) + i64::from(t.ticks().rem_euclid(per) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_time_round_trip() {
        for n in [0i64, 1, 47_999, 48_000, 123_456_789] {
            assert_eq!(time_to_sample(sample_to_time(n)), n);
        }
        assert_eq!(time_to_sample(Time(1)), 1);
    }
}
