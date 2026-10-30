# lumap

REX Technologies' projection-mapping app: a Rust workspace with an egui editor, a wgpu renderer, a CPU
reference renderer that the GPU is tested against, and a headless CLI. Windows is the target
platform for shows; everything builds and tests on Linux too, which is how this container and
CI exercise it.

Cargo root is the repository root. The workflows are `.github/workflows/ci.yml` (which also
publishes `v<version>` from main when the version is new) and `release.yml` (pre-releases on demand), the Claude skill is `.claude/skills/lumap-render/`, and the user
guide is `docs/manual.md`.

## Layout

- `crates/lumap-core/`: pure geometry (homographies, corner-pin quads, bicubic Bezier meshes),
  edge blending and the blend-width suggestion (`blend.rs`), masks, parameter addresses,
  cues (`cues.rs`: snapshots and the crossfade runner), the MIDI map (`midimap.rs`), undo,
  and the TOML project format in `project/` (`mod.rs` types and ids, `params.rs` get/apply,
  `validate.rs`, `migrate.rs`, `demo.rs`, `tests.rs`). No GPU, no I/O beyond the project
  file. Coverage floor 95 %.
- `crates/lumap-sources/`: RGBA frames, test patterns, calibration overlays (`overlay.rs`),
  images, video via an `ffmpeg` subprocess (`video.rs`), image sequences and GIFs, generators,
  the frame clock, the Spout receiver (`spout.rs`, native on Windows behind the `spout`
  feature, stub elsewhere), and `SourceSet` (every source of a project, polled per frame).
- `crates/lumap-render/`: `scene` (project output → draw list, overlays appended as a
  full-frame draw), `software` (f64 reference rasteriser, the oracle), `gpu` (wgpu two-pass
  renderer, same maths), `compare` (diff metrics). WGSL shaders in `src/shaders/`. Golden
  tests in `tests/golden.rs`.
- `crates/lumap-control/`: OSC server and OSC feedback, the midir MIDI listener, the phone
  remote (`web.rs`, tiny_http + `assets/remote.html`, QR modules).
- `crates/lumap-app/`: the `lumap` binary. `cli.rs` (render/validate/migrate/new),
  `settings.rs` (preferences file), `theme.rs` (visuals and icon), `autosave.rs`,
  `watchdog.rs`, and `editor/`: `state/` (all edits, headless: `mod.rs` core and params,
  `objects.rs` outputs/surfaces/sources, `control.rs` OSC/cues/MIDI/remote, `session.rs`
  files and show mode), `panels/` (egui: `mod.rs` menus/tree/status, `inspectors.rs`,
  `control.rs` cue panel and controller section, `dialogs.rs`), `canvas.rs`, `stage.rs`,
  `handles.rs`, `outputs.rs` (output windows), `thumbs.rs`, `ui_tests/` (egui_kittest drives
  the real UI).
- `site/`: the GitHub Pages site (static, no build step; REX ink/signal style shared with the
  other REX products). `scripts/check_site.py` is its gate, tested by `tests/site/`; the page
  script `site/app.js` is tested with `node --test`. `cargo xtask site` runs all three.
  `.portfolio/project.json` describes lumap to the REX portfolio.
- `crates/lumap-app/src/theme.rs`: the REX palette (`INK`, `PANEL`, `RAISED`, `LINE`, `TEXT`,
  `MUTED`, `SIGNAL`, `LIVE`), the embedded Inter fonts (`assets/fonts/`, OFL), the egui visuals,
  and the branded widgets (`section_header`, `primary_button`, `danger_button`, `pill`,
  `empty_state`). Signal is the only accent; live is for show mode, blackout and destructive
  choices; never add a third colour. `LumapApp::frame` applies the theme on its first frame.
  Visual changes: rerun the ignored `render_doc_screenshots` test (DX12 locally) and commit the
  images.
- `crates/lumap-app/src/update/`: the updater. `offer.rs` (release JSON to an offer, version
  rules, SHA-256) and `flow.rs` (every decision over the persisted `UpdateSettings`) are pure
  and fully unit-tested; `worker.rs` fetches through a `Fetcher` (the default runs `curl`, so
  no HTTP crate) on a thread; `mod.rs` ties them to the editor. Rules: nothing happens during a
  show or with an output window open, nothing in the first 45 s, installing always asks, only
  installers hosted by this repository with a matching `.sha256` are ever run.
- `installer/`: `build.ps1 -Version x.y.z` builds `dist\lumap-Setup-<version>.exe` (Inno Setup 6,
  `lumap.iss`) and the portable zip; `fetch-ffmpeg.ps1` downloads the pinned LGPL ffmpeg build
  it bundles (change `$Url` and `$Sha256` together); `smoke-test.ps1` installs, runs and
  uninstalls an installer the way CI and the release job do. `assets/lumap.ico` comes from
  `cargo xtask icon` (`lumap icon`), never from an image editor.
- `xtask/`: `cargo xtask ci` = the CI gate; `check-coverage FILE` enforces floors;
  `file-lengths` rejects source files over 1000 lines.
- `tests/golden/`: golden PNGs (256×144) and `cases/*.lumap.toml`.
- `examples/two-projector-demo.lumap.toml`: the demo project (`lumap new` writes the same).
- `docs/images/`: README renders, produced by `lumap render`.

## Build and test

- Toolchain is pinned in `rust-toolchain.toml`; `rust-version` in `Cargo.toml` is the MSRV CI
  checks.
- `cargo xtask ci` before every push. It runs the file-length check, fmt, clippy (`-D
  warnings`, pedantic), rustdoc, cargo-deny, all tests and the coverage floors (85 %
  workspace, 95 % core).
- GPU tests need a Vulkan adapter. In this container `apt-get install mesa-vulkan-drivers`
  provides lavapipe; run tests with `WGPU_BACKEND=vulkan`. `LUMAP_REQUIRE_GPU=1` turns a
  missing adapter into a failure (CI sets it on Linux). Video tests use `ffmpeg` from the
  path (`LUMAP_REQUIRE_FFMPEG=1` on CI); MIDI needs `libasound2-dev` on Linux.
- `UPDATE_GOLDENS=1` regenerates goldens; see the skill before doing that.
- The UI is tested through egui's accessibility tree, so every interactive widget needs an
  accessible name (label text fields and drag values with `labelled_by`, combo boxes with
  `from_label`). Unique button labels make tests simpler; avoid non-ASCII glyphs that the
  default font lacks. Combo options are clicked by label after clicking the combo's label.
- The Spout native path only compiles on Windows (MSVC, x64); CI's Windows job builds it.
  It cannot be run here, so keep `spout.rs`'s native module small and obviously correct.

## Platforms

- Platform code lives in exactly two places: `lumap-sources::live_input` (Spout today, Syphon
  planned) and `lumap-render::gpu::preferred_backends`. No `cfg(windows)` / `cfg(target_os)`
  anywhere else; a new platform concern gets a thin module there with a trait or an enum.
- `docs/platforms.json` is the capability matrix; `cargo xtask platforms` renders
  `docs/platforms.md` and `check-platforms` (in `xtask ci` and CI's lint job) fails when the
  Markdown is stale or a `supported` cell names no existing test or file. Change the json, never
  the md.
- CI tests on Linux, Windows and macOS and builds a universal, ad-hoc-signed macOS binary with
  `MACOSX_DEPLOYMENT_TARGET=12.0`. The Mac is not released yet: `site/app.js` `SUPPORTED` and the
  publish job are where that turns on, after a `.dmg` and a Syphon input exist.

## Conventions

- `#![forbid(unsafe_code)]` everywhere, including the Spout receiver (the `spout2` crate's
  DirectX CPU path is safe to call).
- No source file over 1000 lines; split into a module directory by concern.
- New live-controllable values get a parameter address in `lumap-core::project` (`get_param`
  and `apply_patch`) so OSC, MIDI, cues, undo and the inspector all work without extra code.
- Rendering changes must keep `gpu_matches_software_oracle` green. Change the oracle and the
  shader together; the oracle is the specification.
- Project schema changes bump `SCHEMA_VERSION` and add a `migrate_N_to_N+1` arm plus a test
  with a literal old file. Optional fields with serde defaults do not need a bump.
- Conventional-commit style subjects prefixed `lumap:`; keep `CHANGELOG.md` current.
