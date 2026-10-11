# Reelwright — agent guide

Offline, cross-platform desktop video editor in Rust. **`PLAN.md` is the brief**: product scope, architecture, milestones and acceptance criteria. Read it before starting work.

## Ground rules (non-negotiable)

- **Offline.** No network requests, ever: no telemetry, update checks, accounts or licence servers. Models and assets ship with the app or are imported from local disk. The integration suite must pass with networking disabled.
- **Rust only.** Native libraries (FFmpeg, ONNX Runtime, whisper.cpp) through Rust bindings are fine where no mature pure-Rust option exists. No Electron, web view, JavaScript, or runtime Python.
- **Original identity.** No names, logos, icons, colours, fonts, templates, media, UI text or help wording from the reference editor or its vendor. `scripts/brand-lint.sh` enforces the banned words outside `docs/` and `PLAN.md`; see "Branding and IP rules" in `PLAN.md`. The product name lives only in `rw_model::app::NAME`.
- **Local-first files.** Projects are plain files on disk that can be copied and opened elsewhere.
- **Licences.** Only MIT, Apache-2.0, BSD, Zlib, MPL-2.0, ISC (and the few documented exceptions in `deny.toml`). No GPL/AGPL. FFmpeg is the LGPL build, dynamically linked.

## How to work

1. Go milestone by milestone (M0–M10 in `PLAN.md`). Don't start one until the previous one's acceptance criteria pass.
2. Log each session in `PROGRESS.md`: date, milestone, what landed, what's blocked.
3. Record every new crate and its licence in `docs/DEPENDENCIES.md`, and pin it with `=x.y.z` in the root `Cargo.toml`.
4. When the plan is ambiguous, choose the simplest mainstream-editor behaviour and log it in `docs/DECISIONS.md`.
5. Every asset under `assets/` needs a row in `assets/ATTRIBUTION.md` (a test enforces this).
6. Before finishing a milestone run:
   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo deny check licenses
   scripts/brand-lint.sh
   ```

## Layout

`crates/rw-*` hold the libraries (model, edit, media, cache, render, fx, text, audio, engine, capture, ai, assets, ui); `app/` is the `reelwright` binary; `assets/` holds bundled fonts, icons and media; `scripts/` holds the brand lint and the FFmpeg build scripts; `docs/` holds decisions and dependencies.
