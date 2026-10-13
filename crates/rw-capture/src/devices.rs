//! The screens and cameras this computer can record, per operating system.

use rw_media::DeviceSpec;
use rw_media::capture::{has_device_format, list_devices};

/// A screen or camera to record.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoDevice {
    pub label: String,
    pub spec: DeviceSpec,
}

fn spec(format: &str, url: impl Into<String>, options: &[(&str, String)]) -> DeviceSpec {
    DeviceSpec {
        format: format.into(),
        url: url.into(),
        options: options
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
    }
}

/// Screens that can be recorded. `cursor` draws the mouse pointer.
pub fn screens(fps: u32, cursor: bool) -> Vec<VideoDevice> {
    let fps_s = fps.to_string();
    let mouse = if cursor { "1" } else { "0" }.to_owned();
    if cfg!(target_os = "windows") && has_device_format("gdigrab") {
        vec![VideoDevice {
            label: "All screens".into(),
            spec: spec(
                "gdigrab",
                "desktop",
                &[("framerate", fps_s), ("draw_mouse", mouse)],
            ),
        }]
    } else if cfg!(target_os = "macos") && has_device_format("avfoundation") {
        // Screens are named "Capture screen N"; offer the first two.
        (0..2)
            .map(|i| VideoDevice {
                label: format!("Screen {}", i + 1),
                spec: spec(
                    "avfoundation",
                    format!("Capture screen {i}:none"),
                    &[
                        ("framerate", fps_s.clone()),
                        ("capture_cursor", mouse.clone()),
                        ("pixel_format", "uyvy422".into()),
                    ],
                ),
            })
            .collect()
    } else if has_device_format("x11grab") {
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
        vec![VideoDevice {
            label: "Screen".into(),
            spec: spec(
                "x11grab",
                format!("{display}.0"),
                &[("framerate", fps_s), ("draw_mouse", mouse)],
            ),
        }]
    } else {
        Vec::new()
    }
}

/// Cameras that can be recorded.
pub fn cameras(fps: u32) -> Vec<VideoDevice> {
    let fps_s = fps.to_string();
    if cfg!(target_os = "windows") && has_device_format("dshow") {
        list_devices("dshow")
            .into_iter()
            .filter(|d| d.video)
            .map(|d| VideoDevice {
                label: d.description.clone(),
                spec: spec(
                    "dshow",
                    format!("video={}", d.name),
                    &[("rtbufsize", "256M".into())],
                ),
            })
            .collect()
    } else if cfg!(target_os = "macos") && has_device_format("avfoundation") {
        // AVFoundation can't list here; cameras open by number.
        vec![VideoDevice {
            label: "Built-in or first camera".into(),
            spec: spec(
                "avfoundation",
                "0:none",
                &[("framerate", fps_s), ("video_size", "1280x720".into())],
            ),
        }]
    } else if has_device_format("video4linux2") {
        let mut found: Vec<VideoDevice> = std::fs::read_dir("/dev")
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("video"))
            })
            .map(|p| VideoDevice {
                label: p.display().to_string(),
                spec: spec(
                    "video4linux2",
                    p.display().to_string(),
                    &[("framerate", fps_s.clone())],
                ),
            })
            .collect();
        found.sort_by(|a, b| a.label.cmp(&b.label));
        found
    } else {
        Vec::new()
    }
}

/// A friendlier message for a device that wouldn't open, with what to do.
pub fn explain_open_error(what: &str, error: &str) -> String {
    let hint = if cfg!(target_os = "macos") {
        "Allow the app in System Settings → Privacy & Security (Screen Recording, Camera or Microphone), then try again."
    } else if cfg!(target_os = "windows") {
        "Check that no other app is using it and that camera and microphone access is on in Settings → Privacy & security."
    } else {
        "Check that it's connected and that you have permission to use it."
    };
    format!("Couldn't start the {what}: {error}. {hint}")
}
