# Platforms

What lumap does on each platform, generated from `docs/platforms.json` by `cargo xtask platforms`; CI fails when this file is stale. A *supported* cell names the test or file that proves it; *planned* is on the way; *n/a* does not apply there.

Totals: windows: 23 supported, macos: 18 supported, linux: 18 supported.

| Capability | Windows | macOS | Linux | Proof |
|---|---|---|---|---|
| Outputs: one borderless window per projector, placed on a monitor | supported | supported | supported | `output_inspector_places_and_opens_windows` |
| Corner-pin homographies and bicubic Bezier meshes | supported | supported | supported | `gpu_matches_software_oracle` |
| Edge blending in linear light, blend suggestion | supported | supported | supported | `gpu_matches_software_oracle` |
| Calibration overlays (grid, crosshair, bars, grey steps, blend check) | supported | supported | supported | `gpu_matches_software_oracle` |
| PNG and JPEG sources | supported | supported | supported | `image_round_trip_and_errors` |
| Video through ffmpeg (bundled on Windows; PATH elsewhere) | supported | supported | supported | `ffmpeg_is_found_in_the_setting_then_beside_the_exe_then_on_the_path` |
| Image sequences and GIFs | supported | supported | supported | `folder_and_gif_sources` |
| Six animated generators | supported | supported | supported | `every_generator_is_deterministic_and_consistent` |
| Built-in looks (generators with colour, speed and blend) | supported | supported | supported | `applying_a_look_points_the_surface_at_it_and_is_idempotent` |
| Live input from Spout senders | supported | n/a | n/a | `lifecycle` |
| Live input from Syphon servers | n/a | planned | n/a | `` |
| OSC control and feedback | supported | supported | supported | `osc_patches_apply_live` |
| MIDI learn and control | supported | supported | supported | `control_section_learns_midi_and_runs_feedback_and_remote` |
| Phone remote over HTTP with a QR code | supported | supported | supported | `control_section_learns_midi_and_runs_feedback_and_remote` |
| Cues with crossfades | supported | supported | supported | `cue_panel_stores_fires_updates_and_reorders` |
| Locked show mode, autosave, crash recovery | supported | supported | supported | `show_mode_locks_edits_and_double_escape_exits` |
| Render-loop watchdog that relaunches the show | supported | supported | supported | `crates/lumap-app/src/watchdog.rs` |
| Headless render, validate, migrate, new | supported | supported | supported | `render_uses_the_gpu_when_one_is_available` |
| GPU backend (DirectX 12 / Metal / Vulkan) | supported | supported | supported | `windows_prefers_dx12_unless_the_environment_says_otherwise` |
| Installer (Inno Setup / .dmg / package) | supported | planned | planned | `installer/smoke-test.ps1` |
| Open .lumap.toml projects from the file manager | supported | planned | planned | `installer/smoke-test.ps1` |
| The lumap command on the PATH | supported | planned | planned | `installer/smoke-test.ps1` |
| In-app updates from GitHub Releases | supported | planned | planned | `notify_mode_checks_offers_downloads_on_request_and_installs_on_request` |
| Built and tested on every push | supported | supported | supported | `.github/workflows/ci.yml` |
