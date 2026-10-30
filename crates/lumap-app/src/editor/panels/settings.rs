//! The Settings window (Menu ▸ Settings): how lumap itself behaves, kept apart from the show.

use super::{Actions, PanelState};
use crate::settings::Settings;
use crate::update::flow::Mode;
use eframe::egui::{self, RichText};

/// Draws the Settings window while it is open.
pub fn settings_window(
    ctx: &egui::Context,
    state: &mut PanelState,
    settings: &Settings,
    actions: &mut Actions,
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
                let mut mode = state.update_mode;
                egui::ComboBox::from_label("Updates")
                    .selected_text(match mode {
                        Mode::Notify => "tell me",
                        Mode::Download => "download, then tell me",
                        Mode::Off => "never check",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut mode, Mode::Notify, "tell me");
                        ui.selectable_value(&mut mode, Mode::Download, "download, then tell me");
                        ui.selectable_value(&mut mode, Mode::Off, "never check");
                    });
                if mode != state.update_mode {
                    state.update_mode = mode;
                    actions.update_mode = Some(mode);
                }
                if ui.button("Check now").clicked() {
                    actions.update = Some(crate::update::Request::CheckNow);
                }
            });
            ui.label(
                RichText::new("Never during a show. Installing always asks first.")
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
