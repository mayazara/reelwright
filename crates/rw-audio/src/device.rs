//! Sound card output through cpal.

use crate::output::{AudioOutput, OutputFactory, Pull};
use crate::{CHANNELS, SAMPLE_RATE};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Assumed latency until the first callback reports the real one.
const INITIAL_LATENCY: Duration = Duration::from_millis(40);

/// Opens the system's default output device.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeviceFactory;

struct DeviceOutput {
    // Dropping the stream stops it.
    _stream: cpal::Stream,
    name: String,
    /// Measured output latency in nanoseconds.
    latency_ns: Arc<AtomicU64>,
}

impl AudioOutput for DeviceOutput {
    fn latency(&self) -> Duration {
        Duration::from_nanos(self.latency_ns.load(Ordering::Relaxed))
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

/// Converts the timeline's 48 kHz stereo into the device's rate and channel
/// count. Linear interpolation is enough for preview; export never goes
/// through here.
struct Adapter {
    pull: Pull,
    device_rate: u32,
    device_channels: usize,
    /// Source frames pulled but not yet consumed, interleaved stereo.
    src: Vec<f32>,
    /// Fractional read position in `src`, in source frames.
    pos: f64,
}

impl Adapter {
    fn fill(&mut self, out: &mut [f32]) {
        let ch = self.device_channels.max(1);
        let frames = out.len() / ch;
        if self.device_rate == SAMPLE_RATE {
            // Fast path: same rate, only the channel layout may differ.
            self.src.resize(frames * CHANNELS, 0.0);
            (self.pull)(&mut self.src);
            for (o, s) in out.chunks_mut(ch).zip(self.src.chunks(CHANNELS)) {
                write_frame(o, s[0], s[1]);
            }
            return;
        }
        let step = f64::from(SAMPLE_RATE) / f64::from(self.device_rate);
        let needed = (self.pos + frames as f64 * step).ceil() as usize + 2;
        let have = self.src.len() / CHANNELS;
        if needed > have {
            let start = self.src.len();
            self.src.resize(needed * CHANNELS, 0.0);
            (self.pull)(&mut self.src[start..]);
        }
        for o in out.chunks_mut(ch) {
            let i = self.pos.floor() as usize;
            let frac = (self.pos - i as f64) as f32;
            let a = &self.src[i * CHANNELS..i * CHANNELS + CHANNELS];
            let b = &self.src[(i + 1) * CHANNELS..(i + 1) * CHANNELS + CHANNELS];
            write_frame(o, a[0] + (b[0] - a[0]) * frac, a[1] + (b[1] - a[1]) * frac);
            self.pos += step;
        }
        // Keep the unconsumed tail (and one frame of history for interpolation).
        let used = self.pos.floor() as usize;
        self.src.drain(..used * CHANNELS);
        self.pos -= used as f64;
    }
}

/// Writes one stereo frame into a device frame of any width: mono gets the
/// average, extra channels get silence.
fn write_frame(out: &mut [f32], l: f32, r: f32) {
    match out.len() {
        0 => {}
        1 => out[0] = (l + r) * 0.5,
        _ => {
            out[0] = l;
            out[1] = r;
            out[2..].fill(0.0);
        }
    }
}

impl OutputFactory for DeviceFactory {
    fn open(&self, pull: Pull) -> Result<Box<dyn AudioOutput>, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no audio output device".to_owned())?;
        let name = device
            .description()
            .map(|d| d.to_string())
            .unwrap_or_else(|_| "Audio output".into());
        let latency_ns = Arc::new(AtomicU64::new(INITIAL_LATENCY.as_nanos() as u64));

        // Prefer 48 kHz stereo; otherwise use the device's own format.
        let preferred = cpal::StreamConfig {
            channels: CHANNELS as u16,
            sample_rate: SAMPLE_RATE,
            buffer_size: cpal::BufferSize::Default,
        };
        let fallback = device
            .default_output_config()
            .map_err(|e| e.to_string())?
            .config();
        let mut adapter = Some(Adapter {
            pull,
            device_rate: SAMPLE_RATE,
            device_channels: CHANNELS,
            src: Vec::new(),
            pos: 0.0,
        });
        let mut last_err = String::new();
        for config in [preferred, fallback] {
            let mut a = adapter
                .take()
                .expect("adapter present until a stream opens");
            a.device_rate = config.sample_rate;
            a.device_channels = usize::from(config.channels);
            let shared = Arc::new(std::sync::Mutex::new(a));
            let cb_adapter = shared.clone();
            let cb_latency = latency_ns.clone();
            let result = device.build_output_stream::<f32, _, _>(
                config,
                move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    let ts = info.timestamp();
                    let d = ts.playback.duration_since(ts.callback);
                    cb_latency.store(d.as_nanos() as u64, Ordering::Relaxed);
                    match cb_adapter.try_lock() {
                        Ok(mut a) => a.fill(data),
                        Err(_) => data.fill(0.0),
                    }
                },
                |e| tracing::warn!(error = %e, "audio stream error"),
                None,
            );
            match result {
                Ok(stream) => {
                    stream.play().map_err(|e| e.to_string())?;
                    tracing::info!(device = %name, rate = config.sample_rate, channels = config.channels, "audio output open");
                    return Ok(Box::new(DeviceOutput {
                        _stream: stream,
                        name,
                        latency_ns,
                    }));
                }
                Err(e) => {
                    last_err = e.to_string();
                    // Recover the adapter for the next attempt.
                    match Arc::try_unwrap(shared) {
                        Ok(m) => adapter = Some(m.into_inner().unwrap_or_else(|e| e.into_inner())),
                        Err(_) => break,
                    }
                }
            }
        }
        Err(format!("couldn't open {name}: {last_err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp_pull() -> Pull {
        let mut n = 0.0f32;
        Box::new(move |buf: &mut [f32]| {
            for f in buf.chunks_mut(CHANNELS) {
                f[0] = n;
                f[1] = -n;
                n += 1.0;
            }
        })
    }

    #[test]
    fn same_rate_maps_channels() {
        let mut a = Adapter {
            pull: ramp_pull(),
            device_rate: SAMPLE_RATE,
            device_channels: 6,
            src: Vec::new(),
            pos: 0.0,
        };
        let mut out = vec![9.0; 6 * 3];
        a.fill(&mut out);
        assert_eq!(&out[..6], &[0.0, -0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(&out[12..14], &[2.0, -2.0]);
        let mut mono = Adapter {
            pull: ramp_pull(),
            device_rate: SAMPLE_RATE,
            device_channels: 1,
            src: Vec::new(),
            pos: 0.0,
        };
        let mut out = vec![9.0; 2];
        mono.fill(&mut out);
        assert_eq!(out, [0.0, 0.0]);
    }

    #[test]
    fn resampling_is_continuous_across_callbacks() {
        // 44.1 kHz device: the ramp must advance by 48/44.1 per output frame,
        // with no jumps between callbacks of odd sizes.
        let mut a = Adapter {
            pull: ramp_pull(),
            device_rate: 44_100,
            device_channels: 2,
            src: Vec::new(),
            pos: 0.0,
        };
        let mut all = Vec::new();
        for size in [100, 37, 512, 3, 1000] {
            let mut out = vec![0.0; size * 2];
            a.fill(&mut out);
            all.extend(out.chunks(2).map(|f| f[0]));
        }
        let step = 48_000.0 / 44_100.0;
        for (i, v) in all.iter().enumerate() {
            assert!((v - i as f32 * step).abs() < 1e-2, "frame {i}: {v}");
        }
    }
}
