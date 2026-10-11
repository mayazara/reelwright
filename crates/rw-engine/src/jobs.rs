//! Background media jobs: probing imports, thumbnails and proxies.
//!
//! Light jobs (probe, thumbnail) share a small worker pool; proxies run on a
//! single dedicated worker so a long transcode never blocks thumbnails.
//! Results arrive as [`MediaEvent`]s that the UI drains each frame.

use crate::import::{ensure_proxy, ensure_thumbnail, probe_file};
use crossbeam_channel::{Receiver, Sender, unbounded};
use rw_cache::ProjectCache;
use rw_media::{MediaError, MediaIo};
use rw_model::{ContentHash, MediaRef};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A finished (or progressing) background job.
#[derive(Debug)]
pub enum MediaEvent {
    /// Every file in an import batch has been probed. `media` keeps the
    /// order the paths were given in.
    Imported {
        batch: u64,
        media: Vec<MediaRef>,
        failed: Vec<(PathBuf, String)>,
    },
    Thumbnail {
        hash: ContentHash,
        result: Result<Option<PathBuf>, String>,
    },
    ProxyProgress {
        hash: ContentHash,
        fraction: f32,
    },
    Proxy {
        hash: ContentHash,
        result: Result<Option<PathBuf>, String>,
    },
}

type Task = Box<dyn FnOnce() + Send>;
/// Called after each event is queued, e.g. to request a UI repaint.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

struct Shared {
    io: Arc<dyn MediaIo>,
    events: Sender<MediaEvent>,
    waker: Waker,
    pending: AtomicUsize,
}

impl Shared {
    fn emit(&self, event: MediaEvent) {
        let _ = self.events.send(event);
        (self.waker)();
    }
}

/// Handle to the background media workers.
pub struct MediaJobs {
    shared: Arc<Shared>,
    light: Sender<Task>,
    heavy: Sender<Task>,
    events: Receiver<MediaEvent>,
    cancel: Mutex<Arc<AtomicBool>>,
    next_batch: AtomicU64,
}

impl MediaJobs {
    pub fn new(io: Arc<dyn MediaIo>, waker: Waker) -> Self {
        let (events_tx, events_rx) = unbounded();
        let shared = Arc::new(Shared {
            io,
            events: events_tx,
            waker,
            pending: AtomicUsize::new(0),
        });
        let (light_tx, light_rx) = unbounded::<Task>();
        let (heavy_tx, heavy_rx) = unbounded::<Task>();
        let workers = std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .clamp(1, 4);
        for i in 0..workers {
            spawn_worker(format!("media-light-{i}"), light_rx.clone());
        }
        spawn_worker("media-proxy".into(), heavy_rx);
        MediaJobs {
            shared,
            light: light_tx,
            heavy: heavy_tx,
            events: events_rx,
            cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            next_batch: AtomicU64::new(1),
        }
    }

    fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn submit(&self, heavy: bool, task: impl FnOnce(&Shared) + Send + 'static) {
        let shared = self.shared.clone();
        shared.pending.fetch_add(1, Ordering::SeqCst);
        let job: Task = Box::new(move || {
            task(&shared);
            shared.pending.fetch_sub(1, Ordering::SeqCst);
            (shared.waker)();
        });
        let queue = if heavy { &self.heavy } else { &self.light };
        if queue.send(job).is_err() {
            self.shared.pending.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Probes files in parallel; one [`MediaEvent::Imported`] arrives when
    /// all are done. Returns the batch id.
    pub fn import(&self, paths: Vec<PathBuf>) -> u64 {
        let batch = self.next_batch.fetch_add(1, Ordering::SeqCst);
        let total = paths.len();
        if total == 0 {
            self.shared.emit(MediaEvent::Imported {
                batch,
                media: Vec::new(),
                failed: Vec::new(),
            });
            return batch;
        }
        type Slot = Option<Result<MediaRef, (PathBuf, String)>>;
        let results: Arc<Mutex<Vec<Slot>>> =
            Arc::new(Mutex::new((0..total).map(|_| None).collect()));
        let remaining = Arc::new(AtomicUsize::new(total));
        for (i, path) in paths.into_iter().enumerate() {
            let results = results.clone();
            let remaining = remaining.clone();
            self.submit(false, move |s| {
                let r = probe_file(s.io.as_ref(), &path).map_err(|e| (path.clone(), e.to_string()));
                if let Err((p, e)) = &r {
                    tracing::warn!(path = %p.display(), error = %e, "import failed");
                }
                results.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(r);
                if remaining.fetch_sub(1, Ordering::SeqCst) == 1 {
                    let slots =
                        std::mem::take(&mut *results.lock().unwrap_or_else(|e| e.into_inner()));
                    let (mut media, mut failed) = (Vec::new(), Vec::new());
                    for slot in slots.into_iter().flatten() {
                        match slot {
                            Ok(m) => media.push(m),
                            Err(f) => failed.push(f),
                        }
                    }
                    s.emit(MediaEvent::Imported {
                        batch,
                        media,
                        failed,
                    });
                }
            });
        }
        batch
    }

    /// Generates the thumbnail and, if needed, the proxy for `media`.
    pub fn prepare(&self, cache: &ProjectCache, media: &MediaRef) {
        let (c, m) = (cache.clone(), media.clone());
        self.submit(false, move |s| {
            let result = ensure_thumbnail(s.io.as_ref(), &c, &m).map_err(|e| e.to_string());
            s.emit(MediaEvent::Thumbnail {
                hash: m.hash.clone(),
                result,
            });
        });
        if rw_media::needs_proxy(&media.info) {
            let (c, m) = (cache.clone(), media.clone());
            let cancel = self.cancel_flag();
            self.submit(true, move |s| {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let hash = m.hash.clone();
                let progress = |fraction: f32| {
                    s.emit(MediaEvent::ProxyProgress {
                        hash: hash.clone(),
                        fraction,
                    });
                };
                let result = ensure_proxy(s.io.as_ref(), &c, &m, &cancel, &progress);
                if matches!(result, Err(MediaError::Cancelled)) {
                    return;
                }
                s.emit(MediaEvent::Proxy {
                    hash,
                    result: result.map_err(|e| e.to_string()),
                });
            });
        }
    }

    /// Cancels queued and running proxy jobs, e.g. when a project closes.
    /// Light jobs are short and simply finish.
    pub fn cancel_all(&self) {
        let mut flag = self.cancel.lock().unwrap_or_else(|e| e.into_inner());
        flag.store(true, Ordering::Relaxed);
        *flag = Arc::new(AtomicBool::new(false));
    }

    /// Next finished event, if any.
    pub fn try_recv(&self) -> Option<MediaEvent> {
        self.events.try_recv().ok()
    }

    /// Blocks for the next event, up to `timeout`.
    pub fn recv_timeout(&self, timeout: std::time::Duration) -> Option<MediaEvent> {
        self.events.recv_timeout(timeout).ok()
    }

    /// Number of jobs queued or running.
    pub fn pending(&self) -> usize {
        self.shared.pending.load(Ordering::SeqCst)
    }
}

impl Drop for MediaJobs {
    fn drop(&mut self) {
        // Workers exit once their queue is closed; a running proxy stops at
        // its next packet.
        self.cancel_all();
    }
}

fn spawn_worker(name: String, rx: Receiver<Task>) {
    std::thread::Builder::new()
        .name(name)
        .spawn(move || {
            while let Ok(task) = rx.recv() {
                task();
            }
        })
        .expect("failed to spawn media worker");
}
