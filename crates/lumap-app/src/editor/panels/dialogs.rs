//! Modal prompts.

use super::super::{Editor, Pending};
use super::{Actions, PanelState, SHORTCUTS};
use eframe::egui::{self, RichText};

/// Modal prompts: autosave recovery, unsaved changes and the cheat sheet.
pub fn dialogs(
    ctx: &egui::Context,
    editor: &mut Editor,
    state: &mut PanelState,
    actions: &mut Actions,
) {
    if let Some(auto) = editor.recovery.clone() {
        egui::Window::new("Recover autosave?")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "An autosave newer than the project exists:\n{}",
                    auto.display()
                ));
                ui.horizontal(|ui| {
                    if ui.button("Recover").clicked() {
                        editor.recover();
                    }
                    if crate::theme::danger_button(ui, "Discard").clicked() {
                        editor.discard_recovery();
                    }
                });
            });
    }
    if let Some(pending) = editor.pending.clone() {
        let what = match &pending {
            Pending::Close => "quitting",
            Pending::New => "starting a new project",
            Pending::Open(_) => "opening another project",
        };
        egui::Window::new("Unsaved changes")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("Save before {what}?"));
                ui.horizontal(|ui| {
                    if ui.button("Save and continue").clicked() {
                        actions.save = true;
                        actions.resolve_pending = Some(true);
                    }
                    if crate::theme::danger_button(ui, "Discard changes").clicked() {
                        actions.resolve_pending = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        actions.resolve_pending = Some(false);
                    }
                });
            });
    }
    if state.help {
        egui::Window::new("Keyboard shortcuts")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                for (group, keys) in SHORTCUTS {
                    crate::theme::section_header(ui, group);
                    egui::Grid::new(("shortcuts", *group))
                        .num_columns(2)
                        .min_col_width(150.0)
                        .spacing([24.0, 4.0])
                        .show(ui, |ui| {
                            for (k, what) in *keys {
                                ui.label(RichText::new(*k).monospace());
                                ui.label(*what);
                                ui.end_row();
                            }
                        });
                }
                if ui.button("Close").clicked() {
                    state.help = false;
                }
            });
    }
}
