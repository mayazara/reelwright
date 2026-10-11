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
| serde | 1.0.229 | MIT OR Apache-2.0 | — (pinned for M1) | Serialisation | M0 |
| serde_json | 1.0.152 | MIT OR Apache-2.0 | — (pinned for M1) | Project file format | M0 |

## Native libraries

| Library | Version | Licence | Linking | Notes |
| --- | --- | --- | --- | --- |
| FFmpeg | 8.0.1 (planned, M1) | LGPL-2.1+ | Shared, replaceable | Built by `scripts/ffmpeg/build-*.sh` with no GPL/nonfree components |

## Bundled assets

See `assets/ATTRIBUTION.md`. M0 adds the Inter UI font (OFL-1.1) and the original logo.

## Tools (not shipped)

| Tool | Licence | Purpose |
| --- | --- | --- |
| cargo-deny | MIT OR Apache-2.0 | Licence, ban and source checks |
