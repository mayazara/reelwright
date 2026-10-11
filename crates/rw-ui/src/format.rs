//! Human-readable formatting for the UI.

use rw_model::Time;
use std::time::{Duration, SystemTime};

/// `m:ss` or `h:mm:ss`, rounded down to whole seconds.
pub fn duration(t: Time) -> String {
    let total = t.as_secs_f64().max(0.0) as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
pub fn ago(then: SystemTime, now: SystemTime) -> String {
    let d = now.duration_since(then).unwrap_or(Duration::ZERO).as_secs();
    match d {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", d / 60),
        3600..86_400 => format!("{} h ago", d / 3600),
        86_400..172_800 => "yesterday".into(),
        _ => format!("{} days ago", d / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(duration(Time::from_millis(65_900)), "1:05");
        assert_eq!(duration(Time::from_secs(3_725)), "1:02:05");
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(1_500_000), "1.5 MB");
        let now = SystemTime::now();
        assert_eq!(ago(now, now), "just now");
        assert_eq!(ago(now - Duration::from_secs(7200), now), "2 h ago");
        assert_eq!(ago(now + Duration::from_secs(10), now), "just now");
    }
}
