//! Mixes every audible item on the timeline into interleaved stereo.

use crate::stretch::{ClipAudio, SampleSource};
use crate::{CHANNELS, sample_to_time, time_to_sample};
use rw_media::AudioReader;
use rw_model::{Item, ItemId, MediaKind, Project, TrackKind};
use std::collections::HashMap;

/// Readers idle longer than this many mix calls are closed.
const IDLE_CALLS: u64 = 400;

/// Items whose sound plays, with the combined gain of item and track.
pub fn audible_items(project: &Project) -> Vec<(&Item, f32)> {
    let mut out = Vec::new();
    for track in &project.tracks {
        if track.muted {
            continue;
        }
        for item in &track.items {
            // Held frames are silent.
            if item.muted || item.gain <= 0.0 || item.held_frame().is_some() {
                continue;
            }
            let Some(media) = item.media().and_then(|id| project.media(id)) else {
                continue;
            };
            let has_sound = media.info.audio.is_some()
                && match track.kind {
                    TrackKind::Audio => true,
                    TrackKind::Video | TrackKind::Overlay => {
                        media.kind() == MediaKind::Video && !item.audio_detached
                    }
                    TrackKind::Text => false,
                };
            if has_sound {
                out.push((item, item.gain * track.gain));
            }
        }
    }
    out
}

/// A decoder addressed in samples.
struct Reader {
    reader: AudioReader,
    failed: bool,
}

impl SampleSource for Reader {
    fn position(&self) -> i64 {
        time_to_sample(self.reader.position())
    }

    fn seek(&mut self, sample: i64) {
        self.reader.seek(sample_to_time(sample));
    }

    fn read(&mut self, out: &mut [f32]) -> usize {
        if self.failed {
            return 0;
        }
        self.reader.read(out).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "audio decode failed");
            self.failed = true;
            0
        })
    }
}

struct Source {
    clip: Option<ClipAudio<Reader>>,
    /// The file failed to open; the item stays silent.
    broken: bool,
    last_used: u64,
}

/// Keeps one reader per item so overlapping uses of the same file don't
/// fight over a seek position.
#[derive(Default)]
pub struct Mixer {
    sources: HashMap<ItemId, Source>,
    calls: u64,
    scratch: Vec<f32>,
}

impl std::fmt::Debug for Mixer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mixer")
            .field("sources", &self.sources.len())
            .finish()
    }
}

impl Mixer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fills `out` (interleaved stereo) with the timeline's sound starting
    /// at timeline sample `start` (sample 0 = time zero).
    pub fn mix(&mut self, project: &Project, start: i64, out: &mut [f32]) {
        self.calls += 1;
        out.fill(0.0);
        let frames = (out.len() / CHANNELS) as i64;
        let end = start + frames;
        for (item, gain) in audible_items(project) {
            let item_start = time_to_sample(item.start);
            let item_end = time_to_sample(item.end());
            let (a, b) = (start.max(item_start), end.min(item_end));
            if a >= b {
                continue;
            }
            let Some(media) = item.media().and_then(|id| project.media(id)) else {
                continue;
            };
            // The clip's first sample in the source, in playing order.
            let origin = time_to_sample(if item.reverse {
                item.source_in + item.source_span()
            } else {
                item.source_in
            });
            let source = self.sources.entry(item.id).or_insert_with(|| Source {
                clip: None,
                broken: false,
                last_used: 0,
            });
            source.last_used = self.calls;
            let stale = source
                .clip
                .as_ref()
                .is_none_or(|c| !c.matches(origin, item.reverse, item.speed, item.keep_pitch));
            if source.broken {
                continue;
            }
            if stale {
                source.clip = AudioReader::open(&media.path)
                    .inspect_err(|e| tracing::warn!(path = %media.path.display(), error = %e, "can't open audio"))
                    .ok()
                    .map(|reader| {
                        let reader = Reader { reader, failed: false };
                        ClipAudio::new(reader, origin, item.reverse, item.speed, item.keep_pitch)
                    });
                source.broken = source.clip.is_none();
            }
            let Some(clip) = source.clip.as_mut() else {
                continue;
            };
            let n = ((b - a) as usize) * CHANNELS;
            self.scratch.resize(n, 0.0);
            clip.read(a - item_start, &mut self.scratch[..n]);
            let got = n;
            let offset = ((a - start) as usize) * CHANNELS;
            let look = &item.look;
            let faded = look.fade_in > rw_model::Time::ZERO || look.fade_out > rw_model::Time::ZERO;
            for (f, (o, s)) in out[offset..offset + got]
                .chunks_mut(CHANNELS)
                .zip(self.scratch[..got].chunks(CHANNELS))
                .enumerate()
            {
                let g = if faded {
                    gain * look.fade_at(sample_to_time(a + f as i64) - item.start, item.duration)
                } else {
                    gain
                };
                for (o, s) in o.iter_mut().zip(s) {
                    *o += s * g;
                }
            }
        }
        let calls = self.calls;
        self.sources.retain(|_, s| calls - s.last_used < IDLE_CALLS);
    }

    /// Drops all open readers (e.g. after the project's media changed).
    pub fn reset(&mut self) {
        self.sources.clear();
    }
}
