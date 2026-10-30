//! The Settings window (Menu ▸ Settings): how lumap itself behaves, kept apart from the show.

use super::{Actions, PanelState};
use crate::settings::Settings;
use crate::update::flow::{Mode, Stage};
use crate::update::{CheckUi, Request};
use eframe::egui::{self, RichText};

/// A consistent one-click update action on the welcome page, status bar and in Settings.
pub fn check_button(ui: &mut egui::Ui, check: &CheckUi, actions: &mut Actions, label: &str) {
    if check.checking && !check.enabled {
        ui.spinner();
        ui.label("Checking…").on_hover_text(&check.detail);
    } else if ui
        .add_enabled(check.enabled, egui::Button::new(label))
        .on_hover_text(&check.detail)
        .clicked()
    {
        actions.update = Some(Request::CheckNow);
    }
}


/// Draws the Settings window while it is open.
pub fn settings_window(
    ctx: &egui::Context,
    state: &mut PanelState,
    settings: &Settings,
    actions: &mut Actions,
    check: &CheckUi,
) {
    if !state.settings_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Settings")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .show(ctx, |ui| {
            crate::theme::section_header(ui, "Start");
            let mut show = !settings.skip_welcome;
            if ui
                .checkbox(&mut show, "Show the start page when lumap opens")
                .changed()
            {
                actions.start_page = Some(show);
            }

            crate::theme::section_header(ui, "Video");
            ui.label(if state.ffmpeg_ok {
                format!(
                    "ffmpeg found ({}): video plays.",
                    state.ffmpeg_origin.describe()
                )
            } else {
                "ffmpeg not found: video cannot play. Install it, or point lumap at it below."
                    .to_string()
            });
            ui.horizontal(|ui| {
                let label = ui.label("ffmpeg path");
                let r = ui
                    .text_edit_singleline(&mut state.ffmpeg_path)
                    .labelled_by(label.id);
                if r.lost_focus() {
                    actions.ffmpeg_path = Some(state.ffmpeg_path.trim().to_string());
                }
            });

            crate::theme::section_header(ui, "Keeping lumap current");
            ui.horizontal(|ui| {
                ui.strong(format!("lumap {}", env!("CARGO_PKG_VERSION")));
                ui.separator();
                ui.hyperlink_to("Latest releases", crate::update::offer::RELEASES_PAGE);
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                check_button(ui, check, actions, "Check for updates");
                let mut mode = state.update_mode;
                egui::ComboBox::from_label("Updates")
                    .selected_text(match mode {
                        Mode::Notify => "Notify only",
                        Mode::Download => "Download automatically",
                        Mode::Off => "Off (manual only)",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut mode, Mode::Notify, "Notify only");
                        ui.selectable_value(&mut mode, Mode::Download, "Download automatically");
                        ui.selectable_value(&mut mode, Mode::Off, "Off (manual only)");
                    });
                if mode != state.update_mode {
                    state.update_mode = mode;
                    actions.update_mode = Some(mode);
                }
            });
            ui.label(RichText::new(&check.detail).color(crate::theme::MUTED).small());
            if let Some(p) = &settings.update.pending {
                ui.add_space(6.0);
                ui.label(RichText::new(format!("lumap {} — update", p.version)).strong());
                ui.horizontal_wrapped(|ui| {
                    match p.stage {
                        Stage::Available => {
                            if ui.button("Download update").clicked() {
                                actions.update = Some(Request::Download);
                            }
                        }
                        Stage::Downloading => {
                            ui.spinner();
                            ui.label("Downloading and verifying securely in the background…");
                        }
                        Stage::Ready => {
                            ui.label("Downloaded and verified. Install when you are ready.");
                            if crate::theme::primary_button(ui, "Install and restart").clicked() {
                                actions.update = Some(Request::Install);
                            }
                            if ui.button("Later").clicked() {
                                actions.update = Some(Request::Later);
                            }
                        }
                        Stage::Installing => {
                            ui.label("Starting installer…");
                        }
                    }
                    let page = if p.page_url.starts_with("https://github.com/tochi-mba/lumap/releases/") {
                        p.page_url.as_str()
                    } else {
                        crate::update::offer::RELEASES_PAGE
                    };
                    ui.hyperlink_to("What's new", page);
                });
                if !p.notes.is_empty() {
                    ui.collapsing("Release notes", |ui| {
                        ui.label(&p.notes);
                    });
                }
            }
            ui.label(
                RichText::new("Checks run daily. By default, updates download and verify in the background. You choose when to install and restart. Updates pause during live shows.")
                    .weak()
                    .small(),
            );
            ui.add_space(8.0);
            if ui.button("Close settings").clicked() {
                state.settings_open = false;
            }
        });
    if !open {
        state.settings_open = false;
    }
}
