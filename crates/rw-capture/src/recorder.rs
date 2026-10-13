//! A recording session: screen and/or camera pictures plus sound, each
//! picture source to its own file, all on one clock.

use crate::clock::{RecClock, TimeSource};
use crate::sources::{AudioInput, VideoSource};
use crate::sync::{AudioFollower, CHANNELS, RATE, frame_index};
use crossbeam_channel::{Receiver, Sender};
use rw_media::{AudioSettings, AudioWriter, Mp4Writer, VideoCodec, VideoSettings};
use rw_model::FrameRate;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Recording stops when the disk has less than this free.
pub const MIN_FREE_BYTES: u64 = 500 * 1024 * 1024;

/// Sound from an input that has said nothing for this long is filled with
/// silence (system sound sends nothing while nothing plays).
const QUIET_FILL: Duration = Duration::from_millis(200);

impl TimeSource for Arc<dyn TimeSource> {
    fn now(&self) -> Duration {
        (**self).now()
    }
}

type Clock = RecClock<Arc<dyn TimeSource>>;

/// A picture source, its writer, and (for the main file) the mixed sound.
type PictureJob = (Box<dyn VideoSource>, Mp4Writer, Option<Receiver<Vec<f32>>>);

/// What a finished recording file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Screen,
    Camera,
    Sound,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordedFile {
    pub path: PathBuf,
    pub kind: FileKind,
}

/// What to record from; each is already open.
pub struct Sources {
    pub screen: Option<Box<dyn VideoSource>>,
    pub camera: Option<Box<dyn VideoSource>>,
    pub audio: Vec<AudioInput>,
}

/// Where and how to record.
#[derive(Debug, Clone)]
pub struct Options {
    pub folder: PathBuf,
    /// File names start with this.
    pub name: String,
    pub fps: u32,
    /// Encoder to use; `None` for the best available.
    pub encoder: Option<String>,
}

/// A recording in progress.
pub struct Recorder {
    clock: Arc<Clock>,
    stop: Arc<AtomicBool>,
    /// Why recording stopped by itself (an error or a full disk).
    problem: Arc<Mutex<Option<String>>>,
    threads: Vec<JoinHandle<()>>,
    files: Vec<RecordedFile>,
    /// Peak level of the first sound input, for a meter.
    levels: Vec<Arc<std::sync::atomic::AtomicU32>>,
    _audio: Vec<AudioInput>,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder")
            .field("files", &self.files)
            .finish()
    }
}

/// An even output size no larger than 4K wide, keeping the shape.
fn output_size((w, h): (u32, u32)) -> (u32, u32) {
    let scale = (3840.0 / f64::from(w.max(1))).min(1.0);
    let even = |v: f64| ((v / 2.0).floor() as u32).max(1) * 2;
    (even(f64::from(w) * scale), even(f64::from(h) * scale))
}

fn video_settings(size: (u32, u32), fps: u32, encoder: &Option<String>) -> VideoSettings {
    let (w, h) = output_size(size);
    // Screens compress well; about 4 Mbit/s per megapixel at 30 fps.
    let rate = (f64::from(w) * f64::from(h) * 4.0 * f64::from(fps) / 30.0) as usize;
    VideoSettings {
        width: w,
        height: h,
        frame_rate: FrameRate::new(fps, 1),
        bit_rate: rate.max(1_000_000),
        encoder: encoder.clone(),
        codec: VideoCodec::H264,
    }
}

fn set_problem(problem: &Mutex<Option<String>>, stop: &AtomicBool, message: String) {
    let mut p = problem.lock().unwrap_or_else(|e| e.into_inner());
    if p.is_none() {
        tracing::warn!(%message, "recording stopped");
        *p = Some(message);
    }
    stop.store(true, Ordering::Relaxed);
}

impl Recorder {
    /// Starts recording `sources` into `options.folder`. `time` must be
    /// the time source the sound inputs stamp their chunks with.
    pub fn start(
        sources: Sources,
        options: Options,
        time: Arc<dyn TimeSource>,
    ) -> Result<Self, String> {
        if sources.screen.is_none() && sources.camera.is_none() && sources.audio.is_empty() {
            return Err("choose something to record".into());
        }
        std::fs::create_dir_all(&options.folder).map_err(|e| e.to_string())?;
        let clock = Arc::new(RecClock::new(time));
        let stop = Arc::new(AtomicBool::new(false));
        let problem = Arc::new(Mutex::new(None));
        let mut threads = Vec::new();
        let mut files = Vec::new();
        let path = |suffix: &str, ext: &str| {
            options
                .folder
                .join(format!("{} - {suffix}.{ext}", options.name))
        };

        // Mixed sound goes to the first picture file, or a sound file.
        let has_sound = !sources.audio.is_empty();
        let (mix_tx, mix_rx) = crossbeam_channel::unbounded::<Vec<f32>>();
        let Sources {
            screen,
            camera,
            audio,
        } = sources;
        let levels = audio.iter().map(|a| a.level.clone()).collect();
        let receivers: Vec<Receiver<crate::sources::AudioChunk>> =
            audio.iter().map(|a| a.chunks.clone()).collect();

        let mut pictures: Vec<(Box<dyn VideoSource>, FileKind, PathBuf)> = Vec::new();
        if let Some(s) = screen {
            pictures.push((s, FileKind::Screen, path("Screen", "mp4")));
        }
        if let Some(c) = camera {
            pictures.push((c, FileKind::Camera, path("Camera", "mp4")));
        }
        // Writers are created here, so errors (no encoder, no disk) come
        // back before anything starts.
        let mut primary = true;
        let mut started: Vec<PictureJob> = Vec::new();
        for (source, kind, p) in pictures {
            let with_sound = primary && has_sound;
            let settings = video_settings(source.size(), options.fps, &options.encoder);
            let audio_settings = AudioSettings { bit_rate: 192_000 };
            let writer =
                Mp4Writer::create_fragmented(&p, &settings, with_sound.then_some(&audio_settings))
                    .map_err(|e| e.to_string())?;
            files.push(RecordedFile { path: p, kind });
            started.push((source, writer, with_sound.then(|| mix_rx.clone())));
            primary = false;
        }
        let mut sound_writer = None;
        if started.is_empty() && has_sound {
            let p = path("Sound", "m4a");
            sound_writer =
                Some(AudioWriter::create_m4a_fragmented(&p, 192_000).map_err(|e| e.to_string())?);
            files.push(RecordedFile {
                path: p,
                kind: FileKind::Sound,
            });
        }

        clock.start();
        let fps = f64::from(options.fps);
        for (mut source, mut writer, sound) in started {
            let (clock, stop, problem) = (clock.clone(), stop.clone(), problem.clone());
            threads.push(
                std::thread::Builder::new()
                    .name("record-video".into())
                    .spawn(move || {
                        let mut last = None;
                        let mut run = || -> Result<(), String> {
                            while !stop.load(Ordering::Relaxed) {
                                let frame = source.read().map_err(|e| e.to_string())?;
                                if let Some(t) = clock.at(clock.source().now()) {
                                    // Every file starts at the recording's
                                    // zero: the first picture (devices take
                                    // a moment to deliver) is shown from 0.
                                    let index = if last.is_none() {
                                        0
                                    } else {
                                        frame_index(t, fps)
                                    };
                                    writer
                                        .write_frame(&frame, index)
                                        .map_err(|e| e.to_string())?;
                                    last = Some(frame);
                                }
                                if let Some(rx) = &sound {
                                    for chunk in rx.try_iter() {
                                        writer.write_audio(&chunk).map_err(|e| e.to_string())?;
                                    }
                                }
                            }
                            // Every file ends at the same moment: the clock
                            // is frozen at the stop (by `finish`, or here
                            // when a problem stopped it), and the last
                            // picture is held until then.
                            clock.pause();
                            let end = frame_index(clock.elapsed(), fps);
                            if let Some(frame) = &last
                                && end > 0
                            {
                                writer
                                    .write_frame(frame, end - 1)
                                    .map_err(|e| e.to_string())?;
                            }
                            // Sound mixed before the stop.
                            if let Some(rx) = &sound {
                                std::thread::sleep(Duration::from_millis(100));
                                for chunk in rx.try_iter() {
                                    writer.write_audio(&chunk).map_err(|e| e.to_string())?;
                                }
                            }
                            Ok(())
                        };
                        let result = run();
                        if let Err(e) = &result {
                            set_problem(&problem, &stop, e.clone());
                        }
                        if let Err(e) = writer.finish() {
                            set_problem(&problem, &stop, e.to_string());
                        }
                    })
                    .map_err(|e| e.to_string())?,
            );
        }
        if let Some(mut w) = sound_writer {
            let (stop, problem, rx) = (stop.clone(), problem.clone(), mix_rx.clone());
            threads.push(
                std::thread::Builder::new()
                    .name("record-sound".into())
                    .spawn(move || {
                        let mut run = || -> Result<(), String> {
                            loop {
                                match rx.recv_timeout(Duration::from_millis(50)) {
                                    Ok(chunk) => w.write(&chunk).map_err(|e| e.to_string())?,
                                    Err(_) if stop.load(Ordering::Relaxed) => return Ok(()),
                                    Err(_) => {}
                                }
                            }
                        };
                        if let Err(e) = run() {
                            set_problem(&problem, &stop, e);
                        }
                        if let Err(e) = w.finish() {
                            set_problem(&problem, &stop, e.to_string());
                        }
                    })
                    .map_err(|e| e.to_string())?,
            );
        }
        if has_sound {
            let (clock, stop) = (clock.clone(), stop.clone());
            threads.push(
                std::thread::Builder::new()
                    .name("record-mix".into())
                    .spawn(move || mix(&receivers, &clock, &stop, &mix_tx))
                    .map_err(|e| e.to_string())?,
            );
        }
        {
            let (folder, stop, problem) = (options.folder.clone(), stop.clone(), problem.clone());
            threads.push(
                std::thread::Builder::new()
                    .name("record-disk".into())
                    .spawn(move || watch_disk(&folder, &stop, &problem))
                    .map_err(|e| e.to_string())?,
            );
        }
        Ok(Recorder {
            clock,
            stop,
            problem,
            threads,
            files,
            levels,
            _audio: audio,
        })
    }

    pub fn pause(&self) {
        self.clock.pause();
    }

    pub fn resume(&self) {
        self.clock.resume();
    }

    pub fn is_paused(&self) -> bool {
        self.clock.is_paused()
    }

    /// Recorded time so far (pauses excluded).
    pub fn elapsed(&self) -> Duration {
        self.clock.elapsed()
    }

    /// Peak sound levels, 0–1, one per sound input.
    pub fn levels(&self) -> Vec<f32> {
        self.levels
            .iter()
            .map(|l| f32::from_bits(l.load(Ordering::Relaxed)))
            .collect()
    }

    /// Why recording stopped by itself, if it did.
    pub fn problem(&self) -> Option<String> {
        self.problem
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Stops and finishes the files. Files are complete even when a problem
    /// stopped the recording early; the problem is returned alongside.
    pub fn finish(mut self) -> (Vec<RecordedFile>, Option<String>) {
        // Freeze time first, so every file ends at this moment.
        self.clock.pause();
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        let problem = self.problem();
        (std::mem::take(&mut self.files), problem)
    }

    /// Stops and deletes what was recorded (a retake).
    pub fn discard(self) {
        let (files, _) = self.finish();
        for f in files {
            let _ = std::fs::remove_file(f.path);
        }
    }
}

/// Mixes every sound input, each kept in step with the clock, and sends
/// the mix on in chunks.
fn mix(
    inputs: &[Receiver<crate::sources::AudioChunk>],
    clock: &Clock,
    stop: &AtomicBool,
    out: &Sender<Vec<f32>>,
) {
    let mut followers: Vec<AudioFollower> = inputs.iter().map(|_| AudioFollower::new()).collect();
    let mut buffers: Vec<Vec<f32>> = vec![Vec::new(); inputs.len()];
    let mut sent = 0usize;
    loop {
        let stopping = stop.load(Ordering::Relaxed);
        let mut got = false;
        for (i, rx) in inputs.iter().enumerate() {
            for chunk in rx.try_iter() {
                got = true;
                if let Some(t) = clock.at(chunk.at) {
                    let kept = followers[i].take(t, &chunk.samples);
                    buffers[i].extend(kept);
                }
            }
        }
        // Inputs that have been quiet catch up with silence, so a silent
        // system-sound input doesn't hold back the microphone.
        if let Some(now) = clock.at(clock.source().now()) {
            let behind = now.saturating_sub(QUIET_FILL);
            for (f, b) in followers.iter_mut().zip(buffers.iter_mut()) {
                let target = (behind.as_secs_f64() * f64::from(RATE)) as i64;
                if f.written() < target {
                    b.extend(f.take(behind, &[]));
                }
            }
        }
        // Everything all inputs have reached can be mixed.
        let ready = buffers.iter().map(Vec::len).min().unwrap_or(0) / CHANNELS * CHANNELS;
        if ready > 0 {
            let mut mixed = vec![0.0f32; ready];
            for b in buffers.iter_mut() {
                for (m, s) in mixed.iter_mut().zip(b.drain(..ready)) {
                    *m += s;
                }
            }
            for m in mixed.iter_mut() {
                *m = m.clamp(-1.0, 1.0);
            }
            sent += ready;
            if out.send(mixed).is_err() {
                return;
            }
        }
        if stopping {
            let _ = sent;
            return;
        }
        if !got {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn watch_disk(folder: &Path, stop: &AtomicBool, problem: &Mutex<Option<String>>) {
    while !stop.load(Ordering::Relaxed) {
        if let Ok(free) = fs4::available_space(folder)
            && free < MIN_FREE_BYTES
        {
            set_problem(
                problem,
                stop,
                "the disk is nearly full, so recording stopped; what was recorded is kept".into(),
            );
            return;
        }
        for _ in 0..20 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
