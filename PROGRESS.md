# Progress

| Date | Milestone | Landed | Blocked / notes |
| --- | --- | --- | --- |
| 2026-10-11 | M0 | Cargo workspace with 13 `rw-*` crates + `reelwright` app; egui/wgpu window that opens to the home screen and a docked Library / Preview / Inspector / Timeline editor; light/dark theme tokens with contrast test; bundled Inter font; original logo; `assets/ATTRIBUTION.md` + sync test; `deny.toml` (GPL/AGPL denied, `cargo deny check licenses` passes); `scripts/brand-lint.sh`; LGPL FFmpeg build scripts for Linux/macOS/Windows; offline test runner; GitHub Actions CI for all three OSes; UI smoke tests via `egui_kittest`. fmt, clippy `-D warnings`, tests and offline tests pass locally on Linux. | CI has not run yet (no remote). FFmpeg build scripts are untested. M1 needs the FFmpeg dev libraries and clang on the dev machine. |
