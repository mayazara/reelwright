//! Where recorded pictures and sound come from.

use crate::clock::TimeSource;
#[cfg(test)]
use crate::sync::CHANNELS;
use crate::sync::RATE;
use crossbeam_channel::{Receiver, Sender};
use rw_media::{CapturedFrame, DeviceReader, MediaError, RgbaImage};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

/// Pictures, one at a time, as they're captured.
pub trait VideoSource: Send {
    /// Waits for the next picture.
    fn read(&mut self) -> Result<CapturedFrame, MediaError>;
    /// Size of its pictures.
    fn size(&self) -> (u32, u32);
}

impl VideoSource for DeviceReader {
    fn read(&mut self) -> Result<CapturedFrame, MediaError> {
        DeviceReader::read(self)
    }

    fn size(&self) -> (u32, u32) {
        DeviceReader::size(self)
    }
}

/// A test picture source: `fps` pictures a second, each a flat grey whose
/// level encodes its number (4 levels per picture), paced in real time.
pub struct TestPattern {
    size: (u32, u32),
    interval: Duration,
    next: std::time::Instant,
    n: u32,
}

impl TestPattern {
    pub fn new(size: (u32, u32), fps: u32) -> Self {
        TestPattern {
            size,
            interval: Duration::from_secs_f64(1.0 / f64::from(fps.max(1))),
            next: std::time::Instant::now(),
            n: 0,
        }
    }
}

impl VideoSource for TestPattern {
    fn read(&mut self) -> Result<CapturedFrame, MediaError> {
        let now = std::time::Instant::now();
        if self.next > now {
            std::thread::sleep(self.next - now);
        }
        self.next += self.interval;
        let v = ((self.n * 4) % 256) as u8;
        self.n += 1;
        let (w, h) = self.size;
        let pixels = [v, v, v, 255].repeat((w * h) as usize);
        Ok(CapturedFrame::from_rgba(&RgbaImage::new(w, h, pixels)))
    }

    fn size(&self) -> (u32, u32) {
        self.size
    }
}

/// Sound captured at one moment: interleaved stereo at 48 kHz, and when
/// its first sample was captured (by the recording's time source).
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub samples: Vec<f32>,
    pub at: Duration,
}

/// A running sound input. Dropping it stops capturing.
pub struct AudioInput {
    pub chunks: Receiver<AudioChunk>,
    /// Peak level of the latest chunk, 0–1 (as f32 bits), for meters.
    pub level: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for AudioInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl AudioInput {
    /// The peak level of the most recent sound, 0–1.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
}

/// Converts any channel count and rate to stereo 48 kHz (linear
/// interpolation; good enough for speech and system sound).
struct Converter {
    channels: usize,
    rate: u32,
    pos: f64,
    last: [f32; 2],
}

impl Converter {
    fn stereo(&self, frame: &[f32]) -> [f32; 2] {
        match self.channels {
            0 => [0.0, 0.0],
            1 => [frame[0], frame[0]],
            _ => [frame[0], frame[1]],
        }
    }

    fn convert(&mut self, data: &[f32]) -> Vec<f32> {
        let frames: Vec<[f32; 2]> = data
            .chunks(self.channels.max(1))
            .map(|f| self.stereo(f))
            .collect();
        if self.rate == RATE {
            return frames.into_iter().flatten().collect();
        }
        let step = f64::from(self.rate) / f64::from(RATE);
        let mut out = Vec::new();
        // `pos` is relative to the sample before this block (`last`).
        while self.pos < frames.len() as f64 {
            let i = self.pos.floor() as isize - 1;
            let t = (self.pos - self.pos.floor()) as f32;
            let a = if i < 0 { self.last } else { frames[i as usize] };
            let b = frames[(i + 1) as usize];
            out.push(a[0] + (b[0] - a[0]) * t);
            out.push(a[1] + (b[1] - a[1]) * t);
            self.pos += step;
        }
        self.pos -= frames.len() as f64;
        if let Some(l) = frames.last() {
            self.last = *l;
        }
        out
    }
}

/// Which sound to capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioDevice {
    /// A microphone or line input, by name (`None` for the default).
    Input(Option<String>),
    /// What the computer is playing (Windows only).
    System,
}

/// Names of the sound inputs on this computer.
pub fn microphones() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    cpal::default_host()
        .input_devices()
        .map(|it| {
            it.filter_map(|d| d.description().ok().map(|n| n.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether this computer can record what it plays.
pub fn can_record_system_sound() -> bool {
    cfg!(target_os = "windows")
}

/// Starts capturing `device`, stamping chunks with `time`.
pub fn open_audio(device: &AudioDevice, time: Arc<dyn TimeSource>) -> Result<AudioInput, String> {
    let (tx, rx) = crossbeam_channel::bounded::<AudioChunk>(256);
    let level = Arc::new(AtomicU32::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<(), String>>(1);
    let (dev, lvl, stp) = (device.clone(), level.clone(), stop.clone());
    // The stream lives on its own thread: on some systems it can't move
    // between threads.
    let thread = std::thread::Builder::new()
        .name("audio-capture".into())
        .spawn(move || match start_stream(&dev, tx, lvl, time) {
            Ok(stream) => {
                let _ = ready_tx.send(Ok(()));
                while !stp.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(20));
                }
                drop(stream);
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(AudioInput {
            chunks: rx,
            level,
            stop,
            thread: Some(thread),
        }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => {
            stop.store(true, Ordering::Relaxed);
            Err("the sound device didn't start".into())
        }
    }
}

fn start_stream(
    device: &AudioDevice,
    tx: Sender<AudioChunk>,
    level: Arc<AtomicU32>,
    time: Arc<dyn TimeSource>,
) -> Result<cpal::Stream, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let dev = match device {
        AudioDevice::Input(None) => host.default_input_device(),
        AudioDevice::Input(Some(name)) => host
            .input_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.description().is_ok_and(|n| n.to_string() == *name))),
        // Recording from an output device captures what it plays
        // (WASAPI loopback).
        AudioDevice::System => host.default_output_device(),
    }
    .ok_or_else(|| "that sound device isn't connected".to_owned())?;
    let config = match device {
        AudioDevice::System => dev.default_output_config(),
        _ => dev.default_input_config(),
    }
    .map_err(|e| e.to_string())?;
    let mut conv = Converter {
        channels: usize::from(config.channels()),
        rate: config.sample_rate(),
        pos: 0.0,
        last: [0.0; 2],
    };
    let rate = f64::from(config.sample_rate());
    let channels = usize::from(config.channels()).max(1);
    let stream = dev
        .build_input_stream::<f32, _, _>(
            config.config(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // The first sample was captured a buffer's length ago.
                let frames = data.len() / channels;
                let at = time
                    .now()
                    .saturating_sub(Duration::from_secs_f64(frames as f64 / rate));
                let samples = conv.convert(data);
                let peak = samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                level.store(peak.min(1.0).to_bits(), Ordering::Relaxed);
                let _ = tx.try_send(AudioChunk { samples, at });
            },
            |e| tracing::warn!(error = %e, "sound capture error"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

/// A synthetic sound input for tests and diagnostics: a quiet tone with a
/// loud click at every whole second of its own sample count, delivered in
/// 10 ms chunks by a "sound card" running `ppm` parts per million fast
/// (negative: slow), stamped with `time`.
pub fn test_tone(time: Arc<dyn TimeSource>, ppm: f64) -> AudioInput {
    let (tx, rx) = crossbeam_channel::bounded::<AudioChunk>(1024);
    let level = Arc::new(AtomicU32::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (lvl, stp) = (level.clone(), stop.clone());
    let thread = std::thread::Builder::new()
        .name("test-tone".into())
        .spawn(move || {
            let rate = f64::from(RATE) * (1.0 + ppm / 1e6);
            let block = 480usize;
            let started = std::time::Instant::now();
            let mut n = 0u64;
            while !stp.load(Ordering::Relaxed) {
                // Deliver once the block's last sample has been "captured".
                let due = Duration::from_secs_f64((n + block as u64) as f64 / rate);
                let elapsed = started.elapsed();
                if due > elapsed {
                    std::thread::sleep(due - elapsed);
                }
                let at = time
                    .now()
                    .saturating_sub(Duration::from_secs_f64(block as f64 / rate));
                let mut samples = Vec::with_capacity(block * 2);
                for i in 0..block as u64 {
                    let k = n + i;
                    // A click for the first 2 ms of every second (by the
                    // card's own count, which is how a recording sees it).
                    let in_second = k % u64::from(RATE);
                    let v = if in_second < 96 {
                        0.9
                    } else {
                        (k as f32 * 0.05).sin() * 0.05
                    };
                    samples.push(v);
                    samples.push(v);
                }
                n += block as u64;
                lvl.store(0.9f32.to_bits(), Ordering::Relaxed);
                if tx.send(AudioChunk { samples, at }).is_err() {
                    return;
                }
            }
        })
        .expect("spawn test tone");
    AudioInput {
        chunks: rx,
        level,
        stop,
        thread: Some(thread),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converter_makes_stereo_48k() {
        let mut c = Converter {
            channels: 1,
            rate: 24_000,
            pos: 0.0,
            last: [0.0; 2],
        };
        let mut out = Vec::new();
        for _ in 0..10 {
            out.extend(c.convert(&[0.5f32; 240]));
        }
        // 2400 mono samples at 24 kHz → about 4800 stereo frames at 48 kHz.
        let frames = out.len() / CHANNELS;
        assert!((4795..=4801).contains(&frames), "{frames}");
        assert!(out[100..].iter().all(|v| (*v - 0.5).abs() < 1e-6));
    }
}
