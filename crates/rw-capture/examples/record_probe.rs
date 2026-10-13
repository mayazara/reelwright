//! Records the screen (and the default microphone, if any) for a few
//! seconds: `cargo run -p rw-capture --example record_probe -- <seconds> <folder>`

use rw_capture::clock::{SystemTime, TimeSource};
use rw_capture::sources::{AudioDevice, microphones, open_audio};
use rw_capture::{Options, Recorder, Sources, devices};
use std::sync::Arc;

fn main() {
    let mut args = std::env::args().skip(1);
    let secs: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(5.0);
    let folder =
        std::path::PathBuf::from(args.next().unwrap_or_else(|| "target/recordings".into()));
    let screens = devices::screens(30, true);
    println!(
        "screens: {:?}",
        screens.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
    println!(
        "cameras: {:?}",
        devices::cameras(30)
            .iter()
            .map(|s| &s.label)
            .collect::<Vec<_>>()
    );
    println!("microphones: {:?}", microphones());
    let time: Arc<dyn TimeSource> = Arc::new(SystemTime::new());
    let screen = screens
        .first()
        .and_then(|s| {
            rw_media::DeviceReader::open(&s.spec)
                .map_err(|e| println!("{}", devices::explain_open_error("screen", &e.to_string())))
                .ok()
        })
        .map(|r| Box::new(r) as Box<dyn rw_capture::sources::VideoSource>);
    let audio: Vec<_> = open_audio(&AudioDevice::Input(None), time.clone())
        .map_err(|e| println!("microphone: {e}"))
        .into_iter()
        .collect();
    println!(
        "recording {secs} s (screen: {}, sound inputs: {})",
        screen.is_some(),
        audio.len()
    );
    let rec = Recorder::start(
        Sources {
            screen,
            camera: None,
            audio,
        },
        Options {
            folder,
            name: "Probe".into(),
            fps: 30,
            encoder: None,
        },
        time,
    )
    .expect("start");
    let t = std::time::Instant::now();
    while t.elapsed().as_secs_f64() < secs {
        std::thread::sleep(std::time::Duration::from_millis(500));
        println!(
            "  {:.1} s, levels {:?}",
            rec.elapsed().as_secs_f64(),
            rec.levels()
        );
    }
    let (files, problem) = rec.finish();
    println!("files: {files:?}, problem: {problem:?}");
}
