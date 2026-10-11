//! Real-time playback with an audio-driven clock.
//!
//! While playing, three threads cooperate:
//! - a producer mixes the timeline's sound a little ahead into a lock-free
//!   ring buffer;
//! - the audio output pulls from the ring and counts the samples it really
//!   played. That count *is* the clock: `origin + played / 48 kHz − latency`.
//!   If the producer falls behind, the clock stalls instead of letting sound
//!   and picture drift apart;
//! - a video thread decodes frames a few frames ahead of the clock.
//!
//! The UI asks for the newest prepared frame not later than the clock;
//! anything older is dropped.

use crate::frames::FrameSource;
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use rw_audio::output::{AudioOutput, OutputFactory};
use rw_audio::{CHANNELS, Mixer, SAMPLE_RATE, sample_to_time, time_to_sample};
use rw_cache::ProjectCache;
use rw_model::{Project, Time};
use rw_render::Layer;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Audio mixed ahead of the device, in sample frames (250 ms).
const RING_FRAMES: usize = SAMPLE_RATE as usize / 4;
/// Mixer block size (about 21 ms).
const MIX_BLOCK: usize = 1024;
/// Video frames decoded ahead of the clock.
const VIDEO_AHEAD: usize = 4;

/// A frame ready to composite.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// Timeline time of the frame.
    pub t: Time,
    pub layers: Vec<Layer>,
}

/// Counters for the current play session.
#[derive(Debug, Default)]
pub struct Stats {
    pub shown: AtomicU64,
    pub dropped: AtomicU64,
    /// Largest (clock − shown frame time), in ticks.
    pub max_lag: AtomicU64,
    /// Samples the output wanted but the producer hadn't mixed yet.
    pub underrun_samples: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatsSnapshot {
    pub shown: u64,
    pub dropped: u64,
    pub max_lag: Time,
    pub underrun_samples: u64,
}

impl Stats {
    fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            shown: self.shown.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            max_lag: Time(self.max_lag.load(Ordering::Relaxed) as i64),
            underrun_samples: self.underrun_samples.load(Ordering::Relaxed),
        }
    }
}

type SharedProject = Arc<Mutex<Arc<Project>>>;

struct Clock {
    origin: Time,
    played: Arc<AtomicU64>,
    /// Output latency in ticks; known once the device is open.
    latency: AtomicI64,
}

impl Clock {
    fn now(&self) -> Time {
        let played = sample_to_time(self.played.load(Ordering::Acquire) as i64);
        let latency = Time(self.latency.load(Ordering::Relaxed));
        self.origin + (played - latency).max(Time::ZERO)
    }
}

struct Session {
    stop: Arc<AtomicBool>,
    clock: Arc<Clock>,
    frames: Arc<Mutex<VecDeque<Prepared>>>,
    stats: Arc<Stats>,
    // Dropped in this order: output first so it stops pulling.
    output: Option<Box<dyn AudioOutput>>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.output.take();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// Plays a project. Owned by the UI thread.
pub struct Player {
    factory: Arc<dyn OutputFactory>,
    cache: ProjectCache,
    project: SharedProject,
    session: Option<Session>,
    paused_at: Time,
    last_stats: StatsSnapshot,
    output_name: String,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player")
            .field("playing", &self.is_playing())
            .field("position", &self.position())
            .finish()
    }
}

impl Player {
    pub fn new(
        factory: Arc<dyn OutputFactory>,
        cache: ProjectCache,
        project: Arc<Project>,
    ) -> Self {
        Player {
            factory,
            cache,
            project: Arc::new(Mutex::new(project)),
            session: None,
            paused_at: Time::ZERO,
            last_stats: StatsSnapshot::default(),
            output_name: String::new(),
        }
    }

    /// Replaces the project being played (after an edit). Running threads
    /// pick it up on their next block or frame.
    pub fn set_project(&self, project: Arc<Project>) {
        *self.project.lock().unwrap_or_else(|e| e.into_inner()) = project;
    }

    pub fn is_playing(&self) -> bool {
        self.session.is_some()
    }

    /// Name of the audio output in use while playing.
    pub fn output_name(&self) -> &str {
        &self.output_name
    }

    /// Current playhead.
    pub fn position(&self) -> Time {
        match &self.session {
            Some(s) => s.clock.now(),
            None => self.paused_at,
        }
    }

    /// Moves the playhead; restarts playback there if playing.
    pub fn seek(&mut self, t: Time, preview_size: (u32, u32), prefer_proxies: bool) {
        let t = t.max(Time::ZERO);
        if self.is_playing() {
            self.play(t, preview_size, prefer_proxies);
        } else {
            self.paused_at = t;
        }
    }

    /// Starts playing from `from`, decoding pictures for an output of
    /// `preview_size`.
    pub fn play(&mut self, from: Time, preview_size: (u32, u32), prefer_proxies: bool) {
        self.pause();
        let from = from.max(Time::ZERO);
        let stop = Arc::new(AtomicBool::new(false));
        let played = Arc::new(AtomicU64::new(0));
        let stats = Arc::new(Stats::default());
        let frames = Arc::new(Mutex::new(VecDeque::new()));
        let (mut prod, mut cons) = ringbuf::HeapRb::<f32>::new(RING_FRAMES * CHANNELS).split();

        // Prime the ring so playback starts with sound, not an underrun.
        let mut mixer = Mixer::new();
        let mut next_sample = time_to_sample(from);
        let mut block = vec![0.0f32; MIX_BLOCK * CHANNELS];
        for _ in 0..4 {
            let project = self
                .project
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            mixer.mix(&project, next_sample, &mut block);
            prod.push_slice(&block);
            next_sample += MIX_BLOCK as i64;
        }

        let clock = Arc::new(Clock {
            origin: from,
            played: played.clone(),
            latency: AtomicI64::new(0),
        });

        let mut threads = Vec::new();
        {
            let stop = stop.clone();
            let project = self.project.clone();
            threads.push(
                std::thread::Builder::new()
                    .name("audio-mix".into())
                    .spawn(move || {
                        while !stop.load(Ordering::Relaxed) {
                            if prod.vacant_len() < MIX_BLOCK * CHANNELS {
                                std::thread::sleep(Duration::from_millis(2));
                                continue;
                            }
                            let p = project.lock().unwrap_or_else(|e| e.into_inner()).clone();
                            mixer.mix(&p, next_sample, &mut block);
                            prod.push_slice(&block);
                            next_sample += MIX_BLOCK as i64;
                        }
                    })
                    .expect("spawn audio-mix"),
            );
        }
        {
            let stop = stop.clone();
            let project = self.project.clone();
            let clock = clock.clone();
            let frames = frames.clone();
            let stats = stats.clone();
            let cache = self.cache.clone();
            threads.push(
                std::thread::Builder::new()
                    .name("video-decode".into())
                    .spawn(move || {
                        // Built here: decoders hold FFmpeg state that can't
                        // move between threads.
                        let mut source = FrameSource::new(cache, prefer_proxies);
                        let mut next: Option<i64> = None;
                        while !stop.load(Ordering::Relaxed) {
                            let p = project.lock().unwrap_or_else(|e| e.into_inner()).clone();
                            let fps = p.canvas.frame_rate;
                            let now_frame = fps.time_to_frame(clock.now());
                            let mut n = next.unwrap_or(now_frame);
                            if n < now_frame {
                                // Too slow: skip to the clock.
                                stats
                                    .dropped
                                    .fetch_add((now_frame - n) as u64, Ordering::Relaxed);
                                n = now_frame;
                            }
                            let queued = frames.lock().unwrap_or_else(|e| e.into_inner()).len();
                            if queued >= VIDEO_AHEAD {
                                std::thread::sleep(Duration::from_millis(2));
                                continue;
                            }
                            let t = fps.frame_to_time(n);
                            let layers = source.layers_at(&p, t, preview_size);
                            frames
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .push_back(Prepared { t, layers });
                            next = Some(n + 1);
                        }
                    })
                    .expect("spawn video-decode"),
            );
        }
        // Start the clock only once the first picture is ready, so playback
        // doesn't open with a stutter while decoders spin up.
        let ready_by = std::time::Instant::now() + Duration::from_millis(500);
        while frames.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
            && std::time::Instant::now() < ready_by
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        let pull_played = played.clone();
        let pull_stats = stats.clone();
        let pull = Box::new(move |buf: &mut [f32]| {
            let n = cons.pop_slice(buf);
            buf[n..].fill(0.0);
            pull_stats
                .underrun_samples
                .fetch_add(((buf.len() - n) / CHANNELS) as u64, Ordering::Relaxed);
            pull_played.fetch_add((n / CHANNELS) as u64, Ordering::Release);
        });
        let output = match self.factory.open(pull) {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!(error = %e, "audio output unavailable; playing silently");
                // Fall back to a device-less clock so video still plays.
                let p = played.clone();
                rw_audio::output::NullFactory::default()
                    .open(Box::new(move |buf: &mut [f32]| {
                        p.fetch_add((buf.len() / CHANNELS) as u64, Ordering::Release);
                    }))
                    .expect("null output always opens")
            }
        };
        self.output_name = output.name();
        clock.latency.store(
            Time::from_secs_f64(output.latency().as_secs_f64()).ticks(),
            Ordering::Relaxed,
        );

        self.session = Some(Session {
            stop,
            clock,
            frames,
            stats,
            output: Some(output),
            threads,
        });
    }

    /// Stops playback, keeping the playhead where it was.
    pub fn pause(&mut self) -> Time {
        if let Some(s) = self.session.take() {
            self.paused_at = s.clock.now();
            self.last_stats = s.stats.snapshot();
        }
        self.paused_at
    }

    /// The newest prepared frame whose time has come, if a new one is ready.
    pub fn take_frame(&self) -> Option<Prepared> {
        let s = self.session.as_ref()?;
        let now = s.clock.now();
        let mut q = s.frames.lock().unwrap_or_else(|e| e.into_inner());
        let mut shown = None;
        while q.front().is_some_and(|f| f.t <= now) {
            if shown.is_some() {
                s.stats.dropped.fetch_add(1, Ordering::Relaxed);
            }
            shown = q.pop_front();
        }
        if let Some(f) = &shown {
            s.stats.shown.fetch_add(1, Ordering::Relaxed);
            s.stats
                .max_lag
                .fetch_max((now - f.t).ticks() as u64, Ordering::Relaxed);
        }
        shown
    }

    /// Counters for the current session, or the last one if paused.
    pub fn stats(&self) -> StatsSnapshot {
        self.session
            .as_ref()
            .map_or(self.last_stats, |s| s.stats.snapshot())
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.pause();
    }
}
