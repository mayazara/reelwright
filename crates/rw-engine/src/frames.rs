//! Evaluates the timeline at a time into compositor layers.

use rw_cache::ProjectCache;
use rw_media::VideoReader;
use rw_model::{Item, ItemId, MediaKind, MediaRef, Project, Time, TrackKind};
use rw_render::{Layer, fit_contain};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
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
pub fn decode_size(media: &MediaRef, out: (u32, u32)) -> (u32, u32) {
    let Some(v) = &media.info.video else {
        return out;
    };
    let display = v.display_size();
    let (_, _, w, h) = fit_contain(display, out);
    let (w, h) = (w.min(display.0 as f32), h.min(display.1 as f32));
    let even = |x: f32| ((x / 2.0).round() as u32).max(1) * 2;
    if v.rotation % 180 == 90 {
        (even(h), even(w))
    } else {
        (even(w), even(h))
    }
}

struct Entry {
    reader: Option<VideoReader>,
    last_used: u64,
}

/// Opens and caches one reader per item and returns its picture at a time.
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

    fn open(&self, media: &MediaRef, size: (u32, u32)) -> Option<VideoReader> {
        let still = media.kind() == MediaKind::Image;
        let proxy = self.cache.proxy(&media.hash);
        let path = if self.prefer_proxies && proxy.exists() {
            proxy
        } else {
            media.path.clone()
        };
        VideoReader::open(&path, Some(size), still)
            .inspect_err(|e| tracing::warn!(path = %path.display(), error = %e, "can't open video"))
            .ok()
    }

    /// Layers to composite at timeline time `t` into an output of `out`.
    pub fn layers_at(&mut self, project: &Project, t: Time, out: (u32, u32)) -> Vec<Layer> {
        self.calls += 1;
        let mut layers = Vec::new();
        for item in visible_items(project, t) {
            let Some(media) = item.media().and_then(|id| project.media(id)) else {
                continue;
            };
            if media.info.video.is_none() {
                continue;
            }
            let size = decode_size(media, out);
            let calls = self.calls;
            let needs_open = !self.readers.contains_key(&item.id);
            if needs_open {
                let reader = self.open(media, size);
                self.readers.insert(
                    item.id,
                    Entry {
                        reader,
                        last_used: calls,
                    },
                );
            }
            let entry = self.readers.get_mut(&item.id).expect("inserted above");
            entry.last_used = calls;
            let Some(reader) = entry.reader.as_mut() else {
                continue;
            };
            reader.set_out_size(size);
            let frame = match reader.frame_at(item.source_time(t)) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!(error = %e, "decode failed");
                    entry.reader = None;
                    continue;
                }
            };
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (item.id, frame.pts, size).hash(&mut h);
            let rotation = media.info.video.as_ref().map_or(0, |v| v.rotation);
            layers.push(Layer {
                image: Arc::new(frame.image.clone()),
                key: h.finish(),
                rotation,
                opacity: 1.0,
            });
        }
        let calls = self.calls;
        self.readers.retain(|_, e| calls - e.last_used < IDLE_CALLS);
        layers
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
        assert_eq!(decode_size(&video(3840, 2160, 0), (960, 540)), (960, 540));
        assert_eq!(decode_size(&video(640, 360, 0), (1920, 1080)), (640, 360));
        // Rotated portrait phone video in a 16:9 preview: display 1080x1920
        // fits as ~304x540, coded orientation is swapped.
        assert_eq!(decode_size(&video(1920, 1080, 90), (960, 540)), (540, 304));
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
