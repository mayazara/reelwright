# Decisions

Choices made where `PLAN.md` was ambiguous or silent. Newest last.

| Date | Milestone | Decision | Why |
| --- | --- | --- | --- |
| 2026-10-11 | M0 | The repo root is the Cargo workspace root (no extra `reelwright/` folder). | Simplest layout; matches the plan's tree with the root renamed. |
| 2026-10-11 | M0 | The product name and IDs live in `rw_model::app`. | The plan asks for the name to live in one constant; `rw-model` is depended on by everything. |
| 2026-10-11 | M0 | Accent palette: deep teal `#0E7C74` / `#2BB3A6` with amber highlight `#F2A541`; warm-neutral surfaces. Tokens in `rw-ui/src/theme.rs`, checked for WCAG AA contrast by a unit test. | Plan suggests teal or amber; both kept distinct from the reference editor's colours. |
| 2026-10-11 | M0 | egui's default fonts are disabled; Inter Regular/SemiBold are bundled and used for every family, including Monospace, until a mono face is needed. | Default egui fonts include the Ubuntu Font Licence, which is outside the allowed list; the plan specifies Inter for the UI. |
| 2026-10-11 | M0 | `deny.toml` also allows Unicode-3.0, BSL-1.0, CC0-1.0, MIT-0 and OFL-1.1. | All permissive and GPL-compatible; Unicode-3.0 is required by core crates (`unicode-ident`). GPL/AGPL remain denied. |
| 2026-10-11 | M0 | Licence files that ship with an asset (e.g. `Inter-OFL.txt`) get their own row in `assets/ATTRIBUTION.md`; only `ATTRIBUTION.md` and `manifest.json` are exempt. | Keeps the "every file has a row" rule literal and easy to test. |
| 2026-10-11 | M0 | Panels are named Library, Preview, Inspector, Timeline. | Own naming, per the branding rules. |
| 2026-10-11 | M0 | The offline test uses an unprivileged user + network namespace (`scripts/test-offline.sh`, Linux CI). | No root needed; any socket to a non-loopback address fails. |
| 2026-10-11 | M0 | `brand-lint` falls back to `grep` when `rg` is missing, and skips `Cargo.lock`. | Runs on dev machines without ripgrep; lockfile names are upstream crate names, not our branding. |
| 2026-10-11 | M0 | Logs go to `<data-local-dir>/reelwright/logs/reelwright.log`, `RUST_LOG` overrides the level. | Local file only, per the offline rule. |
