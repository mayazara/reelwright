//! Evaluates the timeline at a time into compositor layers.

use rw_cache::ProjectCache;
use rw_media::{VideoFrame, VideoReader};
use rw_model::{Fit, Generated, Item, ItemId, Look, MediaKind, MediaRef, Project, Time, TrackKind};
use rw_render::{Layer, Source};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

/// Readers idle for this many calls are closed.
const IDLE_CALLS: u64 = 300;

/// Items drawn at `t`, bottom-most first.
pub fn visible_items(project: &Project, t: Time) -> Vec<&Item> {
    project
        .tracks
        .iter()
        .filter(|tr| matches!(tr.kind, TrackKind::Video | TrackKind::Overlay) && !tr.hidden)
        .filter_map(|tr| tr.item_at(t))
        .collect()
}

/// Decode size for `media` drawn into an output of `out`: the coded-orientation
/// size that fills its letterboxed rectangle, never larger than the source.
///
/// Accounts for the clip's look: a small picture-in-picture box needs fewer
/// pixels, a tight crop needs more (the kept area is what fills the box).
pub fn decode_size(media: &MediaRef, out: (u32, u32), look: &Look) -> (u32, u32) {
    let Some(v) = &media.info.video else {
        return out;
    };
    let display = v.display_size();
    let t = look.transform;
    let (kx, ky) = look.crop.kept();
    let (bw, bh) = (t.size[0] * out.0 as f32, t.size[1] * out.1 as f32);
    let (cw, ch) = (display.0 as f32 * kx, display.1 as f32 * ky);
    // Size the kept area is drawn at, then scale back up to the whole frame.
    let s = match t.fit {
        Fit::Contain => (bw / cw).min(bh / ch),
        Fit::Cover => (bw / cw).max(bh / ch),
        Fit::Stretch => (bw / cw).max(bh / ch),
    };
    let w = (display.0 as f32 * s).min(display.0 as f32);
    let h = (display.1 as f32 * s).min(display.1 as f32);
    let even = |x: f32| ((x / 2.0).round() as u32).max(1) * 2;
    if v.rotation % 180 == 90 {
        (even(h), even(w))
    } else {
        (even(w), even(h))
    }
}

/// Clips starting this soon get their decoder opened early, so a cut never
/// waits on opening a file.
pub const PREWARM: Time = Time::from_secs(1);

struct Entry {
    reader: Option<VideoReader>,
    /// Whether opening was attempted (a failed open stays `None`).
    opened: bool,
    last_used: u64,
}

/// One decode to run: either a layer for this frame or a warm-up.
struct Want {
    /// Position among this frame's layers; `None` for a warm-up.
    slot: Option<usize>,
    item: ItemId,
    path: PathBuf,
    still: bool,
    size: (u32, u32),
    source_t: Time,
    rotation: u16,
}

/// Opens and caches one reader per item and returns its picture at a time.
/// The layers of a frame are decoded in parallel.
pub struct FrameSource {
    cache: ProjectCache,
    prefer_proxies: bool,
    readers: HashMap<ItemId, Entry>,
    calls: u64,
}

impl std::fmt::Debug for FrameSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameSource")
            .field("readers", &self.readers.len())
            .finish()
    }
}

fn decode(entry: &mut Entry, w: &Want) -> Option<Arc<VideoFrame>> {
    if !entry.opened {
        entry.opened = true;
        entry.reader = VideoReader::open(&w.path, Some(w.size), w.still)
            .inspect_err(
                |e| tracing::warn!(path = %w.path.display(), error = %e, "can't open video"),
            )
            .ok();
    }
    let reader = entry.reader.as_mut()?;
    reader.set_out_size(w.size);
    match reader.frame_at(w.source_t) {
        Ok(f) => Some(f),
        Err(e) => {
            tracing::warn!(error = %e, "decode failed");
            entry.reader = None;
            None
        }
    }
}

impl FrameSource {
    /// `prefer_proxies` decodes preview proxies where they exist (preview);
    /// export always reads the original files.
    pub fn new(cache: ProjectCache, prefer_proxies: bool) -> Self {
        FrameSource {
            cache,
            prefer_proxies,
            readers: HashMap::new(),
            calls: 0,
        }
    }

    fn want(
        &self,
        slot: Option<usize>,
        item: &Item,
        media: &MediaRef,
        source_t: Time,
        out: (u32, u32),
    ) -> Want {
        let proxy = self.cache.proxy(&media.hash);
        let path = if self.prefer_proxies && proxy.exists() {
            proxy
        } else {
            media.path.clone()
        };
        Want {
            slot,
            item: item.id,
            path,
            still: media.kind() == MediaKind::Image,
            size: decode_size(media, out, &item.look),
            source_t,
            rotation: media.info.video.as_ref().map_or(0, |v| v.rotation),
        }
    }

    /// Layers to composite at timeline time `t` into an output of `out`.
    pub fn layers_at(&mut self, project: &Project, t: Time, out: (u32, u32)) -> Vec<Layer> {
        self.calls += 1;
        let media_of = |item: &Item| {
            item.media()
                .and_then(|id| project.media(id))
                .filter(|m| m.info.video.is_some())
        };
        let mut wants = Vec::new();
        for (slot, item) in visible_items(project, t).into_iter().enumerate() {
            if let Some(m) = media_of(item) {
                wants.push(self.want(Some(slot), item, m, item.source_time(t), out));
            }
        }
        for track in project
            .tracks
            .iter()
            .filter(|tr| matches!(tr.kind, TrackKind::Video | TrackKind::Overlay) && !tr.hidden)
        {
            for item in track
                .items
                .iter()
                .filter(|i| i.start > t && i.start <= t + PREWARM)
            {
                if let Some(m) = media_of(item)
                    && self.readers.get(&item.id).is_none_or(|e| !e.opened)
                {
                    wants.push(self.want(None, item, m, item.source_in, out));
                }
            }
        }

        let calls = self.calls;
        for w in &wants {
            self.readers
                .entry(w.item)
                .or_insert(Entry {
                    reader: None,
                    opened: false,
                    last_used: calls,
                })
                .last_used = calls;
        }
        // Pair each want with its entry (distinct items, so distinct &muts).
        let mut by_item: HashMap<ItemId, &Want> = wants.iter().map(|w| (w.item, w)).collect();
        let jobs: Vec<(&mut Entry, &Want)> = self
            .readers
            .iter_mut()
            .filter_map(|(id, e)| by_item.remove(id).map(|w| (e, w)))
            .collect();

        let results: Vec<(&Want, Option<Arc<VideoFrame>>)> = if jobs.len() <= 1 {
            jobs.into_iter().map(|(e, w)| (w, decode(e, w))).collect()
        } else {
            std::thread::scope(|s| {
                let handles: Vec<_> = jobs
                    .into_iter()
                    .map(|(e, w)| s.spawn(move || (w, decode(e, w))))
                    .collect();
                handles.into_iter().filter_map(|h| h.join().ok()).collect()
            })
        };

        let mut decoded: HashMap<usize, Layer> = results
            .into_iter()
            .filter_map(|(w, frame)| {
                let (slot, frame) = (w.slot?, frame?);
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (w.item, frame.pts, w.size).hash(&mut h);
                Some((
                    slot,
                    Layer::image(Arc::new(frame.image.clone()), h.finish(), w.rotation),
                ))
            })
            .collect();
        // One layer per visible clip, bottom first, with its look; fades
        // fold into opacity.
        let mut layers: Vec<(usize, Layer)> = Vec::new();
        for (slot, item) in visible_items(project, t).into_iter().enumerate() {
            let mut layer = match item.generated_fill() {
                Some(Generated::Solid { color }) => Layer {
                    source: Source::Solid(color),
                    key: 0,
                    rotation: 0,
                    look: Look::default(),
                },
                Some(Generated::Gradient { from, to, angle }) => Layer {
                    source: Source::Gradient { from, to, angle },
                    key: 0,
                    rotation: 0,
                    look: Look::default(),
                },
                None => match decoded.remove(&slot) {
                    Some(l) => l,
                    None => continue,
                },
            };
            layer.look = item.look.clone();
            layer.look.opacity *= item.look.fade_at(t - item.start, item.duration);
            if layer.look.opacity > 0.0 {
                layers.push((slot, layer));
            }
        }
        self.readers.retain(|_, e| calls - e.last_used < IDLE_CALLS);
        layers.into_iter().map(|(_, l)| l).collect()
    }

    /// Drops all readers (e.g. when proxies become available).
    pub fn reset(&mut self) {
        self.readers.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rw_model::*;

    fn video(w: u32, h: u32, rotation: u16) -> MediaRef {
        MediaRef {
            id: MediaId::new(),
            name: "v".into(),
            path: "v".into(),
            hash: ContentHash("v".into()),
            size_bytes: 0,
            info: MediaInfo {
                kind: MediaKind::Video,
                container: "mp4".into(),
                duration: Time::from_secs(1),
                video: Some(VideoInfo {
                    codec: "h264".into(),
                    width: w,
                    height: h,
                    frame_rate: FrameRate::FPS_30,
                    pixel_format: "yuv420p".into(),
                    has_alpha: false,
                    rotation,
                }),
                audio: None,
            },
            imported_at: 0,
        }
    }

    #[test]
    fn decode_size_fits_output_without_upscaling() {
        assert_eq!(
            decode_size(&video(3840, 2160, 0), (960, 540), &Look::default()),
            (960, 540)
        );
        assert_eq!(
            decode_size(&video(640, 360, 0), (1920, 1080), &Look::default()),
            (640, 360)
        );
        // Rotated portrait phone video in a 16:9 preview: display 1080x1920
        // fits as ~304x540, coded orientation is swapped.
        assert_eq!(
            decode_size(&video(1920, 1080, 90), (960, 540), &Look::default()),
            (540, 304)
        );
    }

    #[test]
    fn decode_size_follows_box_and_crop() {
        let v = video(3840, 2160, 0);
        // Quarter-size picture-in-picture decodes at a quarter.
        let mut pip = Look::default();
        pip.transform.size = [0.25, 0.25];
        assert_eq!(decode_size(&v, (1920, 1080), &pip), (480, 270));
        // Keeping the middle half fills the frame from half the pixels: the
        // whole frame decodes at twice the output size.
        let cropped = Look {
            crop: rw_model::Crop {
                left: 0.25,
                right: 0.25,
                top: 0.25,
                bottom: 0.25,
            },
            ..Look::default()
        };
        assert_eq!(decode_size(&v, (1920, 1080), &cropped), (3840, 2160));
    }

    #[test]
    fn hidden_tracks_and_audio_tracks_are_not_drawn() {
        let m = video(64, 64, 0);
        let mut p = Project::new("p", Canvas::default());
        p.media.push(m.clone());
        let mut main = Track::new_main();
        main.items.push(Item::new(&m, Time::ZERO));
        let mut over = Track::new(TrackKind::Video);
        over.items.push(Item::new(&m, Time::ZERO));
        over.hidden = true;
        let mut audio = Track::new(TrackKind::Audio);
        audio.items.push(Item::new(&m, Time::ZERO));
        p.tracks = vec![main, over, audio];
        assert_eq!(visible_items(&p, Time::from_millis(500)).len(), 1);
        p.tracks[1].hidden = false;
        assert_eq!(visible_items(&p, Time::from_millis(500)).len(), 2);
        assert!(visible_items(&p, Time::from_secs(2)).is_empty());
    }
}
