//! Operational state written once when it changes, so every run leaves a useful history.

use super::{camera, panels, LumapApp};

#[derive(Default)]
pub(super) struct Journal {
    status: String,
    error: Option<String>,
    camera: Option<camera::CameraStatus>,
    mode: Option<(bool, bool, bool)>,
    step: Option<panels::shell::Step>,
    outputs: Vec<String>,
    scan: bool,
    dirty: bool,
}

impl LumapApp {
    /// Record state transitions at the app boundary. Most failures ultimately become either
    /// the panel error or editor status; journalling those here means a new call site cannot
    /// accidentally show an error to the user without preserving it in the run log.
    pub(super) fn journal_changes(&mut self) {
        if self.editor.status != self.journal.status {
            if !self.editor.status.is_empty() {
                log::info!(target: "lumap::journal", "status: {}", self.editor.status);
            }
            self.journal.status.clone_from(&self.editor.status);
        }
        if self.panels.error != self.journal.error {
            if let Some(error) = &self.panels.error {
                log::error!(target: "lumap::journal", "user-visible error: {error}");
            }
            self.journal.error.clone_from(&self.panels.error);
        }
        let camera = self.camera.status();
        if self.journal.camera.as_ref() != Some(&camera) {
            match &camera {
                camera::CameraStatus::Failed(error) => {
                    log::error!(target: "lumap::journal", "camera failed: {error}");
                }
                _ => log::info!(target: "lumap::journal", "camera: {camera:?}"),
            }
            self.journal.camera = Some(camera);
        }
        let mode = (
            self.editor.show_mode,
            self.editor.project.blackout,
            self.panels.home,
        );
        if self.journal.mode != Some(mode) {
            log::info!(
                target: "lumap::journal",
                "mode: show={}, blackout={}, home={}", mode.0, mode.1, mode.2
            );
            self.journal.mode = Some(mode);
        }
        if self.journal.step != Some(self.panels.step) {
            log::info!(target: "lumap::journal", "step: {:?}", self.panels.step);
            self.journal.step = Some(self.panels.step);
        }
        if self.journal.outputs != self.editor.open_outputs {
            log::info!(target: "lumap::journal", "open outputs: {:?}", self.editor.open_outputs);
            self.journal.outputs.clone_from(&self.editor.open_outputs);
        }
        let scanning = self.scan.is_some();
        if self.journal.scan != scanning {
            log::info!(target: "lumap::journal", "scan active: {scanning}");
            self.journal.scan = scanning;
        }
        if self.journal.dirty != self.editor.dirty {
            log::debug!(target: "lumap::journal", "unsaved changes: {}", self.editor.dirty);
            self.journal.dirty = self.editor.dirty;
        }
    }
}
