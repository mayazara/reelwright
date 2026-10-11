//! Frame-accurate video and audio readers.
//!
//! Times are source times: zero is the start of the file (the container's
//! start time is subtracted), shared by the video and audio streams so their
//! relative offset is kept.

use crate::ffmpeg::{AV_TIME_BASE, decode_err, open, rotation, video_stream_index};
use crate::{MediaError, RgbaImage};
use ff::format::context::Input;
use ff::media::Type;
use ff::software::scaling;
use ff::util::format::Pixel;
use ffmpeg_next as ff;
use rw_model::Time;
use std::path::Path;
use std::sync::Arc;

/// Output sample rate of every audio reader.
pub const SAMPLE_RATE: u32 = 48_000;
/// Output channels (interleaved stereo).
pub const CHANNELS: usize = 2;

/// Seeking forward further than this is faster than decoding through.
const SEEK_THRESHOLD: Time = Time::from_secs(2);

/// A decoded picture and when it starts showing.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub pts: Time,
    /// Until the next frame starts (best effort).
    pub duration: Time,
    pub image: RgbaImage,
}

impl VideoFrame {
    pub fn covers(&self, t: Time) -> bool {
        self.pts <= t && t < self.pts + self.duration
    }
}

fn container_origin(ictx: &Input) -> Time {
    // SAFETY: reading a plain field of a live format context.
    let start = unsafe { (*ictx.as_ptr()).start_time };
    if start == ff::ffi::AV_NOPTS_VALUE || start < 0 {
        Time::ZERO
    } else {
        Time::from_rational(start, 1, AV_TIME_BASE)
    }
}

/// Reads pictures from a video, animation or still image.
pub struct VideoReader {
    ictx: Input,
    index: usize,
    decoder: ff::decoder::Video,
    tb: ff::Rational,
    origin: Time,
    frame_duration: Time,
    rotation: u16,
    out_size: (u32, u32),
    scaler: Option<(scaling::Context, Pixel, u32, u32)>,
    decoded: ff::frame::Video,
    current: Option<Arc<VideoFrame>>,
    /// The frame after `current`, decoded while looking for `current`'s end.
    pending: Option<Arc<VideoFrame>>,
    eof: bool,
    /// Still images hold their only frame forever.
    still: bool,
    /// Nothing decoded or sought yet: the demuxer is at the start.
    fresh: bool,
}

// SAFETY: FFmpeg's demuxer, decoder and scaler contexts have no thread
// affinity; they only require that one thread uses them at a time, which
// `&mut self` on every method guarantees. The raw pointers inside the
// wrappers are what make the type `!Send` by default.
unsafe impl Send for VideoReader {}

impl std::fmt::Debug for VideoReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoReader")
            .field("out_size", &self.out_size)
            .field("rotation", &self.rotation)
            .finish()
    }
}

impl VideoReader {
    /// Opens `path`, producing RGBA frames of `out_size` (coded orientation;
    /// apply [`Self::rotation`] when drawing). `None` keeps the coded size.
    pub fn open(
        path: &Path,
        out_size: Option<(u32, u32)>,
        still: bool,
    ) -> Result<Self, MediaError> {
        let ictx = open(path)?;
        let index = video_stream_index(&ictx).ok_or(MediaError::NoVideo)?;
        let (tb, rot, params, rate) = {
            let s = ictx.stream(index).ok_or(MediaError::NoVideo)?;
            (
                s.time_base(),
                rotation(&s),
                s.parameters(),
                s.avg_frame_rate(),
            )
        };
        let mut decoder = ff::codec::context::Context::from_parameters(params)
            .and_then(|c| c.decoder().video())
            .map_err(decode_err)?;
        let threads = std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .min(8);
        decoder.set_threading(ff::threading::Config {
            kind: ff::threading::Type::Frame,
            count: threads,
        });
        let frame_duration = if rate.numerator() > 0 && rate.denominator() > 0 {
            Time::from_rational(1, rate.denominator(), rate.numerator())
        } else {
            Time::from_millis(40)
        };
        let size = out_size.unwrap_or((decoder.width(), decoder.height()));
        Ok(VideoReader {
            origin: container_origin(&ictx),
            ictx,
            index,
            decoder,
            tb,
            frame_duration,
            rotation: rot,
            out_size: (size.0.max(1), size.1.max(1)),
            scaler: None,
            decoded: ff::frame::Video::empty(),
            current: None,
            pending: None,
            eof: false,
            still,
            fresh: true,
        })
    }

    pub fn rotation(&self) -> u16 {
        self.rotation
    }

    pub fn out_size(&self) -> (u32, u32) {
        self.out_size
    }

    /// Changes the output size; cached frames are dropped.
    pub fn set_out_size(&mut self, size: (u32, u32)) {
        let size = (size.0.max(1), size.1.max(1));
        if size != self.out_size {
            self.out_size = size;
            self.scaler = None;
            if self.still {
                // Re-decode the single frame at the new size.
                self.current = None;
                self.pending = None;
                self.eof = false;
                let _ = self.ictx.seek(0, ..0);
                self.decoder.flush();
            } else {
                self.current = None;
                self.pending = None;
            }
        }
    }

    fn pts_to_time(&self, pts: i64) -> Time {
        Time::from_rational(pts, self.tb.numerator(), self.tb.denominator().max(1)) - self.origin
    }

    fn convert(&mut self) -> Result<RgbaImage, MediaError> {
        let f = &self.decoded;
        let (w, h) = self.out_size;
        let key = (f.format(), f.width(), f.height());
        if self
            .scaler
            .as_ref()
            .is_none_or(|(_, p, sw, sh)| (*p, *sw, *sh) != key)
        {
            let mut ctx = scaling::Context::get(
                key.0,
                key.1,
                key.2,
                Pixel::RGBA,
                w,
                h,
                scaling::Flags::BILINEAR,
            )
            .map_err(decode_err)?;
            set_colorspace(&mut ctx, f);
            self.scaler = Some((ctx, key.0, key.1, key.2));
        }
        let (scaler, ..) = self.scaler.as_mut().expect("just set");
        let mut out = ff::frame::Video::empty();
        scaler.run(f, &mut out).map_err(decode_err)?;
        let stride = out.stride(0);
        let row = w as usize * 4;
        let data = out.data(0);
        let mut pixels = Vec::with_capacity(row * h as usize);
        for y in 0..h as usize {
            pixels.extend_from_slice(&data[y * stride..y * stride + row]);
        }
        Ok(RgbaImage::new(w, h, pixels))
    }

    /// Decodes the next frame in stream order.
    fn decode_next(&mut self) -> Result<Option<Arc<VideoFrame>>, MediaError> {
        if let Some(p) = self.pending.take() {
            return Ok(Some(p));
        }
        loop {
            if self.decoder.receive_frame(&mut self.decoded).is_ok() {
                let pts = self.decoded.timestamp().map(|p| self.pts_to_time(p));
                let image = self.convert()?;
                let pts = pts.unwrap_or_else(|| {
                    self.current
                        .as_ref()
                        .map_or(Time::ZERO, |c| c.pts + c.duration)
                });
                return Ok(Some(Arc::new(VideoFrame {
                    pts,
                    duration: self.frame_duration,
                    image,
                })));
            }
            if self.eof {
                return Ok(None);
            }
            let mut sent = false;
            while let Some((stream, packet)) = self.ictx.packets().next() {
                if stream.index() == self.index {
                    // Corrupt packets are skipped rather than failing playback.
                    let _ = self.decoder.send_packet(&packet);
                    sent = true;
                    break;
                }
            }
            if !sent {
                self.eof = true;
                let _ = self.decoder.send_eof();
            }
        }
    }

    fn seek(&mut self, t: Time) {
        let ts = (t + self.origin).to_rational(1, AV_TIME_BASE);
        let _ = self.ictx.seek(ts, ..ts);
        self.decoder.flush();
        self.current = None;
        self.pending = None;
        self.eof = false;
    }

    /// The frame showing at source time `t`. Past the end, the last frame.
    pub fn frame_at(&mut self, t: Time) -> Result<Arc<VideoFrame>, MediaError> {
        if self.still {
            if self.current.is_none() {
                let f = self
                    .decode_next()?
                    .ok_or_else(|| MediaError::Decode("no frames".into()))?;
                self.current = Some(Arc::new(VideoFrame {
                    pts: Time::ZERO,
                    duration: Time(i64::MAX / 4),
                    image: f.image.clone(),
                }));
            }
            return Ok(self.current.clone().expect("set above"));
        }
        let t = t.max(Time::ZERO);
        let need_seek = match &self.current {
            Some(c) => t < c.pts || t > c.pts + SEEK_THRESHOLD,
            None => true,
        };
        // A fresh reader is already at the start; skip the redundant seek.
        if need_seek && !(self.fresh && t < SEEK_THRESHOLD) {
            self.seek(t);
        }
        self.fresh = false;
        loop {
            if let Some(c) = &self.current
                && c.covers(t)
            {
                return Ok(c.clone());
            }
            match self.decode_next()? {
                Some(next) if next.pts <= t => {
                    // Fix up the previous frame's real duration.
                    self.current = Some(next);
                }
                Some(next) => {
                    // `next` starts after t: the current frame (if any) is the
                    // one showing, however long its nominal duration.
                    match self.current.take() {
                        Some(c) => {
                            let fixed = Arc::new(VideoFrame {
                                duration: next.pts - c.pts,
                                ..(*c).clone()
                            });
                            self.current = Some(fixed.clone());
                            self.pending = Some(next);
                            return Ok(fixed);
                        }
                        // Seeked before the first frame: show the first one.
                        None => {
                            self.current = Some(next.clone());
                            return Ok(next);
                        }
                    }
                }
                None => {
                    return self
                        .current
                        .clone()
                        .ok_or_else(|| MediaError::Decode("no frames decoded".into()));
                }
            }
        }
    }
}

// Binding constant types vary by platform, so the cast stays.
#[allow(clippy::unnecessary_cast)]
/// Sets the YUV→RGB matrix from the frame's tags (BT.709 for HD when
/// untagged, BT.601 otherwise) and its range.
fn set_colorspace(ctx: &mut scaling::Context, f: &ff::frame::Video) {
    use ff::color::{Range, Space};
    let cs = match f.color_space() {
        Space::BT709 => ff::ffi::SWS_CS_ITU709,
        Space::BT470BG | Space::SMPTE170M => ff::ffi::SWS_CS_ITU601,
        Space::BT2020NCL | Space::BT2020CL => ff::ffi::SWS_CS_BT2020,
        _ if f.height() >= 720 => ff::ffi::SWS_CS_ITU709,
        _ => ff::ffi::SWS_CS_ITU601,
    } as i32;
    let src_full = matches!(f.color_range(), Range::JPEG)
        || matches!(
            f.format(),
            Pixel::YUVJ420P | Pixel::YUVJ422P | Pixel::YUVJ444P
        );
    // SAFETY: `ctx` is a live scaler; sws_getCoefficients returns pointers to
    // static tables. The call only reconfigures the scaler's matrices.
    unsafe {
        let table = ff::ffi::sws_getCoefficients(cs);
        ff::ffi::sws_setColorspaceDetails(
            ctx.as_mut_ptr(),
            table,
            i32::from(src_full),
            table,
            1,
            0,
            1 << 16,
            1 << 16,
        );
    }
}

/// Converts decoded audio to interleaved stereo `f32` at [`SAMPLE_RATE`].
///
/// Uses libswresample directly with explicit channel layouts: the binding's
/// wrapper chooses between old and new layout APIs by FFmpeg version and
/// rejects frames whose layout has "unspecified" order (PCM in WAV) on some
/// versions.
struct Resampler {
    ctx: *mut ff::ffi::SwrContext,
    /// (sample format, sample rate, channel count) of the input it was made for.
    key: (i32, i32, i32),
    out: Vec<f32>,
}

impl Resampler {
    fn key(frame: &ff::frame::Audio) -> (i32, i32, i32) {
        // SAFETY: reading plain fields of a live frame.
        unsafe {
            let f = frame.as_ptr();
            ((*f).format, (*f).sample_rate, (*f).ch_layout.nb_channels)
        }
    }

    fn new(frame: &ff::frame::Audio, fallback_channels: i32) -> Result<Self, MediaError> {
        let key = Self::key(frame);
        // SAFETY: layouts are initialised before use and uninitialised after;
        // `ctx` is checked for null and owned by the returned value. The
        // input sample format comes from a decoded frame, so it is a valid
        // `AVSampleFormat` discriminant.
        unsafe {
            let f = frame.as_ptr();
            let mut in_layout: ff::ffi::AVChannelLayout = std::mem::zeroed();
            if (*f).ch_layout.order == ff::ffi::AVChannelOrder::AV_CHANNEL_ORDER_UNSPEC
                || (*f).ch_layout.nb_channels <= 0
            {
                let n = if (*f).ch_layout.nb_channels > 0 {
                    (*f).ch_layout.nb_channels
                } else {
                    fallback_channels
                };
                ff::ffi::av_channel_layout_default(&mut in_layout, n);
            } else if ff::ffi::av_channel_layout_copy(&mut in_layout, &(*f).ch_layout) < 0 {
                return Err(MediaError::Decode("unusable channel layout".into()));
            }
            let mut out_layout: ff::ffi::AVChannelLayout = std::mem::zeroed();
            ff::ffi::av_channel_layout_default(&mut out_layout, CHANNELS as i32);
            let mut ctx: *mut ff::ffi::SwrContext = std::ptr::null_mut();
            let in_format: ff::ffi::AVSampleFormat = std::mem::transmute((*f).format);
            let r = ff::ffi::swr_alloc_set_opts2(
                &mut ctx,
                &out_layout,
                ff::ffi::AVSampleFormat::AV_SAMPLE_FMT_FLT,
                SAMPLE_RATE as i32,
                &in_layout,
                in_format,
                (*f).sample_rate,
                0,
                std::ptr::null_mut(),
            );
            ff::ffi::av_channel_layout_uninit(&mut in_layout);
            ff::ffi::av_channel_layout_uninit(&mut out_layout);
            if r < 0 || ctx.is_null() || ff::ffi::swr_init(ctx) < 0 {
                if !ctx.is_null() {
                    ff::ffi::swr_free(&mut ctx);
                }
                return Err(MediaError::Decode(format!(
                    "can't convert audio ({} Hz, {} channels)",
                    (*f).sample_rate,
                    (*f).ch_layout.nb_channels
                )));
            }
            Ok(Resampler {
                ctx,
                key,
                out: Vec::new(),
            })
        }
    }

    /// Converts one frame into `self.out`; returns sample frames written.
    fn convert(&mut self, frame: &ff::frame::Audio) -> Result<usize, MediaError> {
        // SAFETY: `ctx` is a live initialised context; the output buffer has
        // room for `capacity` stereo frames; input pointers come from a live
        // decoded frame. `as _` adapts to the pointer constness, which
        // differs between FFmpeg versions.
        unsafe {
            let f = frame.as_ptr();
            let in_samples = (*f).nb_samples;
            let capacity = ff::ffi::swr_get_out_samples(self.ctx, in_samples).max(0) as usize + 64;
            self.out.resize(capacity * CHANNELS, 0.0);
            let mut out_ptr = self.out.as_mut_ptr() as *mut u8;
            let n = ff::ffi::swr_convert(
                self.ctx,
                &mut out_ptr as *mut *mut u8 as _,
                capacity as i32,
                (*f).extended_data as _,
                in_samples,
            );
            if n < 0 {
                return Err(decode_err(ff::Error::from(n)));
            }
            Ok(n as usize)
        }
    }
}

impl Drop for Resampler {
    fn drop(&mut self) {
        // SAFETY: `ctx` was allocated by swr_alloc_set_opts2 and is freed once.
        unsafe { ff::ffi::swr_free(&mut self.ctx) }
    }
}

/// Reads audio as interleaved stereo `f32` at [`SAMPLE_RATE`].
pub struct AudioReader {
    ictx: Input,
    index: usize,
    decoder: ff::decoder::Audio,
    tb: ff::Rational,
    origin: Time,
    resampler: Option<Resampler>,
    decoded: ff::frame::Audio,
    /// Converted samples not yet returned.
    buffer: Vec<f32>,
    /// Source time of `buffer[0]`.
    position: Time,
    /// Drop output before this time (after a seek).
    skip_until: Option<Time>,
    eof: bool,
}

// SAFETY: as for `VideoReader`: exclusive access via `&mut self`, and the
// demuxer, decoder and resampler contexts are not tied to a thread.
unsafe impl Send for AudioReader {}

impl std::fmt::Debug for AudioReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioReader")
            .field("position", &self.position)
            .finish()
    }
}

fn sample_time(n: usize) -> Time {
    Time::from_rational(n as i64, 1, SAMPLE_RATE as i32)
}

impl AudioReader {
    pub fn open(path: &Path) -> Result<Self, MediaError> {
        let ictx = open(path)?;
        let stream = ictx
            .streams()
            .best(Type::Audio)
            .ok_or_else(|| MediaError::Unsupported("no audio in this file".into()))?;
        let index = stream.index();
        let tb = stream.time_base();
        let decoder = ff::codec::context::Context::from_parameters(stream.parameters())
            .and_then(|c| c.decoder().audio())
            .map_err(decode_err)?;
        Ok(AudioReader {
            origin: container_origin(&ictx),
            ictx,
            index,
            decoder,
            tb,
            resampler: None,
            decoded: ff::frame::Audio::empty(),
            buffer: Vec::new(),
            position: Time::ZERO,
            skip_until: None,
            eof: false,
        })
    }

    /// Source time of the next sample [`Self::read`] returns.
    pub fn position(&self) -> Time {
        self.position
    }

    /// Positions the reader so the next sample read is at source time `t`.
    pub fn seek(&mut self, t: Time) {
        let t = t.max(Time::ZERO);
        let ts = (t + self.origin).to_rational(1, AV_TIME_BASE);
        let _ = self.ictx.seek(ts, ..ts);
        self.decoder.flush();
        self.resampler = None;
        self.buffer.clear();
        self.eof = false;
        self.position = t;
        self.skip_until = Some(t);
    }

    fn resample(&mut self) -> Result<(), MediaError> {
        let fallback = i32::from(self.decoder.channels()).max(1);
        let key = Resampler::key(&self.decoded);
        if self.resampler.as_ref().is_none_or(|r| r.key != key) {
            self.resampler = Some(Resampler::new(&self.decoded, fallback)?);
        }
        let r = self.resampler.as_mut().expect("just set");
        let n = r.convert(&self.decoded)?;
        let out = std::mem::take(&mut r.out);
        self.push(&out[..n * CHANNELS]);
        if let Some(r) = self.resampler.as_mut() {
            r.out = out;
        }
        Ok(())
    }

    fn push(&mut self, samples: &[f32]) {
        let mut samples = samples;
        if let Some(target) = self.skip_until {
            if self.position > target {
                // Decoding resumed after the target: fill the hole with silence.
                let pad =
                    (self.position - target).to_rational(1, SAMPLE_RATE as i32) as usize * CHANNELS;
                self.buffer.resize(self.buffer.len() + pad, 0.0);
                self.position = target;
                self.skip_until = None;
            } else {
                let behind = ((target - self.position).to_rational(1, SAMPLE_RATE as i32) as usize
                    * CHANNELS)
                    .min(samples.len());
                samples = &samples[behind..];
                self.position += sample_time(behind / CHANNELS);
                if samples.is_empty() {
                    return;
                }
                self.skip_until = None;
            }
        }
        self.buffer.extend_from_slice(samples);
    }

    fn fill(&mut self) -> Result<bool, MediaError> {
        loop {
            if self.decoder.receive_frame(&mut self.decoded).is_ok() {
                if self.skip_until.is_some()
                    && let Some(pts) = self.decoded.timestamp()
                    && self.buffer.is_empty()
                {
                    self.position =
                        Time::from_rational(pts, self.tb.numerator(), self.tb.denominator().max(1))
                            - self.origin;
                }
                self.resample()?;
                return Ok(true);
            }
            if self.eof {
                return Ok(false);
            }
            let mut sent = false;
            while let Some((stream, packet)) = self.ictx.packets().next() {
                if stream.index() == self.index {
                    let _ = self.decoder.send_packet(&packet);
                    sent = true;
                    break;
                }
            }
            if !sent {
                self.eof = true;
                let _ = self.decoder.send_eof();
            }
        }
    }

    /// Fills `out` (interleaved stereo) from the current position. Returns
    /// the number of floats written; fewer than `out.len()` means the end
    /// of the stream.
    pub fn read(&mut self, out: &mut [f32]) -> Result<usize, MediaError> {
        while self.buffer.len() < out.len() {
            if !self.fill()? {
                break;
            }
        }
        let n = self.buffer.len().min(out.len());
        out[..n].copy_from_slice(&self.buffer[..n]);
        self.buffer.drain(..n);
        self.position += sample_time(n / CHANNELS);
        Ok(n)
    }
}
