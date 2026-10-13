//! Recording time: one clock for every source, which stops while paused.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Where the time comes from: the system clock, or a hand-set clock in tests.
pub trait TimeSource: Send + Sync {
    fn now(&self) -> Duration;
}

/// The system's monotonic clock, from when it was made.
#[derive(Debug)]
pub struct SystemTime(Instant);

impl SystemTime {
    pub fn new() -> Self {
        SystemTime(Instant::now())
    }
}

impl Default for SystemTime {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeSource for SystemTime {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

/// A clock tests move by hand.
#[derive(Debug, Default)]
pub struct ManualTime(Mutex<Duration>);

impl ManualTime {
    pub fn set(&self, t: Duration) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = t;
    }

    pub fn advance(&self, d: Duration) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) += d;
    }
}

impl TimeSource for ManualTime {
    fn now(&self) -> Duration {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Debug, Default)]
struct State {
    start: Option<Duration>,
    paused_at: Option<Duration>,
    paused_total: Duration,
}

/// Time into the recording: zero at start, frozen while paused, and
/// continuing (without a jump) after resume.
pub struct RecClock<T: TimeSource> {
    time: T,
    state: Mutex<State>,
}

impl<T: TimeSource> RecClock<T> {
    pub fn new(time: T) -> Self {
        RecClock {
            time,
            state: Mutex::new(State::default()),
        }
    }

    pub fn source(&self) -> &T {
        &self.time
    }

    pub fn start(&self) {
        let now = self.time.now();
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = State {
            start: Some(now),
            ..State::default()
        };
    }

    pub fn pause(&self) {
        let now = self.time.now();
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.paused_at.is_none() {
            s.paused_at = Some(now);
        }
    }

    pub fn resume(&self) {
        let now = self.time.now();
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = s.paused_at.take() {
            s.paused_total += now.saturating_sub(at);
        }
    }

    pub fn is_paused(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .paused_at
            .is_some()
    }

    /// Recording time of the system moment `at` (from [`TimeSource::now`]),
    /// or `None` before the start or while paused.
    pub fn at(&self, at: Duration) -> Option<Duration> {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let start = s.start?;
        if s.paused_at.is_some() || at < start {
            return None;
        }
        Some(at.saturating_sub(start).saturating_sub(s.paused_total))
    }

    /// Recording time now (frozen while paused).
    pub fn elapsed(&self) -> Duration {
        let now = self.time.now();
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(start) = s.start else {
            return Duration::ZERO;
        };
        let end = s.paused_at.unwrap_or(now);
        end.saturating_sub(start).saturating_sub(s.paused_total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pauses_freeze_and_resume_continues() {
        let c = RecClock::new(ManualTime::default());
        c.source().set(Duration::from_secs(10));
        assert_eq!(c.at(Duration::from_secs(10)), None, "not started");
        c.start();
        c.source().advance(Duration::from_secs(3));
        assert_eq!(c.elapsed(), Duration::from_secs(3));
        c.pause();
        c.source().advance(Duration::from_secs(5));
        assert_eq!(c.elapsed(), Duration::from_secs(3), "frozen");
        assert_eq!(c.at(c.source().now()), None, "nothing kept while paused");
        c.resume();
        c.source().advance(Duration::from_secs(1));
        assert_eq!(c.elapsed(), Duration::from_secs(4));
        assert_eq!(c.at(c.source().now()), Some(Duration::from_secs(4)));
    }
}
