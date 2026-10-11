//! Audio output devices.
//!
//! An output pulls interleaved stereo samples at [`SAMPLE_RATE`] from a
//! callback. The playback clock is derived from how many samples the device
//! has actually consumed, so video follows the sound.

use crate::{CHANNELS, SAMPLE_RATE};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Fills the buffer with interleaved stereo samples.
pub type Pull = Box<dyn FnMut(&mut [f32]) + Send>;

/// A running output stream. Dropping it stops playback.
pub trait AudioOutput: Send {
    /// Delay between a sample being pulled and being heard.
    fn latency(&self) -> Duration;
    fn name(&self) -> String;
}

/// Opens outputs.
pub trait OutputFactory: Send + Sync {
    fn open(&self, pull: Pull) -> Result<Box<dyn AudioOutput>, String>;
}

/// An output with no device: pulls audio at real-time pace and discards it.
/// Used when no sound device is available, and in tests. `speed` above 1
/// runs faster than real time.
pub struct NullOutput {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Block size the null output pulls (10 ms).
pub const NULL_BLOCK: usize = (SAMPLE_RATE / 100) as usize;

impl NullOutput {
    pub fn start(mut pull: Pull, speed: f64) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("null-audio".into())
            .spawn(move || {
                let mut buf = vec![0.0f32; NULL_BLOCK * CHANNELS];
                let block = Duration::from_secs_f64(
                    NULL_BLOCK as f64 / f64::from(SAMPLE_RATE) / speed.max(0.01),
                );
                let mut next = Instant::now();
                while !flag.load(Ordering::Relaxed) {
                    pull(&mut buf);
                    next += block;
                    let now = Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    } else if now - next > Duration::from_millis(200) {
                        // Fell far behind (machine paused): don't burst.
                        next = now;
                    }
                }
            })
            .expect("failed to spawn audio thread");
        NullOutput {
            stop,
            thread: Some(thread),
        }
    }
}

impl AudioOutput for NullOutput {
    fn latency(&self) -> Duration {
        Duration::ZERO
    }

    fn name(&self) -> String {
        "No audio device".into()
    }
}

impl Drop for NullOutput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Factory for [`NullOutput`].
#[derive(Debug, Clone, Copy)]
pub struct NullFactory {
    pub speed: f64,
}

impl Default for NullFactory {
    fn default() -> Self {
        NullFactory { speed: 1.0 }
    }
}

impl OutputFactory for NullFactory {
    fn open(&self, pull: Pull) -> Result<Box<dyn AudioOutput>, String> {
        Ok(Box::new(NullOutput::start(pull, self.speed)))
    }
}
