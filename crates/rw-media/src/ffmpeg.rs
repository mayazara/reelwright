//! FFmpeg implementation of [`MediaIo`].

use crate::{MediaError, MediaIo, Progress, RgbaImage};
use ff::format::{context::Input, stream::Disposition};
use ff::media::Type;
use ff::software::scaling;
use ff::util::format::Pixel;
use ffmpeg_next as ff;
use rw_model::{AudioInfo, FrameRate, MediaInfo, MediaKind, Time, VideoInfo};
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

/// FFmpeg's internal time base for container-level durations and seeks.
const AV_TIME_BASE: i32 = 1_000_000;

/// Demuxers that read single still images.
const IMAGE_DEMUXERS: &[&str] = &[
    "image2",
    "png_pipe",
    "jpeg_pipe",
    "bmp_pipe",
    "tiff_pipe",
    "webp_pipe",
];
/// Demuxers whose files may be still or animated.
const ANIMATION_DEMUXERS: &[&str] = &["gif", "apng", "webp_pipe"];

/// Decoded frames to look through for a thumbnail before settling for the
/// latest one.
const THUMBNAIL_FRAME_BUDGET: usize = 240;

static INIT: Once = Once::new();

/// The FFmpeg-backed media library.
#[derive(Debug, Clone, Copy)]
pub struct Ffmpeg {
    _private: (),
}

impl Ffmpeg {
    pub fn new() -> Self {
        INIT.call_once(|| {
            ff::init().expect("FFmpeg failed to initialise");
            ff::util::log::set_level(ff::util::log::Level::Error);
        });
        Ffmpeg { _private: () }
    }
}

impl Default for Ffmpeg {
    fn default() -> Self {
        Self::new()
    }
}

fn open(path: &Path) -> Result<Input, MediaError> {
    ff::format::input(path).map_err(|e| MediaError::Open {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

fn decode_err(e: ff::Error) -> MediaError {
    MediaError::Decode(e.to_string())
}

fn encode_err(e: ff::Error) -> MediaError {
    MediaError::Encode(e.to_string())
}

/// Index of the main video stream, ignoring embedded cover art.
fn video_stream_index(ictx: &Input) -> Option<usize> {
    let is_picture = |s: &ff::format::stream::Stream| {
        s.parameters().medium() == Type::Video
            && !s.disposition().contains(Disposition::ATTACHED_PIC)
    };
    match ictx.streams().best(Type::Video) {
        Some(s) if is_picture(&s) => Some(s.index()),
        _ => ictx.streams().find(is_picture).map(|s| s.index()),
    }
}

fn has_alpha(format: Pixel) -> bool {
    format.descriptor().is_some_and(|d| {
        // SAFETY: `descriptor()` only returns non-null pointers to FFmpeg's
        // static pixel-format table, which lives for the whole program.
        let flags = unsafe { (*d.as_ptr()).flags };
        flags & u64::from(ff::ffi::AV_PIX_FMT_FLAG_ALPHA as u32) != 0
    })
}

/// Clockwise display rotation from the stream's display matrix, snapped to
/// a multiple of 90 degrees.
fn rotation(stream: &ff::format::stream::Stream) -> u16 {
    // SAFETY: `stream` borrows a live AVStream; its `codecpar` and side-data
    // array are owned by the format context and valid for the borrow. We only
    // read, and check the side-data size before reading the 3x3 matrix.
    let angle = unsafe {
        let par = (*stream.as_ptr()).codecpar;
        if par.is_null() {
            return 0;
        }
        let sd = ff::ffi::av_packet_side_data_get(
            (*par).coded_side_data,
            (*par).nb_coded_side_data,
            ff::ffi::AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX,
        );
        if sd.is_null() || (*sd).size < 9 * 4 {
            return 0;
        }
        ff::ffi::av_display_rotation_get((*sd).data as *const i32)
    };
    if !angle.is_finite() {
        return 0;
    }
    // FFmpeg reports counter-clockwise degrees.
    let cw = (-angle / 90.0).round() as i64 * 90;
    cw.rem_euclid(360) as u16
}

fn frame_rate(stream: &ff::format::stream::Stream) -> FrameRate {
    for r in [stream.avg_frame_rate(), stream.rate()] {
        if r.numerator() > 0 && r.denominator() > 0 {
            return FrameRate::new(r.numerator() as u32, r.denominator() as u32);
        }
    }
    FrameRate::FPS_30
}

fn video_info(stream: &ff::format::stream::Stream) -> Result<VideoInfo, MediaError> {
    let params = stream.parameters();
    let codec = params.id().name().to_owned();
    let dec = ff::codec::context::Context::from_parameters(params)
        .and_then(|c| c.decoder().video())
        .map_err(decode_err)?;
    let format = dec.format();
    Ok(VideoInfo {
        codec,
        width: dec.width(),
        height: dec.height(),
        frame_rate: frame_rate(stream),
        pixel_format: format
            .descriptor()
            .map_or_else(|| "unknown".to_owned(), |d| d.name().to_owned()),
        has_alpha: has_alpha(format),
        rotation: rotation(stream),
    })
}

fn audio_info(stream: &ff::format::stream::Stream) -> Result<AudioInfo, MediaError> {
    let params = stream.parameters();
    let codec = params.id().name().to_owned();
    let dec = ff::codec::context::Context::from_parameters(params)
        .and_then(|c| c.decoder().audio())
        .map_err(decode_err)?;
    Ok(AudioInfo {
        codec,
        sample_rate: dec.rate(),
        channels: dec.channels(),
    })
}

fn stream_duration(stream: &ff::format::stream::Stream) -> Time {
    let tb = stream.time_base();
    if stream.duration() > 0 && tb.denominator() > 0 {
        Time::from_rational(stream.duration(), tb.numerator(), tb.denominator())
    } else {
        Time::ZERO
    }
}

/// Counts packets of one stream, stopping at `limit`, and sums their
/// durations.
fn count_packets(ictx: &mut Input, index: usize, limit: usize) -> (usize, Time) {
    let tb = ictx.stream(index).map(|s| s.time_base());
    let mut count = 0;
    let mut total = Time::ZERO;
    for (stream, packet) in ictx.packets() {
        if stream.index() != index {
            continue;
        }
        count += 1;
        if let Some(tb) = tb
            && tb.denominator() > 0
        {
            total += Time::from_rational(packet.duration(), tb.numerator(), tb.denominator());
        }
        if count >= limit {
            break;
        }
    }
    (count, total)
}

/// Scales a decoded frame to RGBA at the given size.
fn to_rgba(frame: &ff::frame::Video, width: u32, height: u32) -> Result<RgbaImage, MediaError> {
    let mut scaler = scaling::Context::get(
        frame.format(),
        frame.width(),
        frame.height(),
        Pixel::RGBA,
        width,
        height,
        scaling::Flags::BILINEAR,
    )
    .map_err(decode_err)?;
    let mut out = ff::frame::Video::empty();
    scaler.run(frame, &mut out).map_err(decode_err)?;
    let stride = out.stride(0);
    let row = width as usize * 4;
    let data = out.data(0);
    let mut pixels = Vec::with_capacity(row * height as usize);
    for y in 0..height as usize {
        pixels.extend_from_slice(&data[y * stride..y * stride + row]);
    }
    Ok(RgbaImage::new(width, height, pixels))
}

/// Size that fits `(w, h)` inside a `max`×`max` box, never upscaling.
fn fit(w: u32, h: u32, max: u32) -> (u32, u32) {
    let scale = (f64::from(max) / f64::from(w.max(h))).min(1.0);
    let s = |v: u32| ((f64::from(v) * scale).round() as u32).max(1);
    (s(w), s(h))
}

fn even(v: f64) -> u32 {
    ((v / 2.0).round() as u32).max(1) * 2
}

impl MediaIo for Ffmpeg {
    fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError> {
        let mut ictx = open(path)?;
        let demuxer = ictx.format().name().to_owned();
        let container = demuxer.split(',').next().unwrap_or_default().to_owned();
        let video_index = video_stream_index(&ictx);
        let video = match video_index.and_then(|i| ictx.stream(i)) {
            Some(s) => Some(video_info(&s)?),
            None => None,
        };
        let audio = match ictx.streams().best(Type::Audio) {
            Some(s) => Some(audio_info(&s)?),
            None => None,
        };
        let mut duration = if ictx.duration() > 0 {
            Time::from_rational(ictx.duration(), 1, AV_TIME_BASE)
        } else {
            ictx.streams()
                .map(|s| stream_duration(&s))
                .max()
                .unwrap_or(Time::ZERO)
        };

        let kind = match (video_index, &audio) {
            (Some(vi), _) if ANIMATION_DEMUXERS.contains(&demuxer.as_str()) => {
                let (frames, total) = count_packets(&mut ictx, vi, usize::MAX);
                if frames > 1 {
                    if duration <= Time::ZERO {
                        duration = total;
                    }
                    MediaKind::Animation
                } else {
                    MediaKind::Image
                }
            }
            (Some(_), _) if IMAGE_DEMUXERS.contains(&demuxer.as_str()) => MediaKind::Image,
            (Some(_), _) => MediaKind::Video,
            (None, Some(_)) => MediaKind::Audio,
            (None, None) => {
                return Err(MediaError::Unsupported(format!(
                    "{} has no audio or video",
                    path.display()
                )));
            }
        };
        if kind == MediaKind::Image {
            duration = Time::ZERO;
        }
        Ok(MediaInfo {
            kind,
            container,
            duration,
            video,
            audio,
        })
    }

    fn thumbnail(&self, path: &Path, at: Time, max_size: u32) -> Result<RgbaImage, MediaError> {
        let mut ictx = open(path)?;
        let index = video_stream_index(&ictx).ok_or(MediaError::NoVideo)?;
        let (tb, rot, params) = {
            let s = ictx.stream(index).ok_or(MediaError::NoVideo)?;
            (s.time_base(), rotation(&s), s.parameters())
        };
        let mut dec = ff::codec::context::Context::from_parameters(params)
            .and_then(|c| c.decoder().video())
            .map_err(decode_err)?;
        if at > Time::ZERO {
            let ts = at.to_rational(1, AV_TIME_BASE);
            // A failed seek just means we take an earlier frame.
            let _ = ictx.seek(ts, ..ts);
        }
        let target = at.to_rational(tb.numerator(), tb.denominator().max(1));

        let mut frame = ff::frame::Video::empty();
        let mut chosen: Option<ff::frame::Video> = None;
        let mut decoded = 0usize;
        let mut done = false;
        let mut take = |dec: &mut ff::decoder::Video, chosen: &mut Option<ff::frame::Video>| {
            while dec.receive_frame(&mut frame).is_ok() {
                decoded += 1;
                let pts = frame.timestamp().unwrap_or(i64::MAX);
                *chosen = Some(frame.clone());
                if pts >= target || decoded >= THUMBNAIL_FRAME_BUDGET {
                    return true;
                }
            }
            false
        };
        for (stream, packet) in ictx.packets() {
            if stream.index() != index {
                continue;
            }
            if dec.send_packet(&packet).is_err() {
                continue;
            }
            if take(&mut dec, &mut chosen) {
                done = true;
                break;
            }
        }
        if !done {
            let _ = dec.send_eof();
            take(&mut dec, &mut chosen);
        }
        let frame = chosen.ok_or_else(|| MediaError::Decode("no frames decoded".into()))?;
        let (w, h) = fit(frame.width(), frame.height(), max_size);
        Ok(to_rgba(&frame, w, h)?.rotated(rot))
    }

    fn make_proxy(
        &self,
        path: &Path,
        out: &Path,
        max_height: u32,
        cancel: &AtomicBool,
        progress: Progress<'_>,
    ) -> Result<(), MediaError> {
        let partial: PathBuf = {
            let mut p = out.as_os_str().to_owned();
            p.push(".part");
            p.into()
        };
        let result = write_proxy(path, &partial, max_height, cancel, progress);
        match result {
            Ok(()) => {
                std::fs::rename(&partial, out)?;
                Ok(())
            }
            Err(e) => {
                let _ = std::fs::remove_file(&partial);
                Err(e)
            }
        }
    }
}

fn write_proxy(
    path: &Path,
    out: &Path,
    max_height: u32,
    cancel: &AtomicBool,
    progress: Progress<'_>,
) -> Result<(), MediaError> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut ictx = open(path)?;
    let index = video_stream_index(&ictx).ok_or(MediaError::NoVideo)?;
    let (tb, rot, params, rate, duration) = {
        let s = ictx.stream(index).ok_or(MediaError::NoVideo)?;
        let d = stream_duration(&s);
        (
            s.time_base(),
            rotation(&s),
            s.parameters(),
            s.avg_frame_rate(),
            d,
        )
    };
    let duration = if duration > Time::ZERO {
        duration
    } else {
        Time::from_rational(ictx.duration().max(0), 1, AV_TIME_BASE)
    };
    let dec = ff::codec::context::Context::from_parameters(params)
        .and_then(|c| c.decoder().video())
        .map_err(decode_err)?;
    let (sw, sh) = (dec.width(), dec.height());
    let display_h = if rot % 180 == 90 { sw } else { sh };
    let scale = (f64::from(max_height) / f64::from(display_h)).min(1.0);
    let (tw, th) = (even(f64::from(sw) * scale), even(f64::from(sh) * scale));

    let mut octx = ff::format::output_as(out, "matroska").map_err(encode_err)?;
    let codec = ff::encoder::find(ff::codec::Id::MJPEG)
        .ok_or_else(|| MediaError::Encode("MJPEG encoder not available".into()))?;
    let global_header = octx
        .format()
        .flags()
        .contains(ff::format::Flags::GLOBAL_HEADER);
    let mut ost = octx.add_stream(codec).map_err(encode_err)?;
    let mut enc = ff::codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
        .map_err(encode_err)?;
    enc.set_width(tw);
    enc.set_height(th);
    enc.set_format(Pixel::YUVJ420P);
    enc.set_time_base(tb);
    if rate.numerator() > 0 {
        enc.set_frame_rate(Some(rate));
    }
    // Roughly one bit per pixel per frame at 30 fps: visually clean for
    // preview while staying small.
    enc.set_bit_rate(tw as usize * th as usize * 30);
    if global_header {
        enc.set_flags(ff::codec::Flags::GLOBAL_HEADER);
    }
    let enc = enc.open_as(codec).map_err(encode_err)?;
    ost.set_parameters(&enc);
    ost.set_time_base(tb);
    octx.write_header().map_err(encode_err)?;
    let ost_tb = octx.stream(0).map(|s| s.time_base()).unwrap_or(tb);

    let scaler = scaling::Context::get(
        dec.format(),
        sw,
        sh,
        Pixel::YUVJ420P,
        tw,
        th,
        scaling::Flags::BILINEAR,
    )
    .map_err(decode_err)?;

    let mut w = ProxyWriter {
        dec,
        enc,
        octx,
        scaler,
        tb,
        ost_tb,
        duration,
        progress,
        decoded: ff::frame::Video::empty(),
        scaled: ff::frame::Video::empty(),
        encoded: ff::Packet::empty(),
    };
    for (stream, packet) in ictx.packets() {
        if cancel.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        if stream.index() != index || w.dec.send_packet(&packet).is_err() {
            continue;
        }
        w.encode_decoded()?;
    }
    let _ = w.dec.send_eof();
    w.encode_decoded()?;
    w.enc.send_eof().map_err(encode_err)?;
    w.write_encoded()?;
    w.octx.write_trailer().map_err(encode_err)?;
    progress(1.0);
    Ok(())
}

/// Decoder → scaler → MJPEG encoder → Matroska muxer.
struct ProxyWriter<'a> {
    dec: ff::decoder::Video,
    enc: ff::encoder::Video,
    octx: ff::format::context::Output,
    scaler: scaling::Context,
    tb: ff::Rational,
    ost_tb: ff::Rational,
    duration: Time,
    progress: Progress<'a>,
    decoded: ff::frame::Video,
    scaled: ff::frame::Video,
    encoded: ff::Packet,
}

impl ProxyWriter<'_> {
    fn encode_decoded(&mut self) -> Result<(), MediaError> {
        while self.dec.receive_frame(&mut self.decoded).is_ok() {
            self.scaler
                .run(&self.decoded, &mut self.scaled)
                .map_err(decode_err)?;
            let pts = self.decoded.timestamp();
            self.scaled.set_pts(pts);
            self.enc.send_frame(&self.scaled).map_err(encode_err)?;
            self.write_encoded()?;
            if let Some(pts) = pts
                && self.duration > Time::ZERO
            {
                let t = Time::from_rational(pts, self.tb.numerator(), self.tb.denominator().max(1));
                (self.progress)(
                    (t.as_secs_f64() / self.duration.as_secs_f64()).clamp(0.0, 1.0) as f32,
                );
            }
        }
        Ok(())
    }

    fn write_encoded(&mut self) -> Result<(), MediaError> {
        while self.enc.receive_packet(&mut self.encoded).is_ok() {
            self.encoded.set_stream(0);
            self.encoded.rescale_ts(self.tb, self.ost_tb);
            self.encoded
                .write_interleaved(&mut self.octx)
                .map_err(encode_err)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_never_upscales() {
        assert_eq!(fit(100, 50, 320), (100, 50));
        assert_eq!(fit(1920, 1080, 320), (320, 180));
        assert_eq!(fit(1080, 1920, 320), (180, 320));
    }

    #[test]
    fn even_rounding() {
        assert_eq!(even(959.4), 960);
        assert_eq!(even(0.2), 2);
    }
}
