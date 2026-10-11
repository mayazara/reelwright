//! MP4 writer: H.264 video and AAC audio.

use crate::decode::{CHANNELS, SAMPLE_RATE};
use crate::ffmpeg::decode_err;
use crate::{MediaError, RgbaImage};
use ff::software::scaling;
use ff::util::format::Pixel;
use ffmpeg_next as ff;
use rw_model::FrameRate;
use std::path::{Path, PathBuf};

/// H.264 encoders to try, in order. OpenH264 and OS/hardware encoders ship
/// with LGPL FFmpeg builds; libx264 only exists in GPL developer builds. The
/// MPEG-4 Part 2 encoder is built into FFmpeg and always available.
pub const H264_ENCODERS: &[&str] = &[
    "libopenh264",
    "h264_videotoolbox",
    "h264_nvenc",
    "h264_qsv",
    "h264_amf",
    "h264_mf",
    "libx264",
];
const FALLBACK_ENCODER: &str = "mpeg4";

#[derive(Debug, Clone, PartialEq)]
pub struct VideoSettings {
    pub width: u32,
    pub height: u32,
    pub frame_rate: FrameRate,
    pub bit_rate: usize,
    /// Encoder name to use; `None` picks the first that works.
    pub encoder: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioSettings {
    pub bit_rate: usize,
}

/// Encoders from `names` that this FFmpeg build has.
pub fn available_encoders(names: &[&str]) -> Vec<String> {
    crate::ffmpeg::Ffmpeg::new();
    names
        .iter()
        .filter(|n| ff::encoder::find_by_name(n).is_some())
        .map(|n| (*n).to_owned())
        .collect()
}

/// Writes an MP4 file one frame and one audio chunk at a time.
pub struct Mp4Writer {
    octx: ff::format::context::Output,
    path: PathBuf,
    partial: PathBuf,
    video: ff::encoder::Video,
    video_tb: ff::Rational,
    video_index: usize,
    scaler: scaling::Context,
    yuv: ff::frame::Video,
    frames: i64,
    audio: Option<AudioTrack>,
    packet: ff::Packet,
    encoder_name: String,
}

struct AudioTrack {
    encoder: ff::encoder::Audio,
    index: usize,
    frame_size: usize,
    pending: Vec<f32>,
    samples_written: i64,
}

impl std::fmt::Debug for Mp4Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mp4Writer")
            .field("path", &self.path)
            .field("encoder", &self.encoder_name)
            .field("frames", &self.frames)
            .finish()
    }
}

fn encode_err(e: ff::Error) -> MediaError {
    MediaError::Encode(e.to_string())
}

fn open_video_encoder(
    name: &str,
    s: &VideoSettings,
    global_header: bool,
) -> Result<(ff::encoder::Video, ff::Codec, Pixel), MediaError> {
    let codec = ff::encoder::find_by_name(name)
        .ok_or_else(|| MediaError::Encode(format!("encoder {name} not available")))?;
    let formats: Vec<Pixel> = codec
        .video()
        .ok()
        .and_then(|v| v.formats().map(|f| f.collect()))
        .unwrap_or_default();
    let format = [Pixel::YUV420P, Pixel::NV12]
        .into_iter()
        .find(|p| formats.is_empty() || formats.contains(p))
        .or_else(|| formats.first().copied())
        .unwrap_or(Pixel::YUV420P);
    let mut enc = ff::codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
        .map_err(encode_err)?;
    enc.set_width(s.width);
    enc.set_height(s.height);
    enc.set_format(format);
    let tb = ff::Rational::new(s.frame_rate.den as i32, s.frame_rate.num as i32);
    enc.set_time_base(tb);
    enc.set_frame_rate(Some(ff::Rational::new(
        s.frame_rate.num as i32,
        s.frame_rate.den as i32,
    )));
    enc.set_bit_rate(s.bit_rate);
    enc.set_max_bit_rate(s.bit_rate * 2);
    enc.set_gop((s.frame_rate.as_f64() * 2.0).round().max(1.0) as u32);
    if global_header {
        enc.set_flags(ff::codec::Flags::GLOBAL_HEADER);
    }
    let mut opts = ff::Dictionary::new();
    if name == "libx264" {
        opts.set("preset", "veryfast");
    }
    let enc = enc.open_as_with(codec, opts).map_err(encode_err)?;
    Ok((enc, codec, format))
}

impl Mp4Writer {
    /// Creates `path` (written to a temporary name and renamed by
    /// [`Self::finish`]).
    pub fn create(
        path: &Path,
        video: &VideoSettings,
        audio: Option<&AudioSettings>,
    ) -> Result<Self, MediaError> {
        crate::ffmpeg::Ffmpeg::new();
        if !video.width.is_multiple_of(2) || !video.height.is_multiple_of(2) {
            return Err(MediaError::Encode("frame size must be even".into()));
        }
        let partial = {
            let mut p = path.as_os_str().to_owned();
            p.push(".part");
            PathBuf::from(p)
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut octx = ff::format::output_as(&partial, "mp4").map_err(encode_err)?;
        let global_header = octx
            .format()
            .flags()
            .contains(ff::format::Flags::GLOBAL_HEADER);

        let candidates: Vec<String> = match &video.encoder {
            Some(n) => vec![n.clone()],
            None => H264_ENCODERS
                .iter()
                .chain(std::iter::once(&FALLBACK_ENCODER))
                .map(|s| (*s).to_owned())
                .collect(),
        };
        let mut last_err = None;
        let mut opened = None;
        for name in &candidates {
            match open_video_encoder(name, video, global_header) {
                Ok(v) => {
                    opened = Some((name.clone(), v));
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }
        let (encoder_name, (venc, vcodec, format)) = opened.ok_or_else(|| {
            last_err.unwrap_or_else(|| MediaError::Encode("no video encoder".into()))
        })?;
        let video_tb = venc.time_base();
        let video_index = {
            let mut st = octx.add_stream(vcodec).map_err(encode_err)?;
            st.set_parameters(&venc);
            st.set_time_base(video_tb);
            let rate = ff::Rational::new(video.frame_rate.num as i32, video.frame_rate.den as i32);
            st.set_avg_frame_rate(rate);
            st.set_rate(rate);
            st.index()
        };

        let audio = match audio {
            None => None,
            Some(a) => {
                let codec = ff::encoder::find(ff::codec::Id::AAC)
                    .ok_or_else(|| MediaError::Encode("AAC encoder not available".into()))?;
                let mut enc = ff::codec::context::Context::new_with_codec(codec)
                    .encoder()
                    .audio()
                    .map_err(encode_err)?;
                enc.set_rate(SAMPLE_RATE as i32);
                enc.set_channel_layout(ff::ChannelLayout::STEREO);
                enc.set_format(ff::format::Sample::F32(ff::format::sample::Type::Planar));
                enc.set_bit_rate(a.bit_rate);
                enc.set_time_base(ff::Rational::new(1, SAMPLE_RATE as i32));
                if global_header {
                    enc.set_flags(ff::codec::Flags::GLOBAL_HEADER);
                }
                let enc = enc.open_as(codec).map_err(encode_err)?;
                let mut st = octx.add_stream(codec).map_err(encode_err)?;
                st.set_parameters(&enc);
                st.set_time_base(ff::Rational::new(1, SAMPLE_RATE as i32));
                let frame_size = (enc.frame_size() as usize).max(1);
                Some(AudioTrack {
                    index: st.index(),
                    encoder: enc,
                    frame_size,
                    pending: Vec::new(),
                    samples_written: 0,
                })
            }
        };

        octx.write_header().map_err(encode_err)?;
        let scaler = scaling::Context::get(
            Pixel::RGBA,
            video.width,
            video.height,
            format,
            video.width,
            video.height,
            scaling::Flags::BILINEAR,
        )
        .map_err(decode_err)?;
        let mut yuv = ff::frame::Video::new(format, video.width, video.height);
        yuv.set_color_space(ff::color::Space::BT709);
        yuv.set_color_primaries(ff::color::Primaries::BT709);
        yuv.set_color_transfer_characteristic(ff::color::TransferCharacteristic::BT709);
        yuv.set_color_range(ff::color::Range::MPEG);
        let mut w = Mp4Writer {
            octx,
            path: path.to_path_buf(),
            partial,
            video: venc,
            video_tb,
            video_index,
            scaler,
            yuv,
            frames: 0,
            audio,
            packet: ff::Packet::empty(),
            encoder_name,
        };
        w.set_output_colorspace();
        Ok(w)
    }

    /// Converts with the BT.709 matrix into limited-range YUV.
    // Binding constant types vary by platform, so the cast stays.
    #[allow(clippy::unnecessary_cast)]
    fn set_output_colorspace(&mut self) {
        // SAFETY: the scaler is live; coefficient tables are static.
        unsafe {
            let table = ff::ffi::sws_getCoefficients(ff::ffi::SWS_CS_ITU709 as i32);
            ff::ffi::sws_setColorspaceDetails(
                self.scaler.as_mut_ptr(),
                table,
                1,
                table,
                0,
                0,
                1 << 16,
                1 << 16,
            );
        }
    }

    pub fn encoder_name(&self) -> &str {
        &self.encoder_name
    }

    /// Frames written so far.
    pub fn frames(&self) -> i64 {
        self.frames
    }

    /// Appends the next video frame (RGBA, exactly the output size).
    pub fn write_video(&mut self, image: &RgbaImage) -> Result<(), MediaError> {
        let (w, h) = (self.video.width(), self.video.height());
        if (image.width, image.height) != (w, h) {
            return Err(MediaError::Encode(format!(
                "frame is {}x{}, expected {w}x{h}",
                image.width, image.height
            )));
        }
        let mut rgba = ff::frame::Video::new(Pixel::RGBA, w, h);
        let stride = rgba.stride(0);
        let row = w as usize * 4;
        let data = rgba.data_mut(0);
        for y in 0..h as usize {
            data[y * stride..y * stride + row]
                .copy_from_slice(&image.pixels[y * row..(y + 1) * row]);
        }
        self.scaler.run(&rgba, &mut self.yuv).map_err(decode_err)?;
        self.yuv.set_pts(Some(self.frames));
        self.frames += 1;
        self.video.send_frame(&self.yuv).map_err(encode_err)?;
        self.drain_video()
    }

    fn drain_video(&mut self) -> Result<(), MediaError> {
        let ost_tb = self
            .octx
            .stream(self.video_index)
            .map(|s| s.time_base())
            .unwrap_or(self.video_tb);
        while self.video.receive_packet(&mut self.packet).is_ok() {
            self.packet.set_stream(self.video_index);
            // Encoders don't always fill in durations; without them the
            // muxer gives the last frame zero length.
            if self.packet.duration() == 0 {
                self.packet.set_duration(1);
            }
            self.packet.rescale_ts(self.video_tb, ost_tb);
            self.packet
                .write_interleaved(&mut self.octx)
                .map_err(encode_err)?;
        }
        Ok(())
    }

    /// Appends interleaved stereo samples at 48 kHz.
    pub fn write_audio(&mut self, samples: &[f32]) -> Result<(), MediaError> {
        let Some(a) = self.audio.as_mut() else {
            return Ok(());
        };
        a.pending.extend_from_slice(samples);
        self.encode_audio(false)
    }

    fn encode_audio(&mut self, flush: bool) -> Result<(), MediaError> {
        let Some(a) = self.audio.as_mut() else {
            return Ok(());
        };
        let fs = a.frame_size;
        while a.pending.len() >= fs * CHANNELS || (flush && !a.pending.is_empty()) {
            let n = (a.pending.len() / CHANNELS).min(fs);
            let mut frame = ff::frame::Audio::new(
                ff::format::Sample::F32(ff::format::sample::Type::Planar),
                n,
                ff::ChannelLayout::STEREO,
            );
            frame.set_rate(SAMPLE_RATE);
            for ch in 0..CHANNELS {
                let plane = frame.plane_mut::<f32>(ch);
                for (i, s) in plane.iter_mut().enumerate().take(n) {
                    *s = a.pending[i * CHANNELS + ch];
                }
            }
            a.pending.drain(..n * CHANNELS);
            frame.set_pts(Some(a.samples_written));
            a.samples_written += n as i64;
            a.encoder.send_frame(&frame).map_err(encode_err)?;
            Self::drain_audio(a, &mut self.octx, &mut self.packet)?;
        }
        if flush {
            a.encoder.send_eof().map_err(encode_err)?;
            Self::drain_audio(a, &mut self.octx, &mut self.packet)?;
        }
        Ok(())
    }

    fn drain_audio(
        a: &mut AudioTrack,
        octx: &mut ff::format::context::Output,
        packet: &mut ff::Packet,
    ) -> Result<(), MediaError> {
        let tb = ff::Rational::new(1, SAMPLE_RATE as i32);
        let ost_tb = octx.stream(a.index).map(|s| s.time_base()).unwrap_or(tb);
        while a.encoder.receive_packet(packet).is_ok() {
            packet.set_stream(a.index);
            packet.rescale_ts(tb, ost_tb);
            packet.write_interleaved(octx).map_err(encode_err)?;
        }
        Ok(())
    }

    /// Flushes encoders, finalises the file and moves it into place.
    pub fn finish(mut self) -> Result<PathBuf, MediaError> {
        self.video.send_eof().map_err(encode_err)?;
        self.drain_video()?;
        self.encode_audio(true)?;
        self.octx.write_trailer().map_err(encode_err)?;
        let Mp4Writer { path, partial, .. } = self;
        // Everything else (including the muxer's file handle) is closed by
        // now: Windows refuses to rename a file that is still open.
        std::fs::rename(&partial, &path)?;
        Ok(path)
    }

    /// Stops writing and deletes the partial file.
    pub fn abort(self) {
        let partial = self.partial.clone();
        drop(self);
        let _ = std::fs::remove_file(partial);
    }
}
