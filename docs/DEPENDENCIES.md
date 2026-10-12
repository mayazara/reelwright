# Dependencies

Every third-party crate used directly by a workspace member, pinned with `=x.y.z` in the root `Cargo.toml`. Transitive licences are enforced by `cargo deny check licenses` (`deny.toml`).

## Rust crates

| Crate | Version | Licence | Used by | Purpose | Added |
| --- | --- | --- | --- | --- | --- |
| eframe | 0.36.2 | MIT OR Apache-2.0 | rw-ui, app | Native window + wgpu renderer for egui | M0 |
| egui | 0.36.2 | MIT OR Apache-2.0 | rw-ui (via eframe) | Immediate-mode UI | M0 |
| egui_dock | 0.21.1 | MIT | rw-ui | Docked panel layout | M0 |
| egui_kittest | 0.36.2 | MIT OR Apache-2.0 | rw-ui (dev) | Headless UI tests via AccessKit | M0 |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | rw-assets | Error types | M0 |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | app | Top-level error handling | M0 |
| tracing | 0.1.44 | MIT | app | Structured logging | M0 |
| tracing-subscriber | 0.3.23 | MIT | app | Log output (local file only) | M0 |
| directories | 6.0.0 | MIT OR Apache-2.0 | app | Platform data/config directories | M0 |
| serde | 1.0.229 | MIT OR Apache-2.0 | rw-model, rw-ui | Serialisation | M0 |
| serde_json | 1.0.152 | MIT OR Apache-2.0 | rw-model, rw-ui | Project file and settings format | M0 |
| uuid | 1.28.0 | Apache-2.0 OR MIT | rw-model | Random ids for projects, media, tracks, items | M1 |
| blake3 | 1.8.7 | CC0-1.0 OR Apache-2.0 | rw-model | Media content fingerprint for relinking and cache keys | M1 |
| ffmpeg-next | 9.0.0 | WTFPL (exception in `deny.toml`) | rw-media | FFmpeg bindings: probe, decode, encode, mux | M1 |
| ffmpeg-sys-next | 9.0.1 | WTFPL (exception in `deny.toml`) | rw-media (via ffmpeg-next) | Raw FFmpeg FFI for display matrix and pixel-format flags | M1 |
| image | 0.25.10 | MIT OR Apache-2.0 | rw-media | PNG read/write for cached thumbnails (`png` feature only) | M1 |
| rfd | 0.17.2 | MIT | rw-ui | Native open-file and folder dialogs | M1 |
| crossbeam-channel | 0.5.17 | MIT OR Apache-2.0 | rw-engine | Job queues and result channels | M1 |
| proptest | 1.12.0 | MIT OR Apache-2.0 | rw-model, rw-edit (dev) | Property tests for time maths and undo/redo | M1 |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | several (dev) | Temporary folders in tests | M1 |
| wgpu | 30.0.1 | MIT OR Apache-2.0 | rw-render (same version egui-wgpu uses) | GPU compositor, headless export device | M2 |
| pollster | 1.0.1 | Apache-2.0 OR MIT | rw-render | Blocking on wgpu adapter/device futures | M2 |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | rw-render | Uniform buffer layout | M2 |
| cosmic-text | 0.19.0 | MIT OR Apache-2.0 | rw-text | Text shaping (HarfRust), BiDi, line layout, font fallback and glyph rasterising (swash) | M5 |
| unicode-script | 0.5.8 | MIT OR Apache-2.0 | rw-text | Script ids for per-script font fallback | M5 |
| gif | 0.14.2 | MIT OR Apache-2.0 | rw-engine | GIF export (per-frame NeuQuant palettes via its `color_quant` dependency, MIT); FFmpeg's palette filters aren't in the trimmed build | M5 |
| resvg | 0.48.1 | Apache-2.0 OR MIT | rw-assets | SVG sticker rendering; its `tiny-skia` (BSD-3-Clause) draws shapes. Default features off (no text or embedded raster images) | M6 |
| cpal | 0.18.2 | Apache-2.0 | rw-audio | Sound card output (ALSA/PulseAudio, CoreAudio, WASAPI); needs `libasound2-dev` to build on Linux | M2 |
| ringbuf | 0.4.8 | MIT OR Apache-2.0 | rw-audio, rw-engine | Lock-free audio ring between mixer and output | M2 |

## Native libraries

| Library | Version | Licence | Linking | Notes |
| --- | --- | --- | --- | --- |
| FFmpeg | 6.1–8.x at build time; 8.0.1 for releases | LGPL-2.1+ | Shared, replaceable | Release builds use `scripts/ffmpeg/build-*.sh` (no GPL/nonfree components). Dev machines and Linux/macOS CI use the distro/Homebrew build, which is GPL but never shipped. Windows CI uses an LGPL shared build. |

## Bundled assets

See `assets/ATTRIBUTION.md`. M0 adds the Inter UI font (OFL-1.1) and the original logo.

## Tools (not shipped)

| Tool | Licence | Purpose |
| --- | --- | --- |
| cargo-deny | MIT OR Apache-2.0 | Licence, ban and source checks |
| ffmpeg CLI | LGPL/GPL (not shipped) | Generates synthetic test fixtures (`rw-media` `fixtures` feature) |
