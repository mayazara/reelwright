//! A clip's sound at its speed and direction.
//!
//! [`ClipAudio`] turns a source into the samples a clip plays, one output
//! sample per timeline sample:
//!
//! 1. [`Stream`] reads the clip's source span forwards, or backwards in
//!    blocks for reversed clips, as one sequence of samples.
//! 2. At normal speed that sequence plays as is. Otherwise it is either
//!    resampled (pitch follows speed, like a tape) or time-stretched with
//!    WSOLA (pitch kept): overlapping windows of the input are re-spaced in
//!    time, each nudged to where its waveform best lines up with the
//!    previous one so the joins don't click.

use crate::CHANNELS;

/// Where samples come from: a decoder in the app, a generated tone in tests.
pub trait SampleSource {
    /// Index of the next sample `read` returns.
    fn position(&self) -> i64;
    fn seek(&mut self, sample: i64);
    /// Fills `out` (interleaved stereo); returns floats written, fewer at the
    /// end of the source.
    fn read(&mut self, out: &mut [f32]) -> usize;
}

/// Samples kept before the requested range (the stretcher looks back).
const HISTORY: i64 = 4096;
/// Extra samples decoded past the requested range.
const AHEAD: i64 = 12_000;

/// The clip's samples in playing order. Index 0 is the clip's first sample
/// (the source's `origin` going forwards, the sample before `origin` going
/// backwards). Indices outside the source read as silence.
struct Stream<S> {
    source: S,
    reverse: bool,
    origin: i64,
    cache: Vec<f32>,
    /// Stream index of `cache[0]`.
    start: i64,
    scratch: Vec<f32>,
}

impl<S: SampleSource> Stream<S> {
    fn new(source: S, origin: i64, reverse: bool) -> Self {
        Stream {
            source,
            reverse,
            origin,
            cache: Vec::new(),
            start: 0,
            scratch: Vec::new(),
        }
    }

    fn end(&self) -> i64 {
        self.start + (self.cache.len() / CHANNELS) as i64
    }

    /// Reads source samples `[a, b)` into `scratch`, silence outside the source.
    fn read_source(&mut self, a: i64, b: i64) {
        let n = (b - a).max(0) as usize;
        self.scratch.clear();
        self.scratch.resize(n * CHANNELS, 0.0);
        let first = a.max(0);
        if first >= b {
            return;
        }
        if self.source.position() != first {
            self.source.seek(first);
        }
        let skip = ((first - a) as usize) * CHANNELS;
        let got = self.source.read(&mut self.scratch[skip..]);
        // A short read is the end of the source; the rest stays silent.
        let _ = got;
    }

    /// Makes `[from, to)` available through [`Self::slice`].
    fn ensure(&mut self, from: i64, to: i64) {
        if from >= self.start && to <= self.end() {
            return;
        }
        let keep_from = (from - HISTORY).max(self.start);
        if from >= self.start && from <= self.end() && !self.cache.is_empty() {
            // Extend: drop old samples, append new ones.
            let drop = ((keep_from - self.start) as usize) * CHANNELS;
            self.cache.drain(..drop);
            self.start = keep_from;
        } else {
            self.cache.clear();
            self.start = from - HISTORY;
        }
        let a = self.end();
        let b = to + AHEAD;
        if self.reverse {
            // Stream [a, b) is source [origin - b, origin - a), backwards.
            self.read_source(self.origin - b, self.origin - a);
            for frame in self.scratch.chunks(CHANNELS).rev() {
                self.cache.extend_from_slice(frame);
            }
        } else {
            self.read_source(self.origin + a, self.origin + b);
            self.cache.extend_from_slice(&self.scratch);
        }
    }

    /// Samples `[from, to)`; call [`Self::ensure`] first.
    fn slice(&self, from: i64, to: i64) -> &[f32] {
        let a = ((from - self.start) as usize) * CHANNELS;
        let b = ((to - self.start) as usize) * CHANNELS;
        &self.cache[a..b]
    }
}

/// WSOLA window length and output hop, in samples (~43 ms and ~21 ms).
const WINDOW: usize = 2048;
const HOP: usize = WINDOW / 2;
/// How far a window may move to line up with the previous one (~10 ms).
const TOLERANCE: i64 = 480;

/// Pitch-preserving time-stretch state.
struct Wsola {
    window: Vec<f32>,
    /// Overlap-add buffer: `WINDOW` stereo samples.
    acc: Vec<f32>,
    /// Finished output not yet handed out, starting at output index `ready_at`.
    ready: Vec<f32>,
    ready_at: i64,
    /// Output index where the next window starts.
    next_at: i64,
    /// Input position of the previous window.
    prev: Option<i64>,
}

impl Wsola {
    fn new() -> Self {
        // Periodic Hann: windows at half overlap sum to exactly one.
        let window = (0..WINDOW)
            .map(|i| {
                let x = std::f32::consts::TAU * i as f32 / WINDOW as f32;
                0.5 - 0.5 * x.cos()
            })
            .collect();
        Wsola {
            window,
            acc: vec![0.0; WINDOW * CHANNELS],
            ready: Vec::new(),
            ready_at: 0,
            next_at: 0,
            prev: None,
        }
    }

    /// Starts producing output from output index `at`.
    fn reset(&mut self, at: i64) {
        self.acc.fill(0.0);
        self.ready.clear();
        // One window before `at` primes the overlap; its half-finished
        // output is thrown away.
        self.next_at = at - HOP as i64;
        self.ready_at = self.next_at;
        self.prev = None;
    }

    /// Mono mix of stream samples, for lining windows up.
    fn mono(s: &[f32]) -> impl Iterator<Item = f32> + '_ {
        s.chunks(CHANNELS).map(|f| f.iter().sum::<f32>())
    }

    /// Normalised similarity of `cand` to `target` (mono, sampled every
    /// `step` samples).
    fn similarity(target: &[f32], cand: &[f32], step: usize) -> f32 {
        let (mut dot, mut energy) = (0.0f32, 1e-9f32);
        for (t, c) in target.iter().zip(cand).step_by(step) {
            dot += t * c;
            energy += c * c;
        }
        dot / energy.sqrt()
    }

    /// Adds one window to the output.
    fn step<S: SampleSource>(&mut self, stream: &mut Stream<S>, speed: f64) {
        let nominal = (self.next_at as f64 * speed).round() as i64;
        let pos = match self.prev {
            None => nominal,
            Some(prev) => {
                // The input that would naturally follow the previous window.
                let natural = prev + HOP as i64;
                let lo = nominal - TOLERANCE;
                let hi = nominal + TOLERANCE;
                stream.ensure(natural.min(lo), (natural + HOP as i64).max(hi + HOP as i64));
                let target: Vec<f32> =
                    Wsola::mono(stream.slice(natural, natural + HOP as i64)).collect();
                let all: Vec<f32> = Wsola::mono(stream.slice(lo, hi + HOP as i64)).collect();
                let score = |p: i64, step: usize| {
                    let o = (p - lo) as usize;
                    Wsola::similarity(&target, &all[o..o + HOP], step)
                };
                // Coarse search, then refine around the best.
                let mut best = nominal;
                let mut best_score = f32::MIN;
                for p in (lo..=hi).step_by(8) {
                    let s = score(p, 4);
                    if s > best_score {
                        (best, best_score) = (p, s);
                    }
                }
                let (a, b) = ((best - 8).max(lo), (best + 8).min(hi));
                best_score = f32::MIN;
                for p in a..=b {
                    let s = score(p, 1);
                    if s > best_score {
                        (best, best_score) = (p, s);
                    }
                }
                best
            }
        };
        stream.ensure(pos, pos + WINDOW as i64);
        for (i, (acc, s)) in self
            .acc
            .chunks_mut(CHANNELS)
            .zip(stream.slice(pos, pos + WINDOW as i64).chunks(CHANNELS))
            .enumerate()
        {
            let w = self.window[i];
            for (a, s) in acc.iter_mut().zip(s) {
                *a += s * w;
            }
        }
        self.prev = Some(pos);
        // The first half is now complete.
        self.ready.extend_from_slice(&self.acc[..HOP * CHANNELS]);
        self.acc.copy_within(HOP * CHANNELS.., 0);
        self.acc[HOP * CHANNELS..].fill(0.0);
        self.next_at += HOP as i64;
    }
}

/// Cubic (Catmull-Rom) interpolation between `p1` and `p2`.
fn cubic(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let a = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
    let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c = -0.5 * p0 + 0.5 * p2;
    ((a * t + b) * t + c) * t + p1
}

/// Speeds this close to one play the source unchanged.
const UNITY: f64 = 1e-4;

/// One clip's sound at its speed and direction.
pub struct ClipAudio<S> {
    stream: Stream<S>,
    speed: f64,
    keep_pitch: bool,
    wsola: Wsola,
    /// Output index the next `read` is expected at; anything else restarts.
    next: Option<i64>,
}

impl<S: SampleSource> ClipAudio<S> {
    /// `origin` is the source sample of the clip's first sample going
    /// forwards, or the sample just after its span going backwards.
    pub fn new(source: S, origin: i64, reverse: bool, speed: f32, keep_pitch: bool) -> Self {
        ClipAudio {
            stream: Stream::new(source, origin, reverse),
            speed: f64::from(speed),
            keep_pitch,
            wsola: Wsola::new(),
            next: None,
        }
    }

    /// Whether this clip plays with the given settings (otherwise it needs
    /// rebuilding).
    pub fn matches(&self, origin: i64, reverse: bool, speed: f32, keep_pitch: bool) -> bool {
        self.stream.origin == origin
            && self.stream.reverse == reverse
            && self.speed == f64::from(speed)
            && self.keep_pitch == keep_pitch
    }

    /// Fills `out` (interleaved stereo) with the clip's output samples
    /// starting at output index `at` (0 = the clip's first sample).
    pub fn read(&mut self, at: i64, out: &mut [f32]) {
        let frames = (out.len() / CHANNELS) as i64;
        let unity = (self.speed - 1.0).abs() < UNITY;
        if unity {
            self.stream.ensure(at, at + frames);
            out.copy_from_slice(self.stream.slice(at, at + frames));
        } else if self.keep_pitch {
            if self.next != Some(at) {
                self.wsola.reset(at);
            }
            let end = at + frames;
            while self.wsola.ready_at + (self.wsola.ready.len() / CHANNELS) as i64 <= end {
                self.wsola.step(&mut self.stream, self.speed);
            }
            let skip = ((at - self.wsola.ready_at) as usize) * CHANNELS;
            out.copy_from_slice(&self.wsola.ready[skip..skip + out.len()]);
            // Drop what's been handed out.
            let used = skip + out.len();
            self.wsola.ready.drain(..used);
            self.wsola.ready_at = end;
        } else {
            let first = (at as f64 * self.speed).floor() as i64 - 1;
            let last = ((at + frames) as f64 * self.speed).ceil() as i64 + 3;
            self.stream.ensure(first, last);
            let src = self.stream.slice(first, last);
            for (j, o) in out.chunks_mut(CHANNELS).enumerate() {
                let x = (at + j as i64) as f64 * self.speed;
                let i = x.floor() as i64;
                let t = (x - i as f64) as f32;
                let base = ((i - 1 - first) as usize) * CHANNELS;
                for (c, o) in o.iter_mut().enumerate() {
                    let p = |k: usize| src[base + k * CHANNELS + c];
                    *o = cubic(p(0), p(1), p(2), p(3), t);
                }
            }
        }
        self.next = Some(at + frames);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SAMPLE_RATE;

    /// A sine tone; with `ramp`, the right channel instead holds the sample
    /// index, to check read positions.
    struct Tone {
        freq: f64,
        len: i64,
        pos: i64,
        ramp: bool,
    }

    impl SampleSource for Tone {
        fn position(&self) -> i64 {
            self.pos
        }
        fn seek(&mut self, sample: i64) {
            self.pos = sample;
        }
        fn read(&mut self, out: &mut [f32]) -> usize {
            let mut n = 0;
            for f in out.chunks_mut(CHANNELS) {
                if self.pos >= self.len {
                    break;
                }
                let t = self.pos as f64 / f64::from(SAMPLE_RATE);
                f[0] = (std::f64::consts::TAU * self.freq * t).sin() as f32 * 0.5;
                f[1] = if self.ramp { self.pos as f32 } else { f[0] };
                self.pos += 1;
                n += CHANNELS;
            }
            n
        }
    }

    fn tone(freq: f64, secs: i64) -> Tone {
        Tone {
            freq,
            len: secs * i64::from(SAMPLE_RATE),
            pos: 0,
            ramp: false,
        }
    }

    fn ramp(secs: i64) -> Tone {
        Tone {
            ramp: true,
            ..tone(440.0, secs)
        }
    }

    /// Plays `clip` from 0 in blocks of `block` samples.
    fn play<S: SampleSource>(clip: &mut ClipAudio<S>, frames: usize, block: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * CHANNELS];
        let mut at = 0;
        for chunk in out.chunks_mut(block * CHANNELS) {
            clip.read(at as i64, chunk);
            at += chunk.len() / CHANNELS;
        }
        out
    }

    /// Dominant frequency of the left channel: the strongest of a sweep of
    /// Goertzel filters (1 Hz steps around `expect`, and its 1/4 and 4×).
    fn pitch(samples: &[f32], expect: f64) -> f64 {
        let left: Vec<f32> = samples.iter().step_by(CHANNELS).copied().collect();
        let power = |f: f64| {
            let w = std::f64::consts::TAU * f / f64::from(SAMPLE_RATE);
            let (mut s1, mut s2) = (0.0f64, 0.0f64);
            for &x in &left {
                let s = f64::from(x) + 2.0 * w.cos() * s1 - s2;
                s2 = s1;
                s1 = s;
            }
            s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2
        };
        let mut best = (0.0, f64::MIN);
        for centre in [expect / 4.0, expect, expect * 4.0] {
            let mut f = centre * 0.9;
            while f < centre * 1.1 {
                let p = power(f);
                if p > best.1 {
                    best = (f, p);
                }
                f += 1.0;
            }
        }
        best.0
    }

    #[test]
    fn unity_forward_and_reverse_are_exact() {
        let mut clip = ClipAudio::new(ramp(4), 1000, false, 1.0, true);
        let out = play(&mut clip, 30_000, 1024);
        assert!(
            out.chunks(2)
                .enumerate()
                .all(|(i, f)| f[1] == (1000 + i) as f32)
        );

        // Backwards from sample 30 000: 29 999, 29 998, ...; before the
        // source starts it is silent.
        let mut clip = ClipAudio::new(ramp(4), 30_000, true, 1.0, true);
        let out = play(&mut clip, 40_000, 777);
        for (i, f) in out.chunks(2).enumerate() {
            let want = 29_999 - i as i64;
            assert_eq!(f[1], want.max(0) as f32, "sample {i}");
        }
    }

    #[test]
    fn four_times_faster_keeps_pitch_only_when_asked() {
        // 8 s of 440 Hz played at 4× gives 2 s.
        let frames = 2 * SAMPLE_RATE as usize;
        let mut kept = ClipAudio::new(tone(440.0, 8), 0, false, 4.0, true);
        let out = play(&mut kept, frames, 1024);
        let f = pitch(&out[4096..], 440.0);
        assert!((f - 440.0).abs() < 5.0, "pitch kept: {f} Hz");

        let mut tape = ClipAudio::new(tone(440.0, 8), 0, false, 4.0, false);
        let out = play(&mut tape, frames, 1024);
        let f = pitch(&out, 440.0);
        assert!((f - 1760.0).abs() < 5.0, "pitch follows speed: {f} Hz");
        // The read position advances four samples per sample.
        let mut tape = ClipAudio::new(ramp(8), 0, false, 4.0, false);
        let out = play(&mut tape, 2000, 1024);
        assert_eq!(out[2 * 1000 + 1], 4000.0);
    }

    #[test]
    fn slow_motion_keeps_pitch_and_level() {
        let frames = SAMPLE_RATE as usize;
        let mut clip = ClipAudio::new(tone(440.0, 2), 0, false, 0.25, true);
        let out = play(&mut clip, frames, 500);
        let f = pitch(&out, 440.0);
        assert!((f - 440.0).abs() < 5.0, "{f} Hz");
        // No dips at window joins: the level stays near the source's.
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        for block in left.chunks(960) {
            let peak = block.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(peak > 0.4 && peak < 0.6, "peak {peak}");
        }
    }

    #[test]
    fn stretched_output_doesnt_depend_on_block_size_and_seeks() {
        let mut a = ClipAudio::new(tone(300.0, 4), 0, false, 1.7, true);
        let mut b = ClipAudio::new(tone(300.0, 4), 0, false, 1.7, true);
        assert_eq!(play(&mut a, 20_000, 1024), play(&mut b, 20_000, 333));
        // Jumping elsewhere restarts cleanly at the right place.
        let mut out = vec![0.0; 2048];
        a.read(48_000, &mut out);
        let f = pitch(
            &{
                let mut v = vec![0.0; 48_000];
                a.read(49_024, &mut v);
                v
            },
            300.0,
        );
        assert!((f - 300.0).abs() < 5.0, "{f} Hz");
    }
}
