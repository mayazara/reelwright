//! Reading from capture devices (screens, cameras) through FFmpeg's input
//! devices: gdigrab/dshow on Windows, avfoundation on macOS, x11grab/v4l2
//! on Linux.

use crate::{MediaError, RgbaImage};
use ff::util::format::Pixel;
use ffmpeg_next as ff;
use std::ffi::{CStr, CString};

/// A picture as it came from a device, in the device's own format and
/// size. Writers convert it.
pub struct CapturedFrame {
    pub(crate) frame: ff::frame::Video,
}

impl std::fmt::Debug for CapturedFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapturedFrame")
            .field("width", &self.frame.width())
            .field("height", &self.frame.height())
            .finish()
    }
}

// SAFETY: a decoded frame is plain reference-counted data with no thread
// affinity; it is only ever used by one thread at a time.
unsafe impl Send for CapturedFrame {}

impl CapturedFrame {
    /// A frame from an RGBA picture (test patterns, stills).
    pub fn from_rgba(image: &RgbaImage) -> Self {
        let (w, h) = (image.width, image.height);
        let mut frame = ff::frame::Video::new(Pixel::RGBA, w, h);
        let stride = frame.stride(0);
        let row = w as usize * 4;
        let data = frame.data_mut(0);
        for y in 0..h as usize {
            data[y * stride..y * stride + row]
                .copy_from_slice(&image.pixels[y * row..(y + 1) * row]);
        }
        CapturedFrame { frame }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.frame.width(), self.frame.height())
    }
}

/// What to open: an FFmpeg input device, the device's address, options.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceSpec {
    /// Input device format, e.g. `gdigrab`, `dshow`, `avfoundation`, `x11grab`.
    pub format: String,
    /// Device address, e.g. `desktop`, `video=Camera`, `0`, `:0.0`.
    pub url: String,
    pub options: Vec<(String, String)>,
}

fn find_format(name: &str) -> Option<ff::format::format::Input> {
    crate::ffmpeg::Ffmpeg::new();
    ff::device::register_all();
    ff::device::input::video()
        .chain(ff::device::input::audio())
        .find_map(|f| match f {
            // Names can list aliases: "video4linux2,v4l2".
            ff::Format::Input(i) if i.name().split(',').any(|n| n == name) => Some(i),
            _ => None,
        })
}

/// Whether this FFmpeg build has the input device `format`.
pub fn has_device_format(format: &str) -> bool {
    find_format(format).is_some()
}

/// A device a capture format can open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Address to open it with.
    pub name: String,
    pub description: String,
    pub video: bool,
    pub audio: bool,
}

/// Devices `format` can open. Empty when the device type can't list (some
/// can only be opened by index).
pub fn list_devices(format: &str) -> Vec<DeviceInfo> {
    let Some(fmt) = find_format(format) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // SAFETY: `fmt` is a registered input format; the list is allocated by
    // FFmpeg, read while alive and freed with its own function.
    unsafe {
        let mut list: *mut ff::ffi::AVDeviceInfoList = std::ptr::null_mut();
        let r = ff::ffi::avdevice_list_input_sources(
            fmt.as_ptr() as *const _,
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut list,
        );
        if r >= 0 && !list.is_null() {
            for i in 0..(*list).nb_devices {
                let d = *(*list).devices.offset(i as isize);
                if d.is_null() {
                    continue;
                }
                let s = |p: *const std::os::raw::c_char| {
                    if p.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(p).to_string_lossy().into_owned()
                    }
                };
                let kinds: &[ff::ffi::AVMediaType] = if (*d).media_types.is_null() {
                    &[]
                } else {
                    std::slice::from_raw_parts((*d).media_types, (*d).nb_media_types as usize)
                };
                // Devices that don't say what they carry are taken as both.
                let video =
                    kinds.is_empty() || kinds.contains(&ff::ffi::AVMediaType::AVMEDIA_TYPE_VIDEO);
                let audio =
                    kinds.is_empty() || kinds.contains(&ff::ffi::AVMediaType::AVMEDIA_TYPE_AUDIO);
                out.push(DeviceInfo {
                    name: s((*d).device_name),
                    description: s((*d).device_description),
                    video,
                    audio,
                });
            }
            ff::ffi::avdevice_free_list_devices(&mut list);
        }
    }
    out
}

/// An open capture device, read one decoded frame at a time.
pub struct DeviceReader {
    ictx: ff::format::context::Input,
    decoder: ff::decoder::Video,
    index: usize,
    frame: ff::frame::Video,
}

// SAFETY: as for `VideoReader`: FFmpeg contexts have no thread affinity and
// `&mut self` guarantees one user at a time.
unsafe impl Send for DeviceReader {}

impl std::fmt::Debug for DeviceReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceReader")
            .field("width", &self.decoder.width())
            .field("height", &self.decoder.height())
            .finish()
    }
}

impl DeviceReader {
    /// Opens the device. Errors carry FFmpeg's message, which usually says
    /// whether the device is missing, busy or blocked by privacy settings.
    pub fn open(spec: &DeviceSpec) -> Result<Self, MediaError> {
        let fmt = find_format(&spec.format).ok_or_else(|| MediaError::Open {
            path: spec.url.clone(),
            reason: format!("this build can't capture with {}", spec.format),
        })?;
        let mut opts = ff::Dictionary::new();
        for (k, v) in &spec.options {
            opts.set(k, v);
        }
        // Device addresses aren't files; `open_with` takes them as paths.
        let url = CString::new(spec.url.clone()).map_err(|_| MediaError::Open {
            path: spec.url.clone(),
            reason: "invalid device name".into(),
        })?;
        let url = url.to_string_lossy().into_owned();
        let ctx = ff::format::open_with(&url, &ff::Format::Input(fmt), opts).map_err(|e| {
            MediaError::Open {
                path: spec.url.clone(),
                reason: e.to_string(),
            }
        })?;
        let ictx = match ctx {
            ff::format::context::Context::Input(i) => i,
            ff::format::context::Context::Output(_) => {
                return Err(MediaError::Open {
                    path: spec.url.clone(),
                    reason: "not an input".into(),
                });
            }
        };
        let stream = ictx
            .streams()
            .best(ff::media::Type::Video)
            .ok_or(MediaError::NoVideo)?;
        let index = stream.index();
        let decoder = ff::codec::context::Context::from_parameters(stream.parameters())
            .and_then(|c| c.decoder().video())
            .map_err(|e| MediaError::Decode(e.to_string()))?;
        Ok(DeviceReader {
            ictx,
            decoder,
            index,
            frame: ff::frame::Video::empty(),
        })
    }

    /// Size of the pictures the device delivers.
    pub fn size(&self) -> (u32, u32) {
        (self.decoder.width(), self.decoder.height())
    }

    /// Waits for the next picture.
    pub fn read(&mut self) -> Result<CapturedFrame, MediaError> {
        loop {
            if self.decoder.receive_frame(&mut self.frame).is_ok() {
                let out = std::mem::replace(&mut self.frame, ff::frame::Video::empty());
                return Ok(CapturedFrame { frame: out });
            }
            let mut packet = ff::Packet::empty();
            packet
                .read(&mut self.ictx)
                .map_err(|e| MediaError::Decode(format!("device stopped: {e}")))?;
            if packet.stream() == self.index {
                self.decoder
                    .send_packet(&packet)
                    .map_err(|e| MediaError::Decode(e.to_string()))?;
            }
        }
    }
}
