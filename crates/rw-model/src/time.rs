//! Exact time arithmetic.
//!
//! Time is an integer tick count at [`TICKS_PER_SECOND`], chosen so that every
//! common frame duration (23.976, 24, 25, 29.97, 30, 50, 59.94, 60 fps) and
//! sample duration (44.1, 48, 96 kHz) is a whole number of ticks.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// Ticks in one second (2^9 · 3^2 · 5^5 · 7^2), divisible by 24000, 30000,
/// 25, 50, 60000 and 44100/48000/96000.
pub const TICKS_PER_SECOND: i64 = 705_600_000;

/// A point or span on a timeline, in ticks.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Time(pub i64);

impl Time {
    pub const ZERO: Time = Time(0);

    pub const fn from_ticks(ticks: i64) -> Self {
        Time(ticks)
    }

    pub const fn ticks(self) -> i64 {
        self.0
    }

    pub const fn from_secs(secs: i64) -> Self {
        Time(secs * TICKS_PER_SECOND)
    }

    pub const fn from_millis(ms: i64) -> Self {
        Time(ms * (TICKS_PER_SECOND / 1000))
    }

    /// Nearest tick to a floating-point number of seconds. Only for UI input
    /// and media probing; internal arithmetic stays in ticks.
    pub fn from_secs_f64(secs: f64) -> Self {
        Time((secs * TICKS_PER_SECOND as f64).round() as i64)
    }

    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / TICKS_PER_SECOND as f64
    }

    /// Converts `value` expressed in units of `num/den` seconds (an FFmpeg
    /// time base) to ticks, rounding to nearest.
    pub fn from_rational(value: i64, num: i32, den: i32) -> Self {
        let n = i128::from(value) * i128::from(num) * i128::from(TICKS_PER_SECOND);
        let d = i128::from(den);
        Time(div_round(n, d) as i64)
    }

    /// Converts to units of `num/den` seconds, rounding to nearest.
    pub fn to_rational(self, num: i32, den: i32) -> i64 {
        let n = i128::from(self.0) * i128::from(den);
        let d = i128::from(num) * i128::from(TICKS_PER_SECOND);
        div_round(n, d) as i64
    }

    pub fn max(self, other: Time) -> Time {
        Time(self.0.max(other.0))
    }

    pub fn min(self, other: Time) -> Time {
        Time(self.0.min(other.0))
    }
}

fn div_round(n: i128, d: i128) -> i128 {
    let (q, r) = (n / d, n % d);
    if 2 * r.abs() >= d.abs() {
        q + if (n < 0) == (d < 0) { 1 } else { -1 }
    } else {
        q
    }
}

impl Add for Time {
    type Output = Time;
    fn add(self, rhs: Time) -> Time {
        Time(self.0 + rhs.0)
    }
}

impl Sub for Time {
    type Output = Time;
    fn sub(self, rhs: Time) -> Time {
        Time(self.0 - rhs.0)
    }
}

impl AddAssign for Time {
    fn add_assign(&mut self, rhs: Time) {
        self.0 += rhs.0;
    }
}

impl SubAssign for Time {
    fn sub_assign(&mut self, rhs: Time) {
        self.0 -= rhs.0;
    }
}

impl Neg for Time {
    type Output = Time;
    fn neg(self) -> Time {
        Time(-self.0)
    }
}

impl fmt::Display for Time {
    /// `h:mm:ss.mmm`, or `m:ss.mmm` under an hour.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let ms_total = div_round(
            i128::from(self.0.abs()) * 1000,
            i128::from(TICKS_PER_SECOND),
        ) as i64;
        let (h, rem) = (ms_total / 3_600_000, ms_total % 3_600_000);
        let (m, rem) = (rem / 60_000, rem % 60_000);
        let (s, ms) = (rem / 1000, rem % 1000);
        if h > 0 {
            write!(f, "{sign}{h}:{m:02}:{s:02}.{ms:03}")
        } else {
            write!(f, "{sign}{m}:{s:02}.{ms:03}")
        }
    }
}

/// A frame rate as an exact fraction, e.g. 30000/1001.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameRate {
    pub num: u32,
    pub den: u32,
}

impl FrameRate {
    pub const FPS_24: FrameRate = FrameRate::new(24, 1);
    pub const FPS_25: FrameRate = FrameRate::new(25, 1);
    pub const FPS_30: FrameRate = FrameRate::new(30, 1);
    pub const FPS_60: FrameRate = FrameRate::new(60, 1);

    pub const fn new(num: u32, den: u32) -> Self {
        FrameRate { num, den }
    }

    pub fn as_f64(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }

    /// Duration of one frame. Exact for all common rates.
    pub fn frame_duration(self) -> Time {
        Time(div_round(
            i128::from(TICKS_PER_SECOND) * i128::from(self.den),
            i128::from(self.num),
        ) as i64)
    }

    /// Start time of frame `n`.
    pub fn frame_to_time(self, n: i64) -> Time {
        Time(div_round(
            i128::from(n) * i128::from(TICKS_PER_SECOND) * i128::from(self.den),
            i128::from(self.num),
        ) as i64)
    }

    /// Index of the frame showing at time `t` (floor).
    pub fn time_to_frame(self, t: Time) -> i64 {
        let n = i128::from(t.0) * i128::from(self.num);
        let d = i128::from(TICKS_PER_SECOND) * i128::from(self.den);
        n.div_euclid(d) as i64
    }
}

impl Default for FrameRate {
    fn default() -> Self {
        FrameRate::FPS_30
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn common_rates_have_whole_tick_frames() {
        for (num, den) in [
            (24000, 1001),
            (24, 1),
            (25, 1),
            (30000, 1001),
            (30, 1),
            (50, 1),
            (60000, 1001),
            (60, 1),
        ] {
            assert_eq!((TICKS_PER_SECOND * den) % num, 0, "{num}/{den}");
        }
        for rate in [8000, 16000, 22050, 32000, 44100, 48000, 96000] {
            assert_eq!(TICKS_PER_SECOND % rate, 0, "{rate} Hz");
        }
    }

    #[test]
    fn display_formats() {
        assert_eq!(Time::from_millis(61_250).to_string(), "1:01.250");
        assert_eq!(Time::from_secs(3723).to_string(), "1:02:03.000");
        assert_eq!((-Time::from_millis(500)).to_string(), "-0:00.500");
    }

    #[test]
    fn rational_conversion() {
        // 90 kHz MPEG time base.
        assert_eq!(Time::from_rational(90_000, 1, 90_000), Time::from_secs(1));
        assert_eq!(Time::from_secs(2).to_rational(1, 48_000), 96_000);
    }

    proptest! {
        #[test]
        fn frame_round_trip(n in 0i64..10_000_000, which in 0usize..4) {
            let rate = [FrameRate::new(30000, 1001), FrameRate::FPS_25, FrameRate::FPS_60, FrameRate::new(24000, 1001)][which];
            prop_assert_eq!(rate.time_to_frame(rate.frame_to_time(n)), n);
        }

        #[test]
        fn rational_round_trip(ticks in -1_000_000_000_000i64..1_000_000_000_000) {
            let t = Time(ticks);
            let back = Time::from_rational(t.to_rational(1, 48_000), 1, 48_000);
            prop_assert!((back - t).ticks().abs() <= TICKS_PER_SECOND / 48_000 / 2);
        }
    }
}
