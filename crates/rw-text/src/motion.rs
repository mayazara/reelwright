//! How a text item moves at a moment: entrance, exit and credits roll.

use rw_model::{TextBehaviour, TextMotion, TextSpec, Time};

/// What to change about the drawn text at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// Characters to draw; `None` for all.
    pub reveal: Option<usize>,
    pub opacity: f32,
    /// Shift as shares of the canvas width and height.
    pub offset: [f32; 2],
    pub scale: f32,
    /// Credits roll: `0` = text just below the frame, `1` = just above.
    pub roll: Option<f32>,
}

impl Default for Pose {
    fn default() -> Self {
        Pose {
            reveal: None,
            opacity: 1.0,
            offset: [0.0, 0.0],
            scale: 1.0,
            roll: None,
        }
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// Overshoots then settles (for "pop").
fn ease_out_back(t: f32) -> f32 {
    let c = 1.70158;
    1.0 + (c + 1.0) * (t - 1.0).powi(3) + c * (t - 1.0).powi(2)
}

/// Drops and bounces to rest (1 at rest).
fn bounce(t: f32) -> f32 {
    let (n, d) = (7.5625, 2.75);
    if t < 1.0 / d {
        n * t * t
    } else if t < 2.0 / d {
        let t = t - 1.5 / d;
        n * t * t + 0.75
    } else if t < 2.5 / d {
        let t = t - 2.25 / d;
        n * t * t + 0.9375
    } else {
        let t = t - 2.625 / d;
        n * t * t + 0.984375
    }
}

/// Number of words (runs of non-space) in the first `chars` characters.
fn chars_for_words(text: &str, words: usize) -> usize {
    let mut seen = 0;
    let mut in_word = false;
    for (i, c) in text.chars().enumerate() {
        if c.is_whitespace() {
            in_word = false;
        } else if !in_word {
            in_word = true;
            seen += 1;
            if seen > words {
                return i;
            }
        }
    }
    text.chars().count()
}

/// Applies one motion at `t` (0 = start of the entrance or end of the exit,
/// 1 = fully on screen).
fn apply(pose: &mut Pose, motion: TextMotion, t: f32, text: &str) {
    let t = t.clamp(0.0, 1.0);
    let e = ease_out(t);
    match motion {
        TextMotion::None => {}
        TextMotion::Fade => pose.opacity *= t,
        TextMotion::Pop => {
            pose.scale *= 0.4 + 0.6 * ease_out_back(t);
            pose.opacity *= (t * 3.0).min(1.0);
        }
        TextMotion::SlideLeft => {
            pose.offset[0] += 0.25 * (1.0 - e);
            pose.opacity *= e;
        }
        TextMotion::SlideRight => {
            pose.offset[0] -= 0.25 * (1.0 - e);
            pose.opacity *= e;
        }
        TextMotion::SlideUp => {
            pose.offset[1] += 0.2 * (1.0 - e);
            pose.opacity *= e;
        }
        TextMotion::SlideDown => {
            pose.offset[1] -= 0.2 * (1.0 - e);
            pose.opacity *= e;
        }
        TextMotion::Zoom => {
            pose.scale *= 1.0 + 1.5 * (1.0 - e);
            pose.opacity *= e;
        }
        TextMotion::Bounce => {
            pose.offset[1] -= 0.3 * (1.0 - bounce(t));
            pose.opacity *= (t * 4.0).min(1.0);
        }
        TextMotion::Typewriter => {
            let n = text.chars().count();
            let shown = ((n as f32) * t).floor() as usize;
            pose.reveal = Some(pose.reveal.map_or(shown, |r| r.min(shown)));
        }
        TextMotion::WordByWord => {
            let words = text.split_whitespace().count();
            let shown = ((words as f32) * t).ceil() as usize;
            let chars = if t <= 0.0 {
                0
            } else {
                chars_for_words(text, shown)
            };
            pose.reveal = Some(pose.reveal.map_or(chars, |r| r.min(chars)));
        }
    }
}

/// The pose of `spec` showing `text`, `local` into an item of `duration`.
pub fn pose_at(spec: &TextSpec, text: &str, local: Time, duration: Time) -> Pose {
    let mut pose = Pose::default();
    let secs = |t: Time| t.as_secs_f64() as f32;
    if spec.enter.duration > Time::ZERO && local < spec.enter.duration {
        apply(
            &mut pose,
            spec.enter.motion,
            secs(local) / secs(spec.enter.duration),
            text,
        );
    }
    let left = duration - local;
    if spec.exit.duration > Time::ZERO && left < spec.exit.duration {
        apply(
            &mut pose,
            spec.exit.motion,
            secs(left) / secs(spec.exit.duration),
            text,
        );
    }
    if spec.behaviour == TextBehaviour::Roll {
        pose.roll = Some((secs(local) / secs(duration).max(1e-6)).clamp(0.0, 1.0));
    }
    pose
}

#[cfg(test)]
mod tests {
    use super::*;
    use rw_model::TextAnimation;

    fn spec(enter: TextMotion, exit: TextMotion) -> TextSpec {
        let mut s = TextSpec::new("one two three");
        s.enter = TextAnimation {
            motion: enter,
            duration: Time::from_secs(1),
        };
        s.exit = TextAnimation {
            motion: exit,
            duration: Time::from_secs(1),
        };
        s
    }

    #[test]
    fn rests_between_entrance_and_exit() {
        for m in TextMotion::ALL {
            let s = spec(m, m);
            let p = pose_at(&s, &s.text, Time::from_secs(2), Time::from_secs(5));
            assert_eq!(p, Pose::default(), "{m:?} at rest");
        }
    }

    #[test]
    fn typewriter_and_words_reveal_progressively() {
        let s = spec(TextMotion::Typewriter, TextMotion::None);
        let at = |ms| pose_at(&s, &s.text, Time::from_millis(ms), Time::from_secs(5)).reveal;
        assert_eq!(at(0), Some(0));
        assert_eq!(at(500), Some(6));
        let s = spec(TextMotion::WordByWord, TextMotion::None);
        let at = |ms| pose_at(&s, &s.text, Time::from_millis(ms), Time::from_secs(5)).reveal;
        assert_eq!(at(0), Some(0));
        assert_eq!(at(100), Some(4), "first word: 'one '");
        assert_eq!(at(500), Some(8), "two words");
        assert_eq!(at(999), Some(13));
    }

    #[test]
    fn fades_and_slides_start_off_and_end_on() {
        let s = spec(TextMotion::SlideUp, TextMotion::Fade);
        let start = pose_at(&s, &s.text, Time::ZERO, Time::from_secs(5));
        assert_eq!(start.opacity, 0.0);
        assert!(start.offset[1] > 0.1);
        let near_end = pose_at(&s, &s.text, Time::from_millis(4900), Time::from_secs(5));
        assert!((near_end.opacity - 0.1).abs() < 1e-3);
    }
}
