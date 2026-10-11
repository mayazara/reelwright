//! Test fixtures: short synthetic media files generated with the `ffmpeg`
//! command-line tool, so no third-party footage is committed.
//!
//! Files are written once to `target/fixtures/` (or `$RW_FIXTURES_DIR`) and
//! reused. A fixture whose encoder is missing from the local `ffmpeg` build is
//! reported as skipped rather than failing generation.

use rw_model::MediaKind;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// What probing a fixture must report.
#[derive(Debug, Clone, Copy)]
pub struct Expect {
    pub kind: MediaKind,
    /// Coded video size, if any.
    pub video: Option<(u32, u32)>,
    pub alpha: bool,
    pub rotation: u16,
    pub audio: bool,
}

#[derive(Debug)]
pub struct Fixture {
    pub name: &'static str,
    /// Alternative encoders; `{enc}` in `args` is replaced by the first one
    /// the local `ffmpeg` provides. Empty means no substitution.
    pub encoders: &'static [&'static str],
    /// Arguments between `ffmpeg -y` and the output path. `{fixtures}` is
    /// replaced by the fixtures directory.
    pub args: &'static [&'static str],
    /// Optional second pass reading the first pass's output (`{prev}`).
    pub then: &'static [&'static str],
    pub expect: Expect,
}

const fn video(kind: MediaKind, w: u32, h: u32, audio: bool) -> Expect {
    Expect {
        kind,
        video: Some((w, h)),
        alpha: false,
        rotation: 0,
        audio,
    }
}

const AUDIO: Expect = Expect {
    kind: MediaKind::Audio,
    video: None,
    alpha: false,
    rotation: 0,
    audio: true,
};

const H264: &[&str] = &["libx264", "libopenh264", "h264_mf", "h264_videotoolbox"];
const HEVC: &[&str] = &["libx265", "libkvazaar", "hevc_mf", "hevc_videotoolbox"];

/// The media matrix used by import tests. Covers the codecs, containers and
/// layouts the M1 acceptance criteria name.
pub const ALL: &[Fixture] = &[
    Fixture {
        name: "h264_1080p30_aac.mp4",
        encoders: H264,
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=1920x1080:r=30:d=2",
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:d=2",
            "-c:v",
            "{enc}",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ],
        then: &[],
        expect: video(MediaKind::Video, 1920, 1080, true),
    },
    Fixture {
        name: "h264_720p60.mp4",
        encoders: H264,
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=1280x720:r=60:d=2",
            "-c:v",
            "{enc}",
            "-pix_fmt",
            "yuv420p",
            "-an",
        ],
        then: &[],
        expect: video(MediaKind::Video, 1280, 720, false),
    },
    Fixture {
        name: "h264_2997_rotated.mp4",
        encoders: H264,
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=1280x720:r=30000/1001:d=2",
            "-c:v",
            "{enc}",
            "-pix_fmt",
            "yuv420p",
            "-an",
        ],
        then: &["-display_rotation:v:0", "90", "-i", "{prev}", "-c", "copy"],
        expect: Expect {
            rotation: 270,
            ..video(MediaKind::Video, 1280, 720, false)
        },
    },
    Fixture {
        name: "h264_4k30.mp4",
        encoders: H264,
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=3840x2160:r=30:d=1",
            "-c:v",
            "{enc}",
            "-pix_fmt",
            "yuv420p",
            "-an",
        ],
        then: &[],
        expect: video(MediaKind::Video, 3840, 2160, false),
    },
    Fixture {
        name: "hevc_720p_aac.mp4",
        encoders: HEVC,
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=1280x720:r=25:d=2",
            "-f",
            "lavfi",
            "-i",
            "sine=f=330:d=2",
            "-c:v",
            "{enc}",
            "-pix_fmt",
            "yuv420p",
            "-tag:v",
            "hvc1",
            "-c:a",
            "aac",
            "-shortest",
        ],
        then: &[],
        expect: video(MediaKind::Video, 1280, 720, true),
    },
    Fixture {
        name: "prores_422.mov",
        encoders: &[],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=1280x720:r=24:d=2",
            "-f",
            "lavfi",
            "-i",
            "sine=f=550:d=2",
            "-c:v",
            "prores_ks",
            "-profile:v",
            "2",
            "-c:a",
            "pcm_s16le",
            "-shortest",
        ],
        then: &[],
        expect: video(MediaKind::Video, 1280, 720, true),
    },
    Fixture {
        name: "prores_4444_alpha.mov",
        encoders: &[],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=640x360:r=25:d=2,format=yuva444p10le",
            "-c:v",
            "prores_ks",
            "-profile:v",
            "4444",
            "-pix_fmt",
            "yuva444p10le",
        ],
        then: &[],
        expect: Expect {
            alpha: true,
            ..video(MediaKind::Video, 640, 360, false)
        },
    },
    Fixture {
        name: "qtrle_alpha.mov",
        encoders: &[],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=320x240:r=15:d=2,format=argb",
            "-c:v",
            "qtrle",
        ],
        then: &[],
        expect: Expect {
            alpha: true,
            ..video(MediaKind::Video, 320, 240, false)
        },
    },
    Fixture {
        name: "vp9_opus.webm",
        encoders: &["libvpx-vp9"],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=854x480:r=30:d=2",
            "-f",
            "lavfi",
            "-i",
            "sine=f=660:d=2",
            "-c:v",
            "{enc}",
            "-deadline",
            "realtime",
            "-cpu-used",
            "8",
            "-b:v",
            "1M",
            "-c:a",
            "libopus",
            "-shortest",
        ],
        then: &[],
        expect: video(MediaKind::Video, 854, 480, true),
    },
    Fixture {
        name: "mjpeg_pcm.avi",
        encoders: &[],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=640x480:r=25:d=2",
            "-f",
            "lavfi",
            "-i",
            "sine=f=220:d=2",
            "-c:v",
            "mjpeg",
            "-q:v",
            "5",
            "-pix_fmt",
            "yuvj420p",
            "-c:a",
            "pcm_s16le",
            "-shortest",
        ],
        then: &[],
        expect: video(MediaKind::Video, 640, 480, true),
    },
    Fixture {
        name: "mpeg2_mp2.mpg",
        encoders: &[],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=720x576:r=25:d=2",
            "-f",
            "lavfi",
            "-i",
            "sine=f=880:d=2",
            "-c:v",
            "mpeg2video",
            "-c:a",
            "mp2",
            "-shortest",
        ],
        then: &[],
        expect: video(MediaKind::Video, 720, 576, true),
    },
    Fixture {
        name: "still.png",
        encoders: &[],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=1600x900:r=1:d=1",
            "-frames:v",
            "1",
            "-update",
            "1",
        ],
        then: &[],
        expect: video(MediaKind::Image, 1600, 900, false),
    },
    Fixture {
        name: "still_alpha.png",
        encoders: &[],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "color=c=0x2BB3A6@0.5:s=400x300:d=1,format=rgba",
            "-frames:v",
            "1",
            "-update",
            "1",
        ],
        then: &[],
        expect: Expect {
            alpha: true,
            ..video(MediaKind::Image, 400, 300, false)
        },
    },
    Fixture {
        name: "photo.jpg",
        encoders: &[],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=1200x1600:r=1:d=1",
            "-frames:v",
            "1",
            "-q:v",
            "3",
            "-update",
            "1",
        ],
        then: &[],
        expect: video(MediaKind::Image, 1200, 1600, false),
    },
    Fixture {
        name: "animated.gif",
        encoders: &[],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=320x240:r=10:d=2",
            "-pix_fmt",
            "rgb8",
        ],
        then: &[],
        // GIF decodes to BGRA because any palette entry may be transparent.
        expect: Expect {
            alpha: true,
            ..video(MediaKind::Animation, 320, 240, false)
        },
    },
    Fixture {
        name: "single_frame.gif",
        encoders: &[],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=200x200:r=1:d=1",
            "-frames:v",
            "1",
            "-pix_fmt",
            "rgb8",
        ],
        then: &[],
        // GIF decodes to BGRA because any palette entry may be transparent.
        expect: Expect {
            alpha: true,
            ..video(MediaKind::Image, 200, 200, false)
        },
    },
    Fixture {
        name: "tone.mp3",
        encoders: &["libmp3lame"],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:d=3",
            "-c:a",
            "{enc}",
            "-b:a",
            "128k",
        ],
        then: &[],
        expect: AUDIO,
    },
    Fixture {
        name: "tone_with_cover.mp3",
        encoders: &["libmp3lame"],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "sine=f=523:d=3",
            "-i",
            "{fixtures}/still.png",
            "-map",
            "0:a",
            "-map",
            "1:v",
            "-c:a",
            "{enc}",
            "-c:v",
            "png",
            "-disposition:v",
            "attached_pic",
        ],
        then: &[],
        expect: AUDIO,
    },
    Fixture {
        name: "tone.wav",
        encoders: &[],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:d=3:sample_rate=48000",
            "-c:a",
            "pcm_s16le",
        ],
        then: &[],
        expect: AUDIO,
    },
    Fixture {
        name: "tone_aac.m4a",
        encoders: &[],
        args: &["-f", "lavfi", "-i", "sine=f=440:d=3", "-c:a", "aac"],
        then: &[],
        expect: AUDIO,
    },
    Fixture {
        name: "tone.flac",
        encoders: &[],
        args: &[
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:d=3:sample_rate=44100",
            "-ac",
            "2",
            "-c:a",
            "flac",
        ],
        then: &[],
        expect: AUDIO,
    },
];

/// Result of making sure fixtures exist.
#[derive(Debug, Default)]
pub struct Generated {
    pub dir: PathBuf,
    /// Fixtures present on disk.
    pub available: Vec<&'static Fixture>,
    /// Fixtures that could not be made, with the reason.
    pub skipped: Vec<(&'static str, String)>,
}

impl Generated {
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("RW_FIXTURES_DIR") {
        return PathBuf::from(d);
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    target.join("fixtures")
}

fn ffmpeg_encoders() -> Result<BTreeSet<String>, String> {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .map_err(|e| {
            format!("could not run the ffmpeg CLI ({e}); install FFmpeg to generate fixtures")
        })?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text
        .lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let flags = parts.next()?;
            (flags.len() == 6 && flags.chars().all(|c| c.is_ascii_alphabetic() || c == '.'))
                .then(|| parts.next().map(str::to_owned))
                .flatten()
        })
        .collect())
}

fn run(args: &[String]) -> Result<(), String> {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

fn make(f: &Fixture, dir: &Path, encoders: &BTreeSet<String>) -> Result<(), String> {
    let enc = if f.encoders.is_empty() {
        None
    } else {
        Some(
            f.encoders
                .iter()
                .find(|e| encoders.contains(**e))
                .ok_or_else(|| format!("none of {:?} available", f.encoders))?,
        )
    };
    let dir_s = dir.to_string_lossy().into_owned();
    let subst = |a: &str, prev: &str| {
        a.replace("{enc}", enc.copied().unwrap_or(""))
            .replace("{fixtures}", &dir_s)
            .replace("{prev}", prev)
    };
    let out = dir.join(f.name);
    let ext = Path::new(f.name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let tmp1 = dir.join(format!(".{}.pass1.{ext}", f.name));
    let tmp2 = dir.join(format!(".{}.tmp.{ext}", f.name));
    let mut args: Vec<String> = f.args.iter().map(|a| subst(a, "")).collect();
    let first_out = if f.then.is_empty() { &tmp2 } else { &tmp1 };
    args.push(first_out.to_string_lossy().into_owned());
    run(&args)?;
    if !f.then.is_empty() {
        let prev = tmp1.to_string_lossy().into_owned();
        let mut args: Vec<String> = f.then.iter().map(|a| subst(a, &prev)).collect();
        args.push(tmp2.to_string_lossy().into_owned());
        run(&args)?;
        let _ = std::fs::remove_file(&tmp1);
    }
    std::fs::rename(&tmp2, &out).map_err(|e| e.to_string())
}

static LOCK: Mutex<()> = Mutex::new(());

/// Generates any missing fixtures and reports which are available.
pub fn ensure() -> Generated {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = dir();
    let mut g = Generated {
        dir: dir.clone(),
        ..Default::default()
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        g.skipped = ALL.iter().map(|f| (f.name, e.to_string())).collect();
        return g;
    }
    let mut encoders: Option<Result<BTreeSet<String>, String>> = None;
    for f in ALL {
        let path = dir.join(f.name);
        if path.metadata().is_ok_and(|m| m.len() > 0) {
            g.available.push(f);
            continue;
        }
        let encs = encoders.get_or_insert_with(ffmpeg_encoders);
        match encs
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|e| make(f, &dir, e))
        {
            Ok(()) => g.available.push(f),
            Err(e) => g.skipped.push((f.name, e)),
        }
    }
    g
}
