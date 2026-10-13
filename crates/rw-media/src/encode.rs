//! Writers for exported files: video (MP4 with H.264/HEVC/AV1 and AAC, or
//! WebM with VP9 and Opus) and sound only (MP3).

use crate::decode::{CHANNELS, SAMPLE_RATE};
use crate::ffmpeg::decode_err;
use crate::{MediaError, RgbaImage};
use ff::software::scaling;
use ff::util::format::Pixel;
use ffmpeg_next as ff;
use rw_model::FrameRate;
use std::path::{Path, PathBuf};

/// H.264 encoders to try, in order. Platform encoders come first, then GPU
/// vendors' encoders: their H.264 licensing is covered by the OS or the
/// hardware vendor. OpenH264 is the software fallback (e.g. Windows "N"
/// editions without Media Foundation); libx264 only exists in GPL developer
/// builds of FFmpeg. FFmpeg's built-in MPEG-4 Part 2 encoder is the last
/// resort.
pub const H264_ENCODERS: &[&str] = &[
    "h264_videotoolbox",
    "h264_mf",
    "h264_nvenc",
    "h264_qsv",
    "h264_amf",
    "libopenh264",
    "libx264",
];
const FALLBACK_ENCODER: &str = "mpeg4";

/// HEVC: hardware and OS encoders only (software HEVC encoders are GPL).
const HEVC_ENCODERS: &[&str] = &[
    "hevc_videotoolbox",
    "hevc_mf",
    "hevc_nvenc",
    "hevc_qsv",
    "hevc_amf",
];
const AV1_ENCODERS: &[&str] = &["av1_nvenc", "av1_qsv", "av1_amf", "libsvtav1", "libaom-av1"];
const VP9_ENCODERS: &[&str] = &["vp9_qsv", "libvpx-vp9"];

/// Video compression formats the app can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VideoCodec {
    #[default]
    H264,
    Hevc,
    Av1,
    Vp9,
}

impl VideoCodec {
    pub const ALL: [VideoCodec; 4] = [
        VideoCodec::H264,
        VideoCodec::Hevc,
        VideoCodec::Av1,
        VideoCodec::Vp9,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VideoCodec::H264 => "H.264 (plays everywhere)",
            VideoCodec::Hevc => "HEVC / H.265 (smaller files)",
            VideoCodec::Av1 => "AV1 (smallest files)",
            VideoCodec::Vp9 => "VP9 (WebM, for the web)",
        }
    }

    /// Encoders to try, best first.
    pub fn candidates(self) -> Vec<&'static str> {
        match self {
            VideoCodec::H264 => H264_ENCODERS
                .iter()
                .copied()
                .chain(std::iter::once(FALLBACK_ENCODER))
                .collect(),
            VideoCodec::Hevc => HEVC_ENCODERS.to_vec(),
            VideoCodec::Av1 => AV1_ENCODERS.to_vec(),
            VideoCodec::Vp9 => VP9_ENCODERS.to_vec(),
        }
    }

    /// FFmpeg container name.
    pub fn container(self) -> &'static str {
        match self {
            VideoCodec::Vp9 => "webm",
            _ => "mp4",
        }
    }

    /// File name extension (without the dot).
    pub fn extension(self) -> &'static str {
        match self {
            VideoCodec::Vp9 => "webm",
            _ => "mp4",
        }
    }

    fn audio_codec(self) -> ff::codec::Id {
        match self {
            VideoCodec::Vp9 => ff::codec::Id::OPUS,
            _ => ff::codec::Id::AAC,
        }
    }
}

/// Encoders of `codec` that start on this computer (a hardware encoder is
/// listed by FFmpeg even without the hardware, so each is tried). Cached.
pub fn working_encoders(codec: VideoCodec) -> Vec<String> {
    static FOUND: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<VideoCodec, Vec<String>>>,
    > = std::sync::OnceLock::new();
    let found = FOUND.get_or_init(Default::default);
    if let Some(v) = found.lock().unwrap_or_else(|e| e.into_inner()).get(&codec) {
        return v.clone();
    }
    crate::ffmpeg::Ffmpeg::new();
    let probe = VideoSettings {
        width: 320,
        height: 240,
        frame_rate: FrameRate::FPS_30,
        bit_rate: 1_000_000,
        encoder: None,
        codec,
    };
    let works: Vec<String> = codec
        .candidates()
        .into_iter()
        .filter(|n| ff::encoder::find_by_name(n).is_some())
        .filter(|n| open_video_encoder(n, &probe, true).is_ok())
        .map(str::to_owned)
        .collect();
    found
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(codec, works.clone());
    works
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoSettings {
    pub width: u32,
    pub height: u32,
    pub frame_rate: FrameRate,
    pub bit_rate: usize,
    /// Encoder name to use; `None` picks the first that works.
    pub encoder: Option<String>,
    pub codec: VideoCodec,
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
    /// Converts captured frames of one format and size to the encoder's.
    source_scaler: Option<((Pixel, u32, u32), scaling::Context)>,
    /// Frame number of the last frame written.
    last_index: i64,
}

struct AudioTrack {
    encoder: ff::encoder::Audio,
    index: usize,
    frame_size: usize,
    format: ff::format::Sample,
    pending: Vec<f32>,
    samples_written: i64,
}

impl AudioTrack {
    /// Adds a stereo 48 kHz stream encoded with `id` (or the encoder named
    /// `name`) to `octx`, in a sample format the encoder accepts.
    fn open(
        octx: &mut ff::format::context::Output,
        codec: ff::Codec,
        bit_rate: usize,
        global_header: bool,
    ) -> Result<Self, MediaError> {
        use ff::format::Sample;
        use ff::format::sample::Type;
        let supported: Vec<Sample> = codec
            .audio()
            .ok()
            .and_then(|a| a.formats().map(|f| f.collect()))
            .unwrap_or_default();
        let format = [
            Sample::F32(Type::Planar),
            Sample::F32(Type::Packed),
            Sample::I16(Type::Planar),
            Sample::I16(Type::Packed),
        ]
        .into_iter()
        .find(|f| supported.is_empty() || supported.contains(f))
        .ok_or_else(|| MediaError::Encode("no usable audio sample format".into()))?;
        let mut enc = ff::codec::context::Context::new_with_codec(codec)
            .encoder()
            .audio()
            .map_err(encode_err)?;
        enc.set_rate(SAMPLE_RATE as i32);
        enc.set_channel_layout(ff::ChannelLayout::STEREO);
        enc.set_format(format);
        enc.set_bit_rate(bit_rate);
        enc.set_time_base(ff::Rational::new(1, SAMPLE_RATE as i32));
        if global_header {
            enc.set_flags(ff::codec::Flags::GLOBAL_HEADER);
        }
        let enc = enc.open_as(codec).map_err(encode_err)?;
        let mut st = octx.add_stream(codec).map_err(encode_err)?;
        st.set_parameters(&enc);
        st.set_time_base(ff::Rational::new(1, SAMPLE_RATE as i32));
        let frame_size = match enc.frame_size() {
            0 => 1024,
            n => n as usize,
        };
        Ok(AudioTrack {
            index: st.index(),
            encoder: enc,
            frame_size,
            format,
            pending: Vec::new(),
            samples_written: 0,
        })
    }

    /// Encodes whole frames of pending samples (and the rest on `flush`).
    fn encode(
        &mut self,
        octx: &mut ff::format::context::Output,
        packet: &mut ff::Packet,
        flush: bool,
    ) -> Result<(), MediaError> {
        use ff::format::Sample;
        use ff::format::sample::Type;
        let fs = self.frame_size;
        while self.pending.len() >= fs * CHANNELS || (flush && !self.pending.is_empty()) {
            let n = (self.pending.len() / CHANNELS).min(fs);
            let mut frame = ff::frame::Audio::new(self.format, n, ff::ChannelLayout::STEREO);
            frame.set_rate(SAMPLE_RATE);
            let src = &self.pending[..n * CHANNELS];
            let to_i16 = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            match self.format {
                Sample::F32(Type::Planar) => {
                    for ch in 0..CHANNELS {
                        let plane = frame.plane_mut::<f32>(ch);
                        for (i, s) in plane.iter_mut().enumerate().take(n) {
                            *s = src[i * CHANNELS + ch];
                        }
                    }
                }
                Sample::I16(Type::Planar) => {
                    for ch in 0..CHANNELS {
                        let plane = frame.plane_mut::<i16>(ch);
                        for (i, s) in plane.iter_mut().enumerate().take(n) {
                            *s = to_i16(src[i * CHANNELS + ch]);
                        }
                    }
                }
                Sample::I16(Type::Packed) => {
                    let data = frame.data_mut(0);
                    for (i, v) in src.iter().enumerate() {
                        data[i * 2..i * 2 + 2].copy_from_slice(&to_i16(*v).to_ne_bytes());
                    }
                }
                _ => {
                    let data = frame.data_mut(0);
                    for (i, v) in src.iter().enumerate() {
                        data[i * 4..i * 4 + 4].copy_from_slice(&v.to_ne_bytes());
                    }
                }
            }
            self.pending.drain(..n * CHANNELS);
            frame.set_pts(Some(self.samples_written));
            self.samples_written += n as i64;
            self.encoder.send_frame(&frame).map_err(encode_err)?;
            self.drain(octx, packet)?;
        }
        if flush {
            self.encoder.send_eof().map_err(encode_err)?;
            self.drain(octx, packet)?;
        }
        Ok(())
    }

    fn drain(
        &mut self,
        octx: &mut ff::format::context::Output,
        packet: &mut ff::Packet,
    ) -> Result<(), MediaError> {
        let tb = ff::Rational::new(1, SAMPLE_RATE as i32);
        let ost_tb = octx.stream(self.index).map(|s| s.time_base()).unwrap_or(tb);
        while self.encoder.receive_packet(packet).is_ok() {
            packet.set_stream(self.index);
            packet.rescale_ts(tb, ost_tb);
            packet.write_interleaved(octx).map_err(encode_err)?;
        }
        Ok(())
    }
}

/// `path` with `.part` appended: files are written there and renamed once
/// complete, so a failed export never leaves a broken file behind.
fn partial_path(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(".part");
    PathBuf::from(p)
}

// SAFETY: FFmpeg's muxer, encoder and scaler contexts have no thread
// affinity; they only require one user at a time, which `&mut self` on
// every method guarantees. The raw pointers inside the wrappers are what
// make the type `!Send` by default. A recording creates the writer on one
// thread and writes on another.
unsafe impl Send for Mp4Writer {}

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
        Self::open(path, video, audio, false)
    }

    /// Creates `path` as fragmented MP4 written in place: if the app stops
    /// unexpectedly, everything up to the last fragment (about a second)
    /// still plays. For recordings.
    pub fn create_fragmented(
        path: &Path,
        video: &VideoSettings,
        audio: Option<&AudioSettings>,
    ) -> Result<Self, MediaError> {
        Self::open(path, video, audio, true)
    }

    fn open(
        path: &Path,
        video: &VideoSettings,
        audio: Option<&AudioSettings>,
        fragmented: bool,
    ) -> Result<Self, MediaError> {
        crate::ffmpeg::Ffmpeg::new();
        if !video.width.is_multiple_of(2) || !video.height.is_multiple_of(2) {
            return Err(MediaError::Encode("frame size must be even".into()));
        }
        let partial = if fragmented {
            path.to_path_buf()
        } else {
            partial_path(path)
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut octx =
            ff::format::output_as(&partial, video.codec.container()).map_err(encode_err)?;
        let global_header = octx
            .format()
            .flags()
            .contains(ff::format::Flags::GLOBAL_HEADER);

        let candidates: Vec<String> = match &video.encoder {
            Some(n) => vec![n.clone()],
            None => video
                .codec
                .candidates()
                .into_iter()
                .map(str::to_owned)
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
                let id = video.codec.audio_codec();
                let codec = ff::encoder::find(id)
                    .ok_or_else(|| MediaError::Encode(format!("{id:?} encoder not available")))?;
                Some(AudioTrack::open(
                    &mut octx,
                    codec,
                    a.bit_rate,
                    global_header,
                )?)
            }
        };

        if fragmented {
            let mut opts = ff::Dictionary::new();
            opts.set("movflags", "frag_keyframe+empty_moov+default_base_moof");
            opts.set("frag_duration", "1000000");
            octx.write_header_with(opts).map_err(encode_err)?;
        } else {
            octx.write_header().map_err(encode_err)?;
        }
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
            source_scaler: None,
            last_index: -1,
        };
        w.set_output_colorspace();
        Ok(w)
    }

    /// Appends a captured frame (any pixel format and size; scaled to the
    /// output) as frame number `index`. Indices must increase; a frame for
    /// an index already written is skipped, and gaps just hold the previous
    /// picture longer. Returns whether it was written.
    pub fn write_frame(
        &mut self,
        captured: &crate::CapturedFrame,
        index: i64,
    ) -> Result<bool, MediaError> {
        let frame = &captured.frame;
        if index <= self.last_index {
            return Ok(false);
        }
        let key = (frame.format(), frame.width(), frame.height());
        if self.source_scaler.as_ref().is_none_or(|(k, _)| *k != key) {
            let ctx = scaling::Context::get(
                key.0,
                key.1,
                key.2,
                self.yuv.format(),
                self.yuv.width(),
                self.yuv.height(),
                scaling::Flags::BILINEAR,
            )
            .map_err(decode_err)?;
            self.source_scaler = Some((key, ctx));
        }
        let (_, scaler) = self.source_scaler.as_mut().expect("set above");
        scaler.run(frame, &mut self.yuv).map_err(decode_err)?;
        self.yuv.set_pts(Some(index));
        self.last_index = index;
        self.frames += 1;
        self.video.send_frame(&self.yuv).map_err(encode_err)?;
        self.drain_video()?;
        Ok(true)
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
        let index = self.last_index + 1;
        self.yuv.set_pts(Some(index));
        self.last_index = index;
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
        match self.audio.as_mut() {
            Some(a) => a.encode(&mut self.octx, &mut self.packet, flush),
            None => Ok(()),
        }
    }

    /// Flushes encoders, finalises the file and moves it into place.
    pub fn finish(mut self) -> Result<PathBuf, MediaError> {
        self.video.send_eof().map_err(encode_err)?;
        self.drain_video()?;
        self.encode_audio(true)?;
        self.octx.write_trailer().map_err(encode_err)?;
        let Mp4Writer {
            octx,
            path,
            partial,
            video,
            audio,
            ..
        } = self;
        // Close the file before renaming: Windows refuses to rename an open
        // file. (Fields left in `..` would only drop at the end of the
        // function, so the muxer is dropped explicitly.)
        drop(audio);
        drop(video);
        drop(octx);
        if partial != path {
            std::fs::rename(&partial, &path)?;
        }
        Ok(path)
    }

    /// Stops writing and deletes the partial file (or, for a fragmented
    /// file, the file itself).
    pub fn abort(self) {
        let partial = self.partial.clone();
        drop(self);
        let _ = std::fs::remove_file(partial);
    }
}

/// Writes sound only (MP3) one chunk at a time.
pub struct AudioWriter {
    octx: ff::format::context::Output,
    path: PathBuf,
    partial: PathBuf,
    track: AudioTrack,
    packet: ff::Packet,
}

// SAFETY: as for `Mp4Writer`.
unsafe impl Send for AudioWriter {}

impl std::fmt::Debug for AudioWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioWriter")
            .field("path", &self.path)
            .finish()
    }
}

impl AudioWriter {
    /// Creates an MP3 file at `path` (written to a temporary name and
    /// renamed by [`Self::finish`]).
    pub fn create_mp3(path: &Path, bit_rate: usize) -> Result<Self, MediaError> {
        Self::open(path, "mp3", "libmp3lame", bit_rate, false)
    }

    /// Creates an AAC sound file (`.m4a`) at `path`, fragmented and written
    /// in place so a crash keeps what was recorded. For recordings.
    pub fn create_m4a_fragmented(path: &Path, bit_rate: usize) -> Result<Self, MediaError> {
        Self::open(path, "ipod", "aac", bit_rate, true)
    }

    fn open(
        path: &Path,
        container: &str,
        encoder: &str,
        bit_rate: usize,
        fragmented: bool,
    ) -> Result<Self, MediaError> {
        crate::ffmpeg::Ffmpeg::new();
        let partial = if fragmented {
            path.to_path_buf()
        } else {
            partial_path(path)
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut octx = ff::format::output_as(&partial, container).map_err(encode_err)?;
        let global_header = octx
            .format()
            .flags()
            .contains(ff::format::Flags::GLOBAL_HEADER);
        let codec = ff::encoder::find_by_name(encoder)
            .ok_or_else(|| MediaError::Encode(format!("{encoder} encoder not available")))?;
        let track = AudioTrack::open(&mut octx, codec, bit_rate, global_header)?;
        if fragmented {
            let mut opts = ff::Dictionary::new();
            opts.set("movflags", "frag_keyframe+empty_moov+default_base_moof");
            opts.set("frag_duration", "1000000");
            octx.write_header_with(opts).map_err(encode_err)?;
        } else {
            octx.write_header().map_err(encode_err)?;
        }
        Ok(AudioWriter {
            octx,
            path: path.to_path_buf(),
            partial,
            track,
            packet: ff::Packet::empty(),
        })
    }

    /// Appends interleaved stereo samples at 48 kHz.
    pub fn write(&mut self, samples: &[f32]) -> Result<(), MediaError> {
        self.track.pending.extend_from_slice(samples);
        self.track.encode(&mut self.octx, &mut self.packet, false)
    }

    pub fn finish(mut self) -> Result<PathBuf, MediaError> {
        self.track.encode(&mut self.octx, &mut self.packet, true)?;
        self.octx.write_trailer().map_err(encode_err)?;
        let AudioWriter {
            octx,
            path,
            partial,
            track,
            ..
        } = self;
        drop(track);
        drop(octx);
        if partial != path {
            std::fs::rename(&partial, &path)?;
        }
        Ok(path)
    }

    /// Stops writing and deletes the partial file.
    pub fn abort(self) {
        let partial = self.partial.clone();
        drop(self);
        let _ = std::fs::remove_file(partial);
    }
}
