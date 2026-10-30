# Changelog

All notable changes to lumap, a REX Technologies product, are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- A new shell: lumap now works in four steps, **1 Setup** (projectors), **2 Map** (surfaces),
  **3 Content** (what they show) and **4 Show** (cues, control, going live), as tabs across the
  top with a tick on each finished step, a coach line that says what to do now with a
  *Next* button, and keys 1 to 4. The File, Edit and View menus become one *Menu*; Undo, Redo,
  Identify, Blackout, Master and **Go live** sit in the top bar; Grid, Snap, Edit on the wall,
  Stage view and Fit view sit above the canvas.
- A start page replaces the welcome window: create a projector per display, start from the
  demo, open a show, recent shows, and *Try a look*. Menu ▸ Home returns to it.
- App settings (start page, ffmpeg, updates) move to a Settings window; messages appear as
  toasts in the corner, and problems as a toast that stays until dismissed.
- **Setup step**: projector cards (colour, size, where each one is, and pills for an open
  window, an overlay or "off in show"), and one projector panel: a *Display* picker that puts
  the projector on a display at full size or back to a floating window, window and in-show
  toggles, Picture, Edge blending with *Suggest blends* and plainly named curves (Straight,
  Smooth, Soft), Calibration chips with a *Gamma match* helper, and a *Precise* section for
  size and position. The stage view is Setup's canvas.
- **Map step**: *Layers* lists the surfaces of the projector being mapped, top first, with
  an eye to show or hide, a lock badge, the content each one shows, and six dots to drag it
  up or down; right-click for the rest. A tab per projector, *Add surface* and *Add cut-out*
  sit above the canvas with the canvas toggles. Beside the selected surface a small action
  bar offers Lock, Duplicate, Flip, Rotate, Fit, Curve (or Corners) and Delete. The surface
  panel groups Content, Looks, Appearance (blend modes named Normal, Brighten, Light, Darken),
  Shape (*Make it curved*, *Flatten the curve*, *Back to four corners*, *Reset shape*),
  Arrange, Crop, Cut-outs and Precise (corners in pixels, order, curve patches, MIDI learn).
  The canvas readout says "Point 2 · 1024.0, 512.5 px".
- Information toasts let clicks through to whatever is under them; a problem shows at the
  top until dismissed.
- Plain words: Projector, Surface and Content in the interface. Project files and OSC
  addresses keep `output`, `surface` and `source`.

### Fixed

- The crop preview keeps its corner handles clear of the panel's scrollbar, which used to
  take the drag when the panel was narrow.
- A projector's width and height can be dragged or typed again: the fields used to re-read
  the size every frame, so a drag never accumulated.

## [0.4.0] - 2026-10-30

Looks: something worth seeing on the wall before you own a single clip.

### Added

- Looks: seventeen ready-made animated looks that need no media, chosen from *Try a look* on the
  welcome screen, the surface inspector's *Looks* section, or `lumap new FILE --look NAME`
  (`lumap looks` lists them). Among them *Anniversary* (soft hearts drifting up, beating
  together), *Candlelight*, *Edge chase*, *Night windows*, *Campfire*, *Northern lights* and
  *Disco floor*.
- Fourteen new generators behind them, grouped in the *+ source* menu: Edges (edge chase,
  outline, scan), Facade (bricks, windows, blocks, wireframe), Nature (water, fire, aurora,
  stars), Party (disco, kaleidoscope) and Romance (hearts). All are drawn by the CPU reference
  renderer and proven against the GPU by a new golden case.
- macOS groundwork: CI tests on macOS and builds a universal (Apple silicon + Intel),
  ad-hoc-signed binary with a macOS 12 floor on every push; `docs/platforms.md` (from
  `docs/platforms.json`, checked by `cargo xtask check-platforms`) says what works where and
  which test proves it; live inputs go through `lumap_sources::live_input` (Spout on Windows,
  Syphon planned), so a project naming a provider the platform lacks reports that plainly.

### Changed

- CI publishes releases: every green push to main whose `Cargo.toml` version has no release
  yet becomes `v<version>` on GitHub Releases, with the installer, its fixed-name copy, the
  portable zip and the SHA-256 files from the same run that installed and tested them. Bumping
  the version is the release; tags are no longer pushed by hand. `release.yml` only builds
  pre-releases on demand.

## [0.3.1] - 2026-10-29

### Fixed

- lumap no longer dies silently on launch on PCs whose Intel Vulkan driver crashes when wgpu
  creates the device: on Windows it now uses DirectX 12 unless `WGPU_BACKEND` says otherwise,
  in the window and in `lumap render` alike.

### Changed

- The REX Technologies look: the ink, panel, raised and line greys of the REX product sites,
  signal (`#D7FF3F`) as the one accent for selection, handles, the active cue and primary
  buttons, live (`#FF774D`) for show mode, blackout and destructive choices, and Inter
  (OFL 1.1, embedded) for every label. Welcome screen, toolbar, status bar, cue panel, show
  screen, dialogs, inspectors, handles and the stage view were tidied to match; the layout is
  unchanged and every button keeps its name. `docs/images/editor.png` and `welcome.png` are
  rendered by an ignored UI test (`render_doc_screenshots`).

## [0.3.0] - 2026-10-29

The first release with an installer and an updater: from here on lumap keeps itself current.

### Added

- Product site at <https://tochi-mba.github.io/lumap/>, in the REX Technologies style, with a
  download button that follows the visitor's platform and the latest release. `site/` is checked
  by `scripts/check_site.py` and its tests (`cargo xtask site`, also on CI) and published by the
  Pages workflow.
- `.portfolio/project.json`, so the REX portfolio lists lumap.
- Windows installer `lumap-Setup-<version>.exe` (Inno Setup, per user, no administrator
  prompt): bundles ffmpeg and ffprobe (a pinned LGPL build, verified by SHA-256), the demo
  project, the manual and the licences; offers a desktop shortcut, the `lumap` command on the
  PATH and opening `.lumap.toml` projects with lumap. Every release also publishes it under the
  fixed name `lumap-Setup.exe`, so `releases/latest/download/lumap-Setup.exe` is always the
  newest. CI installs, runs and uninstalls it on every push.
- lumap finds ffmpeg beside its own executable (`ffmpeg\ffmpeg.exe`, then `ffmpeg.exe`)
  between the settings path and the PATH, and the show inspector says which copy is in use.
- `lumap icon OUT` writes the app icon as a multi-size `.ico` (`cargo xtask icon` refreshes
  `assets/lumap.ico`).
- In-app updates: lumap asks GitHub for the latest release once a day (never during a show or
  with an output window open), offers it in the status bar, downloads the installer only when
  the *Updates* setting or the user says so, verifies it against the published SHA-256, and
  installs only on *Install and restart*. Later and Skip as you would expect; developer builds
  never update. Settings: `update.mode` (`notify`, `download`, `off`).

## [0.2.0] - 2026-10-29

First release under the lumap name, as a REX Technologies product, in its own repository.

### Added

- Welcome screen that creates an output per detected display, recent files, native file
  dialogs, drag-and-drop of media, unsaved-changes prompts and a keyboard cheat sheet.
- Edit on the wall: output windows show the same handles as the canvas; identify flashes
  each output's name on its projector; cursor hides on idle outputs.
- Canvas zoom and pan, output-space grid with snapping, numeric readout of the active
  handle, fine and coarse nudges, align operations, duplicate, lock, rename, reorder and
  context menus.
- Video (`ffmpeg` subprocess), image sequences, GIFs and six animated generators with a
  shared transport (`/source/<id>/play|loop|speed|restart`); thumbnails and a draggable
  crop preview.
- Cues: stored looks with crossfades, GO on Space, a button, OSC (`/cue/go`,
  `/cue/<id>/go`, `/cue/stop`), MIDI or the phone remote.
- MIDI learn for any parameter (fader, button and switch modes), saved in the project.
- OSC feedback of every changed parameter to a `host:port`.
- Phone remote: `lumap show --remote-port 8080` serves a page with blackout, master,
  output levels, surface opacities and the cue list; the editor shows a QR code of it.
- Settings file (`settings.toml` in the OS config directory) for ports, grid, snap,
  autosave interval and the ffmpeg path.
- Stage view (`T`): every placed output drawn to scale on the virtual desktop with monitor
  outlines and the overlaps highlighted; click selects, drag moves the placement.
- "Suggest blends" sets left/right or top/bottom blend widths from the overlap of placed
  outputs.
- Calibration overlays per output (grid, crosshair, colour bars, grey steps, blend check),
  never shown in show mode, and a gamma match tool (`[` / `]` nudge the blend gamma).
- Native Spout receiver on Windows (`--features spout`, vendored Spout2 SDK through
  `spout2-rs`, DirectX 11 CPU path, no `unsafe` in lumap); a stub elsewhere.
- Dark theme tuned for a booth, app and window icon, `docs/manual.md`.
- `cargo xtask file-lengths` (also in `cargo xtask ci`): no source file over 1000 lines.

### Changed

- Project files can name only the blend edges and playback fields they set; the rest take
  defaults (gamma 2.2, smooth curve, speed 1).
- Source files over 1000 lines were split into modules (`editor/state/`, `editor/panels/`,
  `project/`, `editor/ui_tests/`).
- Linux CI installs `libasound2-dev` (MIDI) and `ffmpeg`, and requires the video tests.

## [0.1.0] - 2026-10-29

### Added

- Corner-pin (homography) and bicubic Bezier mesh warps with perspective-correct sampling.
- Gamma-correct soft-edge blending on all four edges with linear, smoothstep and cosine
  curves; per-output brightness, contrast and gamma; master level and blackout.
- Feathered polygon masks (holes and windows) per surface; four blend modes; source crop.
- Sources: grid, bars, checker and ramp patterns, solid colours, PNG/JPEG images, and a
  Spout receiver interface behind the `spout` feature.
- Plain-text `.lumap.toml` projects with schema versioning, migration from the pre-release
  format, validation and atomic saves.
- egui editor with draggable handles, undo/redo with drag coalescing, inspector, object
  tree, and per-output borderless always-on-top windows.
- Locked show mode, autosave with recovery, render-loop watchdog with optional relaunch.
- OSC control over every live parameter.
- Headless CLI: `render`, `validate`, `migrate`, `new`.
- CPU reference renderer, wgpu renderer, golden-image tests and GPU-versus-oracle tests.
- CI on Linux and Windows with clippy, cargo-deny, coverage floors and a release workflow.

[Unreleased]: https://github.com/tochi-mba/lumap/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/tochi-mba/lumap/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/tochi-mba/lumap/releases/tag/v0.1.0
