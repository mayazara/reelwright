//! Lists capture devices and grabs a few frames from the screen:
//! `cargo run -p rw-media --example capture_probe -- <format> <url>`

fn main() {
    let mut args = std::env::args().skip(1);
    let format = args.next().unwrap_or_else(|| "x11grab".into());
    let url = args.next().unwrap_or_else(|| ":0.0".into());
    for f in [
        "x11grab",
        "video4linux2",
        "gdigrab",
        "dshow",
        "avfoundation",
    ] {
        println!(
            "{f}: available={} devices={:?}",
            rw_media::capture::has_device_format(f),
            rw_media::capture::list_devices(f)
        );
    }
    let spec = rw_media::DeviceSpec {
        format,
        url,
        options: vec![("framerate".into(), "30".into())],
    };
    match rw_media::DeviceReader::open(&spec) {
        Ok(mut r) => {
            println!("opened, size {:?}", r.size());
            let t = std::time::Instant::now();
            for _ in 0..30 {
                let f = r.read().unwrap();
                let _ = f.size();
            }
            println!("30 frames in {:?}", t.elapsed());
        }
        Err(e) => println!("open failed: {e}"),
    }
}
