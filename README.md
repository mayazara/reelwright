# Reelwright

A video editor that runs entirely on your computer. No account, no cloud,
no internet connection — your projects and media never leave your machine.

> **Early development.** Projects, importing, a multi-track timeline,
> playback, cutting and trimming, transforms, crop, colour adjustment,
> filters, blend modes, fades, colour/gradient backgrounds and MP4 export
> work today. Text, effects, transitions, recording and on-device AI tools
> are on the way — see [`PLAN.md`](PLAN.md) and [`PROGRESS.md`](PROGRESS.md).

## Install

### Windows (10 or 11, 64-bit)

Download from the [Releases page](../../releases/latest):

- **`Reelwright-<version>-windows-x64-setup.exe`** — double-click to
  install. No administrator rights needed. Adds Start-menu and desktop
  shortcuts; remove it any time from *Settings → Apps*.
- **`Reelwright-<version>-windows-x64-portable.zip`** — or unzip anywhere
  and run `Reelwright.exe`. Nothing is installed.

Nothing else is required: video codecs, fonts and filters are included,
and it runs without a dedicated graphics card.

**"Windows protected your PC"?** Builds aren't code-signed yet, so
SmartScreen may warn the first time. Click **More info → Run anyway**.

Projects are saved in your **Videos\Reelwright** folder. Uninstalling
never deletes them.

### macOS

There's no packaged build yet; build from source (below). You'll need:

```sh
xcode-select --install          # compiler tools
brew install ffmpeg pkg-config  # Homebrew: https://brew.sh
curl https://sh.rustup.rs -sSf | sh
```

### Linux

There's no packaged build yet; build from source (below). On Ubuntu or
Debian:

```sh
sudo apt install build-essential pkg-config clang libclang-dev \
  libavcodec-dev libavformat-dev libavutil-dev libswscale-dev \
  libswresample-dev libavfilter-dev libavdevice-dev \
  libasound2-dev libxkbcommon-dev libwayland-dev
curl https://sh.rustup.rs -sSf | sh
```

Fedora: `sudo dnf install clang-devel ffmpeg-free-devel alsa-lib-devel
libxkbcommon-devel wayland-devel pkgconf` (or the full `ffmpeg-devel` from
RPM Fusion for more codecs).

**WSL:** the app runs under WSLg. For sound, route ALSA to WSLg's
PulseAudio server once:

```sh
printf 'pcm.!default pulse\nctl.!default pulse\n' > ~/.asoundrc
```

## Build from source (all platforms)

Requires Rust 1.95 or newer and FFmpeg 6.1 or newer (with development
headers), installed as above.

```sh
git clone https://github.com/mayazara/reelwright.git
cd reelwright
cargo run --release
```

On Windows, building from source needs the MSVC toolchain, LLVM (for
`libclang`) and an FFmpeg shared build with headers: set `FFMPEG_DIR` to
the unpacked folder and add its `bin` to `PATH`. The release workflow
([`.github/workflows/release.yml`](.github/workflows/release.yml)) shows
the exact steps.

Check that a build can do its core work (load FFmpeg, render, encode):

```sh
cargo run --release -- --self-test report.txt && cat report.txt
```

## Development

```sh
cargo test --workspace        # unit, integration and UI tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check licenses     # GPL/AGPL dependencies are not allowed
scripts/brand-lint.sh
```

Tests generate their own media with the `ffmpeg` command-line tool, so it
needs to be on `PATH`. GPU tests use whatever adapter is present, including
software renderers.

[`CLAUDE.md`](CLAUDE.md) has the ground rules for contributors and agents;
decisions are logged in [`docs/DECISIONS.md`](docs/DECISIONS.md).

## Licence

Reelwright is dual-licensed under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE). Builds include FFmpeg under the LGPL 2.1 as
separate, replaceable libraries; third-party notices ship in the
`licenses` folder of each download.
