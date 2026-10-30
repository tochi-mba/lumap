//! The update banner and the update settings, driven through the real UI. The updater never
//! touches the network here: a fresh app is inside its 45-second startup delay, so the state
//! is set directly and only the buttons and the settings round trip are exercised.

use super::*;
use crate::editor::Editor;
use crate::update::flow::{Mode, Pending, Stage, UpdateSettings};
use egui_kittest::kittest::Queryable;
use lumap_core::project::demo_project;

fn pending(stage: Stage) -> Pending {
    Pending {
        version: "0.9.0".into(),
        installer_name: "lumap-Setup-0.9.0.exe".into(),
        installer_url: "https://github.com/tochi-mba/lumap/releases/download/v0.9.0/lumap-Setup-0.9.0.exe".into(),
        checksum_url: "https://github.com/tochi-mba/lumap/releases/download/v0.9.0/lumap-Setup-0.9.0.exe.sha256".into(),
        size: 50 * 1_048_576,
        stage,
        ..Pending::default()
    }
}

#[test]
fn an_available_update_is_offered_in_the_status_bar_and_later_skip_work() {
    let _settings = settings_guard();
    let _dir = config_dir();
    let mut h = harness(Editor::new(demo_project(), None));
    assert!(h.query_by_label("lumap 0.9.0 (50 MB)").is_none());

    h.state_mut().settings.update = UpdateSettings {
        pending: Some(pending(Stage::Available)),
        ..UpdateSettings::default()
    };
    settle(&mut h);
    h.get_by_label("lumap 0.9.0 (50 MB)");
    h.get_by_label("Download");

    click(&mut h, "Later");
    assert!(
        h.query_by_label("lumap 0.9.0 (50 MB)").is_none(),
        "Later hides the offer"
    );
    assert!(h.state().settings.update.quiet_until.is_some());

    h.state_mut().settings.update.quiet_until = None;
    settle(&mut h);
    click(&mut h, "Skip");
    assert!(h.state().settings.update.pending.is_none());
    assert_eq!(h.state().settings.update.skipped.as_deref(), Some("0.9.0"));
    assert!(h.query_by_label("Skip").is_none());
}

#[test]
fn a_ready_update_offers_install_and_a_download_shows_progress_text() {
    let _settings = settings_guard();
    let _dir = config_dir();
    let mut h = harness(Editor::new(demo_project(), None));
    h.state_mut().settings.update.pending = Some(pending(Stage::Ready));
    settle(&mut h);
    h.get_by_label("lumap 0.9.0 is ready");
    h.get_by_label("Install and restart");

    h.state_mut().settings.update.pending = Some(pending(Stage::Downloading));
    settle(&mut h);
    h.get_by_label("Downloading lumap 0.9.0 (50 MB)");
    assert!(h.query_by_label("Install and restart").is_none());

    // Show mode keeps the banner away whatever the state.
    h.state_mut().editor.enter_show_mode();
    settle(&mut h);
    assert!(h
        .query_by_label("Downloading lumap 0.9.0 (50 MB)")
        .is_none());
}

#[test]
fn the_update_mode_is_chosen_in_the_show_inspector_and_saved() {
    let _settings = settings_guard();
    let dir = config_dir();
    let mut h = harness(Editor::new(demo_project(), None));
    assert_eq!(h.state().settings.update.mode, Mode::Download);
    menu(&mut h, "Menu", "Settings…");
    click(&mut h, "Updates");
    click(&mut h, "Notify only");
    assert_eq!(h.state().settings.update.mode, Mode::Notify);
    click(&mut h, "Updates");
    click(&mut h, "Off (manual only)");
    assert_eq!(h.state().settings.update.mode, Mode::Off);
    let saved = crate::settings::Settings::load_from(&dir.join("settings.toml")).unwrap();
    assert_eq!(saved.update.mode, Mode::Off);

    click(&mut h, "Check for updates");
    assert!(
        h.state()
            .editor
            .status
            .starts_with("Checking for a newer lumap"),
        "{}",
        h.state().editor.status
    );
}
