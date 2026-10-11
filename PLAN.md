# Offline Rust Video Editor — Build Plan

Oct 11, 2026 · @Maya

## How to use this plan

Build **Reelwright** (working name, change freely): a fully offline, cross-platform desktop video editor written in Rust that matches Clipchamp's local editing features, with original branding and no cloud dependency. Hand this whole document to a Claude Code agent as its brief; it is written to be followed top to bottom.

**Instructions to the agent**

1. Save this plan as `PLAN.md` at the repo root, and create a `CLAUDE.md` that summarises the ground rules below and points to `PLAN.md`.
2. Work milestone by milestone (see Milestones). Do not start a milestone until the previous one's acceptance criteria pass.
3. Keep a `PROGRESS.md` log: date, milestone, what landed, what's blocked. Update it at the end of every session.
4. Before adding any crate, check its licence is compatible (MIT, Apache-2.0, BSD, Zlib, MPL-2.0, ISC). Record each new dependency and its licence in `docs/DEPENDENCIES.md`.
5. When a spec here is ambiguous, pick the simplest behaviour that matches a mainstream consumer editor, note the decision in `docs/DECISIONS.md`, and keep going.
6. Write tests alongside code. Every milestone ends with `cargo test --workspace`, `cargo clippy --workspace -- -D warnings` and `cargo fmt --check` passing.

**Ground rules (non-negotiable)**

- **Offline.** The app must never make a network request. No telemetry, no update checks, no account system, no licence server. All models and assets ship with the app or are imported from local disk. Add a CI test that runs the app's integration suite with networking disabled.
- **Rust.** All application code is Rust. Native libraries linked through Rust bindings (FFmpeg, ONNX Runtime, whisper.cpp) are acceptable where no mature pure-Rust option exists; no Electron, no web view, no JavaScript, no Python at runtime.
- **Original identity.** No Microsoft or Clipchamp names, logos, icons, colours, fonts, templates, stock media, sound effects or UI text. See Branding and IP rules.
- **Local-first files.** Projects are plain files on disk the user can copy, back up and open on another machine.

**Definition of done**

- Every feature in the Product scope table is implemented and covered by at least one automated test or a documented manual test script.
- A 10-minute, 1080p30, 3-track project previews in real time on a mid-range laptop and exports without errors.
- Signed installers build in CI for Windows (x64), macOS (Apple Silicon and Intel) and Linux (AppImage + .deb).
- A clean grep of the repo and built binaries finds no "Microsoft", "Clipchamp" or "Windows" branding strings outside of `docs/` and OS-integration code.

## Product scope

The app covers Clipchamp's full local editing surface — recording, timeline editing, effects, text, audio, AI assists and export — and replaces every cloud-backed AI feature with an on-device model. Feature list taken from Clipchamp's [features page](https://clipchamp.com/en/features/), [what's new page](https://clipchamp.com/en/whats-new/) and [support index](https://support.microsoft.com/Clipchamp/video-editing-features); "Local approach" is how this app does it.

| Area | Feature | Local approach | Milestone |
| --- | --- | --- | --- |
| Project | Home screen: new, open, recent, duplicate, delete projects | Projects folder on disk; thumbnails cached locally | M1 |
| Project | Aspect ratio presets: 16:9, 9:16, 1:1, 4:3, 4:5, 2:3, 21:9, plus custom | Canvas size stored in project; resize re-fits items | M1 |
| Project | Autosave, unlimited undo/redo | Command pattern + periodic snapshot | M1 |
| Media | Import video, audio, images, GIFs by file picker, drag-drop or folder | FFmpeg probe; proxy generation for heavy files | M1 |
| Media | Media bin with search, filter by type, sort, used/unused badge | Local index | M1 |
| Media | Bundled asset library: backgrounds, shapes, stickers, music, SFX | Original or CC0 assets only; user can add packs from disk | M6 |
| Media | Colour and gradient backgrounds (presets, HSV picker, hex) | Generated clip type | M3 |
| Timeline | Multi-track video, audio, text, overlay tracks; magnetic main track | See Feature specs | M2 |
| Timeline | Split, trim (ripple and plain), delete, ripple delete, gap removal | Edit commands | M2 |
| Timeline | Multi-select, copy/paste, duplicate, group/ungroup | Selection model | M2 |
| Timeline | Snapping, zoom to fit, accurate timecode display, markers |  | M2 |
| Timeline | Detach audio from video, mute, lock/hide track |  | M2 |
| Editing | Crop (freehand and by ratio), rotate 0–360°, flip H/V | Per-item transform | M3 |
| Editing | Position, scale, fit/fill, picture-in-picture layouts | Floating toolbar presets | M3 |
| Editing | Speed 0.1× to 16× with pitch-preserved audio | Time-stretch in audio engine | M4 |
| Editing | Image duration control |  | M2 |
| Editing | Freeze frame and reverse | Beyond parity, cheap to add | M4 |
| Look | Colour adjust: exposure, brightness, contrast, saturation, temperature, highlights/shadows | WGSL shader | M3 |
| Look | Filters (preset LUTs: B&W, vintage, warm, cool, etc.) | Original .cube LUTs | M3 |
| Look | Effects: blur, glitch, noise, slow zoom, smoke/haze, vignette, VHS, pixelate, etc. | WGSL shader per effect | M4 |
| Look | Fade in/out (video and audio) |  | M3 |
| Look | Blend modes and opacity for overlays |  | M3 |
| Look | Green screen / chroma key with tolerance and spill | Shader | M4 |
| Transitions | Cross-dissolve, fade through black/white, wipes, slides, zoom, spin, glitch, burn, page turn, etc. (18+), adjustable duration | WGSL shader per transition | M4 |
| Text | Titles, lower thirds, end credits, captions styles; animated presets (fade, pop, typewriter, slide) | Text layout + animation presets | M5 |
| Text | Font, size, colour, outline, shadow, background box, alignment | Bundled open fonts + system fonts | M5 |
| Graphics | Stickers, emojis (open emoji set), vector shapes, arrows, logos | SVG and APNG/GIF overlays | M6 |
| Graphics | Brand kit: saved logos, fonts, colours | Local JSON + asset folder | M6 |
| Graphics | Audio visualiser (bars, wave, circle) | Analyse audio, draw in shader | M6 |
| Audio | Volume per clip and track, fades, separate/mute audio |  | M2 |
| Audio | Waveforms on timeline | Precomputed peaks | M2 |
| Recording | Screen, webcam, screen+webcam, microphone/voice recorder | Platform capture APIs | M7 |
| Templates | Starter templates (intro, promo, slideshow, social) | Original templates as project files | M9 |
| AI | Auto captions/subtitles, editable, styled, export SRT/VTT | Whisper on-device | M8 |
| AI | Text-to-speech voiceover, voice choice, speed and pitch | Piper-class neural TTS on-device | M8 |
| AI | Silence remover | VAD + energy threshold | M8 |
| AI | Noise suppression | RNNoise-class model | M8 |
| AI | Background removal (images and video) | Segmentation model via ONNX | M8 |
| AI | Video upscaler | Real-ESRGAN-class model via ONNX | M8 |
| AI | Auto compose (auto-edit a short from selected media) | Heuristic: scene detection + beat sync + template | M9 |
| Export | MP4 (H.264) at 480p, 720p, 1080p, 4K; GIF; MP3 audio-only | FFmpeg encode, hardware encoder if present | M5 |
| Export | Export selected range, subtitle sidecar files, progress + cancel |  | M5 |
| UX | Floating toolbar, property panel, keyboard shortcuts, full-screen preview, light/dark mode |  | M1–M10 |

**Excluded (cloud or account features):** sign-in and accounts; OneDrive, SharePoint, Dropbox, Google Drive, Xbox and GIPHY integrations; direct publish to YouTube, TikTok or LinkedIn; share links and review comments; team workspaces; online stock library; Copilot features; content-scheduling plans. Replace "publish" with a "Reveal in folder" button after export.

## Branding and IP rules

Match the functionality, not the look: the app gets its own name, palette, iconography, copy and assets, and nothing is taken from Clipchamp or Microsoft.

**Never use**

- The names Microsoft, Clipchamp, Windows, Office, Copilot, Fluent, Xbox, or any product name of theirs, in the UI, binaries, installers, store listings or code identifiers.
- Microsoft's Fluent UI icons, Segoe UI / Segoe Fluent fonts, Clipchamp's purple brand colour or gradient, its logo, its mascot or illustration style.
- Any Clipchamp template, stock video, image, music track, sound effect, sticker, filter preset, LUT, or transition name that is distinctive to them (generic names like "Cross dissolve" are fine).
- Screenshots of Clipchamp as reference images in the repo.
- Clipchamp's UI text or help-article wording. Write all copy fresh.

**Use instead**

- **Name and logo:** working name Reelwright (placeholder; check trademark availability before release). Agent creates a simple original SVG logo.
- **Icons:** [Lucide](https://lucide.dev) (ISC) or [Phosphor](https://phosphoricons.com) (MIT), bundled as SVG.
- **UI font:** Inter (OFL). **Title fonts:** a curated set of 30–40 OFL fonts from Google Fonts, bundled in `assets/fonts/` with their licence files.
- **Emoji/stickers:** [OpenMoji](https://openmoji.org) (CC BY-SA 4.0 — attribute in About) or Twemoji graphics (CC BY 4.0).
- **Music and SFX:** CC0 sources only (e.g. Freesound items explicitly marked CC0), or none at launch; record each file's source in `assets/ATTRIBUTION.md`.
- **Palette:** own palette defined in a theme file, e.g. a teal or amber accent, with light and dark variants.
- **Layout:** the general shape of a consumer editor (media panel left, preview centre, properties right, timeline bottom) is a common convention and fine to use. Don't pixel-match Clipchamp's spacing, panel names, or toolbar order.

**Enforcement**

- CI job `brand-lint`: fails the build if `rg -i "clipchamp|microsoft|fluent|segoe"` matches anything outside `docs/` and `PLAN.md`.
- Every file under `assets/` must have a row in `assets/ATTRIBUTION.md` (source URL, licence, author). A test checks the two stay in sync.

## Tech stack

Use egui on wgpu for the UI so the preview, timeline and effects all share one GPU device, and FFmpeg (LGPL build, via Rust bindings) for every codec. Pin exact versions in `Cargo.toml` at the start of M0 and record them in `docs/DEPENDENCIES.md`.

| Layer | Choice | Licence | Why / notes |
| --- | --- | --- | --- |
| Window + UI | `eframe` / `egui` with the wgpu renderer | MIT/Apache | Immediate mode suits a constantly-changing timeline; custom painting is easy; preview texture is drawn in-place via an `egui_wgpu` paint callback with no CPU copy. |
| Docking panels | `egui_dock` | MIT | Resizable media / preview / properties / timeline layout. |
| GPU compositing + effects | `wgpu` + WGSL shaders | MIT/Apache | Vulkan, Metal, DX12 from one codebase. |
| Decode / encode / mux | `ffmpeg-next` against FFmpeg 7.x or 8.x, **LGPL configuration** | WTFPL crate; FFmpeg LGPL-2.1 | Mature, but the crate is in maintenance-only mode ([README](https://docs.rs/crate/ffmpeg-next/9.0.0)). Wrap it behind an internal `MediaIo` trait so `rsmpeg` or another binding can be swapped in. |
| Hardware video | FFmpeg hwaccel: D3D11VA/NVDEC/QSV (Windows), VideoToolbox (macOS), VAAPI (Linux); encoders NVENC, QSV, AMF, VideoToolbox, VAAPI | — | Software fallback below. |
| Software H.264 encode | OpenH264 (via FFmpeg `libopenh264`) | BSD-2 | **Do not** link x264 (GPL). See Risks for patent notes. |
| Other export codecs | AV1 via SVT-AV1 or rav1e; VP9 via libvpx; AAC via FFmpeg native; MP3 via libmp3lame | BSD / LGPL | Offer WebM/AV1 as an extra. |
| GIF export | FFmpeg `palettegen` + `paletteuse` filters | LGPL | Avoid `gifski` (AGPL) and `imagequant` (GPL). |
| Audio out | `cpal` | Apache | Low-latency playback, device selection. |
| Audio DSP | Own mixer in Rust; `rubato` for resampling; FFmpeg `atempo` for pitch-preserving speed at first, swap for a Rust WSOLA stretcher later | MIT / LGPL | Avoid Rubber Band (GPL). |
| Text shaping + layout | `cosmic-text` (uses `swash`, `fontdb`) | MIT/Apache | Full Unicode shaping, BiDi, emoji, system fonts. |
| SVG stickers/shapes | `resvg` / `usvg` / `tiny-skia` | MIT/Apache | Rasterise to textures at the needed size. |
| Images | `image` | MIT/Apache | PNG, JPEG, WebP, GIF frames. |
| Project files | `serde` + `serde_json`, `uuid` | MIT/Apache | Human-readable, diffable. |
| Concurrency | `crossbeam-channel`, `rayon`, `parking_lot` | MIT/Apache |  |
| File dialogs | `rfd` | MIT | Native open/save dialogs. |
| ML runtime | `ort` (ONNX Runtime), CPU by default; DirectML / CoreML / CUDA execution providers when present | MIT/Apache | One runtime for background removal, upscaling, VAD, TTS. |
| Speech-to-text | `whisper-rs` (whisper.cpp) with ggml Whisper models | MIT | Offline captions in \~100 languages; ship `base` model, let users add larger ones from disk. |
| Text-to-speech | Neural TTS model in ONNX via `ort` (evaluate Kokoro-82M, Apache-2.0, and Piper voices with permissive licences) | varies — check each voice | Phonemiser licence is the trap: avoid GPL espeak-ng unless the whole app goes GPL. See Risks. |
| Noise suppression | `nnnoiseless` (pure-Rust RNNoise) | BSD-3 |  |
| Voice activity | Silero VAD in ONNX via `ort` | MIT | Drives silence removal. |
| Background removal | MODNet or U²-Net in ONNX | Apache-2.0 | Avoid models with non-commercial licences (e.g. some RMBG releases). |
| Upscaling | Real-ESRGAN (x2, x4) in ONNX | BSD-3 |  |
| Screen capture | Windows: `windows-capture` (Windows Graphics Capture); macOS: ScreenCaptureKit bindings; Linux: xdg-desktop-portal via `ashpd` + PipeWire | MIT / Apache | Behind a `Capture` trait per OS. |
| Webcam | `nokhwa` | Apache |  |
| Logging | `tracing`, `tracing-subscriber` (to local file only) | MIT |  |
| Packaging | `cargo-packager` or `cargo-dist`; MSI/NSIS, DMG, AppImage, .deb | MIT/Apache |  |

**Rejected:** Tauri/Electron (web view), GStreamer (heavy, harder to bundle on Windows/macOS), Slint (licence terms add friction for this use), Python-based ML at runtime.

## Architecture

One immutable project model drives two renderers that share the same compositor code: a real-time preview (drops frames, uses proxies) and an offline export (every frame, full resolution). The UI never touches decoders or the GPU pipeline directly; it sends commands and receives frames.

&#91;embedded content: crate architecture · 10 core crates\]

The UI only sends commands and receives frames; export reuses the same render path and hands finished frames back to rw-media's encoder.

**Cargo workspace**

```
reelwright/
  crates/
    rw-model      project, tracks, items, keyframes, serde schema, migrations
    rw-edit       command pattern: every edit is an undoable Command
    rw-media      MediaIo trait + FFmpeg impl: probe, decode, encode, mux, proxies, thumbnails, waveforms
    rw-cache      frame cache (GPU + CPU LRU), proxy + peak files on disk
    rw-render     compositor: evaluates the model at time t into a wgpu render graph
    rw-fx         effects, filters, transitions, LUTs (WGSL), parameter schemas
    rw-text       titles + captions: cosmic-text layout, animation presets
    rw-audio      mixer, fades, gain, speed/time-stretch, meters, cpal output
    rw-engine     playback clock, A/V sync, preview scheduler, export job runner
    rw-capture    screen, webcam, microphone recording (per-OS backends)
    rw-ai         whisper, TTS, VAD, denoise, segmentation, upscaling; model manager
    rw-assets     bundled library index, brand kit, templates
    rw-ui         egui app: panels, timeline widget, preview, property inspector, dialogs
  app/            binary: wires crates, loads settings, starts UI
  assets/         fonts, icons, LUTs, stickers, shapes, music, templates, models
  tests/          integration + golden-frame tests
  docs/
```

**Project model (`rw-model`)**

- `Project { id, name, canvas: {width, height, fps, background}, media: Vec<MediaRef>, tracks: Vec<Track>, markers, brand_kit_ref, version }`
- `Track { id, kind: Video|Audio|Text|Overlay, locked, hidden, muted, items: Vec<Item> }`
- `Item { id, start: Time, duration: Time, source_in: Time, kind, transform, crop, opacity, blend, speed, volume, fades, effects: Vec<EffectInstance>, group_id }`
- `kind` = `Media(MediaId)` | `Text(TextSpec)` | `Color/Gradient` | `Sticker` | `Shape` | `Visualiser` | `Caption(CaptionTrackId)`
- Transitions live between adjacent items on a track: `Transition { a, b, kind, duration }`.
- Every animatable property is `Animatable<T> { value, keyframes: Vec<(Time, T, Easing)> }`, even if the UI only exposes presets at first.
- Time is a rational `i64` tick count at 1/705,600,000 s (divisible by common frame and sample rates) to avoid float drift.
- Saved as `project.rwproj` (JSON) inside a project folder alongside `cache/`. Media is referenced by absolute path + content hash so moved files can be relinked. Schema has a `version` and a migration function per bump.

**Edits (`rw-edit`)** — every user action is a `Command` with `apply` and `revert`. The UI dispatches commands; the history stack holds them; autosave writes the project every 30 s and on idle. Multi-item edits are a single compound command.

**Rendering (`rw-render`)** — for time *t*, walk tracks bottom-to-top, resolve active items, request frames from `rw-cache`, then build a render graph: decode texture → crop → colour/filters → effects → transform → blend into the canvas. Transitions render both inputs to offscreen textures and run a transition shader. Work in linear-light RGBA16F; convert from source colour space (BT.709/BT.601/sRGB) on upload and to BT.709 on export.

**Threads**

1. UI thread (egui, 60 Hz).
2. Engine thread: playback clock (audio-driven), decides which frames are needed next.
3. Decoder pool: one worker per active source, decoding ahead into the cache.
4. GPU submission on the UI device (shared wgpu `Device`/`Queue`).
5. Audio callback thread (cpal): pulls mixed buffers from a lock-free ring.
6. Background jobs (rayon pool): proxies, thumbnails, waveform peaks, AI tasks, export. Each job reports progress and can be cancelled.

**Preview vs export** — preview renders at the preview panel's size, uses proxies for sources above 1080p or for long-GOP codecs, and drops frames to stay in sync with audio. Export runs the same compositor at full size on a separate wgpu queue (or the same device in sequence), never drops frames, and streams to the FFmpeg encoder through a bounded channel.

## Milestones

Eleven milestones, each ending in a runnable app; the editor is usable for simple cut-and-export work after M2, and feature-complete at M9. Each milestone's acceptance criteria are tests or scripted manual checks the agent runs before moving on.

1. **M0 — Scaffold.** Workspace, CI (Windows, macOS, Linux), `brand-lint`, licence check (`cargo-deny`), FFmpeg LGPL build script per OS, empty egui window with the docked layout and theme tokens.
   - Accept: CI green on all three OSes; app opens to an empty home screen; `cargo deny check licenses` passes with GPL/AGPL denied.
2. **M1 — Projects and media.** Home screen, new project with aspect presets, import (picker, drag-drop), media bin, probing, thumbnails, proxy generation, autosave, undo/redo framework.
   - Accept: import 20 mixed files (H.264, HEVC, ProRes, VP9, MOV with alpha, PNG, JPEG, GIF, MP3, WAV, AAC) without crash; reopen project after restart with identical state.
3. **M2 — Timeline and playback.** Multi-track timeline widget, drag to timeline, move, trim, split, delete, ripple delete, gap removal, snapping, zoom, multi-select, copy/paste, group, detach audio, volume, waveforms; preview with A/V sync; basic 1080p MP4 export (cuts only).
   - Accept: 3 video + 2 audio tracks play at 1080p30 with A/V drift under 1 frame over 10 min; all edit commands undo/redo exactly; exported cut list matches timeline frame-accurately (golden test).
4. **M3 — Transform and look.** Crop (freehand + ratio), rotate, flip, position/scale/fit/fill, opacity, blend modes, colour adjust, LUT filters, fades, colour/gradient backgrounds, floating toolbar, property panel.
   - Accept: golden-frame tests for every filter and blend mode; property changes preview in under 50 ms.
5. **M4 — Effects, transitions, speed.** 15+ effects, 18+ transitions with duration handles, chroma key, speed 0.1×–16× with pitch-preserved audio, reverse, freeze frame.
   - Accept: each effect and transition has a golden-frame test; 4× speed plays smoothly with audio pitch unchanged (spectral check in test).
6. **M5 — Text and full export.** Text items with styling and animated presets, lower thirds, end credits; export dialog: 480p/720p/1080p/4K MP4, GIF, MP3, range export, hardware encoder selection, progress, cancel, "Reveal in folder".
   - Accept: Unicode test strings (Arabic, Hindi, CJK, emoji) render correctly; 1-min 1080p export completes on CPU-only path and on each available hardware encoder.
7. **M6 — Graphics and assets.** Bundled asset library browser, stickers, emoji, vector shapes, logos, brand kit, audio visualiser.
   - Accept: every bundled asset has an attribution row; brand-kit fonts/colours apply to new text in one click.
8. **M7 — Recording.** Screen, webcam, screen+webcam, microphone; countdown, pause, retake; recordings land in the media bin.
   - Accept: 30-min screen+webcam+mic recording on each OS with A/V drift under 2 frames; permission prompts handled gracefully.
9. **M8 — On-device AI.** Model manager, auto captions (edit, style, burn-in, SRT/VTT), TTS voiceover, silence remover, noise suppression, background removal (image + video), upscaler.
   - Accept: whole M8 suite passes with networking disabled; captions on a 5-min English clip reach word error rate under 15% with the bundled model; each AI job is cancellable.
10. **M9 — Templates and auto compose.** 10+ original templates; auto compose from selected media (scene detection, beat sync, style choice, length target).
    - Accept: auto compose produces a valid, editable project from 10 clips and a music track in under 60 s.
11. **M10 — Polish and release.** Keyboard shortcuts + cheat sheet, full-screen preview, light/dark mode, accessibility pass (keyboard navigation, screen-reader labels via AccessKit), crash recovery, settings, signed installers.
    - Accept: Definition of done (top of this plan) is met.

## Feature specs

The behaviour below is the contract for each area; anything not specified follows mainstream consumer-editor conventions and gets logged in `docs/DECISIONS.md`.

### Layout

- Left: tabbed sidebar — Your media, Record, Text, Graphics (stickers, shapes, backgrounds), Music & SFX, Transitions, Templates, Brand kit.
- Centre: preview canvas with aspect-ratio selector, play/pause, timecode, full-screen, and on-canvas handles for move, scale, rotate and freehand crop. A floating toolbar above the canvas shows quick actions for the selected item (crop, flip, rotate, PiP layouts, fit/fill).
- Right: property panel with tabs for the selected item — Fade, Filters, Effects, Adjust colours, Speed, Audio, Transform, Text — and an AI tab (captions, voice, background removal, upscale) where relevant.
- Bottom: timeline with track headers (lock, hide, mute), ruler, playhead, zoom slider, "zoom to fit", snapping toggle, gap indicators with a delete button.

### Timeline behaviour

- The bottom-most video track is the main track and is magnetic: deleting an item closes the gap; dropping inserts and pushes later items. Other tracks are free-placement.
- Dropping media above or below existing tracks creates a new track; empty tracks are removed automatically.
- Split at playhead (`S`), delete (`Del`), ripple delete (`Shift+Del`), trim by dragging edges, slip by `Alt`+drag inside a clip.
- Snapping to playhead, item edges, markers, and transitions; hold `Ctrl/Cmd` to disable.
- Selection: click, `Shift`+click range, `Ctrl/Cmd`+click toggle, marquee drag. Property changes apply to all selected items of a compatible type.
- Grouping: `Ctrl/Cmd+G` links items so they move and trim together.
- Images default to 5 s; still-image duration adjustable by drag or numeric field.
- Thumbnails along video items, waveforms along audio items, both rendered from cache and updated as zoom changes.

### Visual tools

- **Transform:** position, scale (uniform and free), rotation 0–360°, flip H/V, fit/fill/stretch, picture-in-picture presets (corners, split screen).
- **Crop:** drag handles on canvas, or pick a ratio; non-destructive.
- **Colour adjust:** exposure, brightness, contrast, saturation, temperature, tint, highlights, shadows, vibrance; reset per slider.
- **Filters:** 20+ original LUT presets, with an intensity slider.
- **Effects (15+):** gaussian blur, background blur fill (for vertical-in-horizontal), glitch, RGB split, VHS, film grain/noise, vignette, pixelate, slow zoom (Ken Burns with start/end rect), shake, haze/smoke overlay, sharpen, black-and-white, sepia, invert, green screen. Each effect has an intensity plus 1–3 parameters.
- **Chroma key:** colour picker on canvas, tolerance, softness, spill suppression.
- **Blend modes:** normal, multiply, screen, overlay, darken, lighten, colour dodge, colour burn, add, difference.
- **Transitions (18+):** cross-dissolve, fade through black, fade through white, wipe (4 directions), slide (4), push, zoom in/out, spin, circle reveal, glitch, burn/light leak, blur dissolve, page turn. Drop between two adjacent items on any track; duration handle 0.1–3 s.

### Text and captions

- Text item with font, size, weight, colour, outline, shadow, background box with padding and corner radius, alignment, letter and line spacing.
- Animated presets: fade, pop, slide (4 directions), typewriter, word-by-word, bounce, zoom; each has in/out and duration.
- Title templates: centred title, lower third, quote card, countdown, end credits (scrolling list).
- Captions live on a caption track as timed segments; styles shared across the track; edit text inline; split/merge segments; export SRT/VTT; burn-in on export toggle.

### Audio

- Per-item gain (−∞ to +12 dB), fades in/out, mute; per-track gain; master meter with peak hold.
- Detach audio from video creates a linked audio item on an audio track.
- Speed changes keep pitch by default, with an option to let pitch follow speed.
- Optional: audio ducking (lower music under voice) — beyond parity, add if cheap.

### Recording

- Screen: pick display or window, include cursor, include system audio where the OS allows.
- Webcam: pick device, resolution, mirror toggle.
- Screen + webcam: records both as separate files, drops them on two tracks with the webcam as a PiP overlay.
- Microphone: device picker, input meter, 3-2-1 countdown.
- Record to a temp file with a fragmented container so a crash keeps what was captured; cap session at the user's disk space, not a fixed time.

### On-device AI

- **Model manager:** models stored in `assets/models/` (bundled small ones) and a user models folder; app shows size, licence and language support; user can import extra models from disk. No downloads in-app.
- **Captions:** run Whisper on the selected item's audio in a background job; language auto-detect or chosen; produce word-level timestamps; segment into caption lines of ≤ 42 characters and ≤ 2 lines.
- **Text-to-speech:** text box, voice picker, speed (0.5–2×), pitch (±12 semitones); generated WAV goes into media bin and onto an audio track at the playhead.
- **Silence remover:** VAD + level threshold; preview detected silences as shaded regions; slider for minimum silence length and padding; apply as a single undoable ripple edit.
- **Noise suppression:** per-item toggle with strength; processed non-destructively and cached.
- **Background removal:** images processed once; video processed per frame in a background job into a cached alpha matte, then composited like chroma key.
- **Upscaler:** 2× or 4×, writes a new media file and swaps the reference; warn about processing time with an estimate.
- **Auto compose:** user picks clips/images + optional music, target length (15/30/60 s), aspect, and style (energetic, calm, cinematic). Pipeline: scene detection, shot scoring (sharpness, motion, faces optional), beat detection on music, cut on beats, apply style's transitions/filters/text. Output is a normal editable project.

### Export

- Presets: 480p, 720p, 1080p, 4K (only when canvas ≥ 4K or user opts to upscale), GIF (max 30 s, 480p, 15 fps default), MP3 audio-only.
- Advanced: codec (H.264, HEVC if a hardware encoder exists, AV1, VP9), bitrate mode (quality/size), frame rate, range (whole project, selection, in/out markers).
- Output name defaults to the project name; overwriting asks first; after export show file size, duration, and "Reveal in folder".

### Keyboard shortcuts (defaults)

| Action | Shortcut |
| --- | --- |
| Play / pause | Space |
| Split at playhead | S |
| Delete / ripple delete | Del / Shift+Del |
| Undo / redo | Ctrl+Z / Ctrl+Shift+Z (Cmd on macOS) |
| Copy / paste / duplicate | Ctrl+C / Ctrl+V / Ctrl+D |
| Group / ungroup | Ctrl+G / Ctrl+Shift+G |
| Step frame back / forward | ← / → |
| Jump to previous / next edit | ↑ / ↓ |
| Zoom timeline in / out / fit | Ctrl+= / Ctrl+− / Shift+Z |
| Full-screen preview | F |
| Export | Ctrl+E |

## Testing, performance and packaging

Correctness is proven with golden-frame and round-trip tests, speed with fixed benchmarks on a reference project, and releases ship as signed native installers with every licence notice bundled.

**Tests**

- **Unit:** model serialisation round-trips; every `Command` applies then reverts to an identical model (property-based tests with `proptest`).
- **Golden frames:** render frame *n* of small fixture projects headless (wgpu on a software adapter in CI) and compare to stored PNGs with a perceptual tolerance. One fixture per effect, transition, blend mode, filter, and text preset.
- **Media matrix:** a `tests/fixtures/` set of short clips covering common codecs, frame rates (23.976, 25, 29.97, 30, 50, 60, variable), rotations, colour spaces, and audio layouts. Generated by a script with FFmpeg so no third-party footage is committed.
- **Export:** export fixtures, re-probe output, assert duration, frame count, resolution, codec and A/V offset.
- **Offline:** integration suite runs inside a network namespace / firewall rule with no network; any socket attempt fails the test.
- **UI smoke:** `egui_kittest` for key flows (create project, import, cut, export).

**Performance targets (reference: 4-core/8-thread laptop CPU, integrated GPU, 16 GB RAM)**

| Scenario | Target |
| --- | --- |
| App cold start to home screen | < 2 s |
| Open 10-min, 3-track project | < 3 s |
| Preview 1080p30, 3 video tracks + 2 effects | Real time, < 1% dropped frames |
| Preview 4K source | Real time using proxies |
| Scrub latency (seek to displayed frame) | < 100 ms with cache warm |
| Export 1080p30, 1 min, hardware encoder | < 30 s |
| Memory, 10-min 1080p project | < 1.5 GB |

**Packaging and licensing**

- App licence: choose MIT/Apache-2.0 or proprietary — both are compatible with an LGPL FFmpeg as long as FFmpeg is dynamically linked and replaceable.
- Ship FFmpeg as shared libraries next to the binary; include its LGPL text, build configuration, and a link to the exact source used.
- `cargo-deny` denies GPL, AGPL and unknown licences; `cargo-about` generates `THIRD_PARTY_LICENSES.html`, shown from Help › About.
- Windows: MSI or NSIS, Authenticode-signed. macOS: universal or per-arch DMG, notarised, hardened runtime with camera, microphone and screen-recording usage strings. Linux: AppImage and .deb; PipeWire required for screen capture.
- Bundled models and assets are versioned in `assets/manifest.json` with checksums; the app verifies them at start-up.

## Risks and open decisions

The biggest risks are codec patents, ML model and phonemiser licences, and real-time preview performance; none block starting, but each needs a decision before public release.

| Risk | Impact | Mitigation |
| --- | --- | --- |
| H.264/HEVC/AAC patent royalties | Distributing a software H.264 encoder can carry royalty obligations; hardware encoders and OS codecs usually cover this | Prefer OS/hardware encoders; make OpenH264 optional; offer AV1/VP9 as royalty-free defaults; get legal advice before commercial release |
| `ffmpeg-next` in maintenance-only mode | New FFmpeg APIs may lag | Keep all FFmpeg use behind `MediaIo`; pin FFmpeg major version; `rsmpeg` as fallback |
| TTS phonemiser licence | Most open neural TTS pipelines lean on espeak-ng (GPL-3) | Use a permissively licensed G2P (dictionary + rules) for launch languages, or ship TTS as a separately-licensed plugin |
| Model licences | Some background-removal and voice models are non-commercial | Licence column in model manager; `cargo-deny`-style check over `assets/manifest.json` |
| Real-time preview on weak GPUs | Stutter on integrated graphics with several effects | Proxies, preview resolution setting (full/half/quarter), effect caching, render-ahead |
| Screen capture on Linux/Wayland | Portal behaviour varies by desktop | Support PipeWire portal only; document supported desktops |
| Scope size | Feature parity is large for one agent | Strict milestone gates; M2 is a usable editor if work stops early |
| Trademark on the product name | "Reelwright" may be taken | Search trademark registers before release; name lives in one constant |

**Open decisions for you (agent: use the default unless told otherwise)**

- App licence — default: MIT/Apache-2.0 dual.
- Target platforms at first release — default: Windows, macOS and Linux together.
- Whether to bundle Whisper `small` (≈ 470 MB) or only `base` (≈ 140 MB) — default: `base`, with `small` importable.
- TTS languages at launch — default: English only.
- Beyond-parity extras (keyframe editor UI, audio ducking, freeze frame, reverse) — default: freeze frame and reverse in M4; the rest after M10.
