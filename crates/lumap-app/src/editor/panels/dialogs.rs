//! Modal prompts.

use super::super::{Editor, Pending};
use super::{Actions, PanelState, PendingChoice, SHORTCUTS};
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
        let (description, save_label, discard_label) = match &pending {
            Pending::Close => (
                "Save your show before quitting? Unsaved changes will be lost.",
                "Save and quit",
                "Quit without saving",
            ),
            Pending::InstallUpdate => (
                "The update will restart lumap. Save your show before installing?",
                "Save and install",
                "Install without saving",
            ),
            Pending::New => (
                "Save your current show before starting a new one?",
                "Save and create",
                "Create without saving",
            ),
            Pending::Open(_) => (
                "Save your current show before opening another?",
                "Save and open",
                "Open without saving",
            ),
        };
        egui::Modal::new(egui::IdSalt::new("lumap-unsaved-changes")).show(ctx, |ui| {
            ui.heading("Unsaved changes");
            ui.label(description);
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if crate::theme::primary_button(ui, save_label).clicked() {
                    actions.resolve_pending = Some(PendingChoice::Save);
                }
                if ui.button("Cancel").clicked() {
                    actions.resolve_pending = Some(PendingChoice::Cancel);
                }
                if crate::theme::danger_button(ui, discard_label).clicked() {
                    actions.resolve_pending = Some(PendingChoice::Discard);
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
