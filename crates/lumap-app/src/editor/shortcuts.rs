//! Keyboard shortcuts: one place that reads the keys of a frame and acts on them.

use super::{panels, LumapApp, Pending, Selection};
use eframe::egui;
use std::time::{Duration, Instant};

/// Keys pressed this frame.
#[derive(Debug, Default, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
struct Shortcuts {
    undo: bool,
    redo: bool,
    save: bool,
    save_as: bool,
    open: bool,
    new: bool,
    duplicate: bool,
    show: bool,
    escape: bool,
    go: bool,
    delete: bool,
    blackout: bool,
    identify: bool,
    grid: bool,
    snap: bool,
    lock: bool,
    help: bool,
    reset_view: bool,
    stage: bool,
    gamma_down: bool,
    gamma_up: bool,
    step: Option<panels::shell::Step>,
}

impl LumapApp {
    #[allow(clippy::too_many_lines)]
    pub(super) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        let now = Instant::now();
        let typing = ctx.egui_wants_keyboard_input();
        let k = ctx.input_mut(|i| Shortcuts {
            undo: i.consume_key(Modifiers::COMMAND, Key::Z),
            redo: i.consume_key(Modifiers::COMMAND, Key::Y)
                || i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
            save: i.consume_key(Modifiers::COMMAND, Key::S),
            save_as: i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S),
            open: i.consume_key(Modifiers::COMMAND, Key::O),
            new: i.consume_key(Modifiers::COMMAND, Key::N),
            duplicate: i.consume_key(Modifiers::COMMAND, Key::D),
            show: i.consume_key(Modifiers::NONE, Key::F10),
            escape: i.consume_key(Modifiers::NONE, Key::Escape),
            go: !typing && i.consume_key(Modifiers::NONE, Key::Space),
            delete: i.consume_key(Modifiers::NONE, Key::Delete)
                || i.consume_key(Modifiers::NONE, Key::Backspace),
            blackout: i.consume_key(Modifiers::NONE, Key::B),
            identify: i.consume_key(Modifiers::NONE, Key::I),
            grid: i.consume_key(Modifiers::NONE, Key::G),
            snap: i.consume_key(Modifiers::NONE, Key::S),
            lock: i.consume_key(Modifiers::NONE, Key::L),
            help: i.consume_key(Modifiers::NONE, Key::Questionmark)
                || i.consume_key(Modifiers::SHIFT, Key::Slash)
                || i.consume_key(Modifiers::NONE, Key::F1),
            reset_view: i.consume_key(Modifiers::NONE, Key::Num0),
            step: [Key::Num1, Key::Num2, Key::Num3, Key::Num4]
                .into_iter()
                .zip(panels::shell::Step::ALL)
                .find(|(key, _)| !typing && i.consume_key(Modifiers::NONE, *key))
                .map(|(_, step)| step),
            stage: i.consume_key(Modifiers::NONE, Key::T),
            gamma_down: i.consume_key(Modifiers::NONE, Key::OpenBracket),
            gamma_up: i.consume_key(Modifiers::NONE, Key::CloseBracket),
        });
        if k.undo && !typing {
            self.editor.undo();
        }
        if k.redo && !typing {
            self.editor.redo();
        }
        if k.save {
            self.save();
        }
        if k.save_as {
            self.save_as();
        }
        if k.open {
            self.open_dialog();
        }
        if k.new {
            self.request(Pending::New);
        }
        if k.show {
            if self.editor.show_mode {
                self.editor.exit_show_mode();
            } else {
                self.editor.enter_show_mode();
            }
        }
        if k.escape {
            if self.panels.help || self.panels.settings_open {
                self.panels.help = false;
                self.panels.settings_open = false;
            } else {
                self.editor.press_escape(now, Duration::from_secs(2));
            }
        }
        if typing {
            return;
        }
        if k.delete {
            self.editor.delete_selection();
        }
        if k.duplicate {
            if let Selection::Surface { id, .. } = self.editor.selection.clone() {
                self.editor.duplicate_surface(&id);
            }
        }
        if k.go {
            let clock = self.editor.sources.time();
            self.editor.cue_go_next(clock);
        }
        if k.blackout {
            let on = self.editor.project.blackout;
            self.editor
                .set_param("/show/blackout", lumap_core::params::ParamValue::Bool(!on));
            self.editor.commit();
        }
        if k.identify {
            self.editor.identify(now);
        }
        if k.grid && !self.editor.show_mode {
            self.canvas.show_grid = !self.canvas.show_grid;
        }
        if k.snap && !self.editor.show_mode {
            self.canvas.handles.snap = !self.canvas.handles.snap;
        }
        if k.lock && !self.editor.show_mode {
            if let Selection::Surface { id, .. } = self.editor.selection.clone() {
                let locked = self.editor.is_locked(&id);
                self.editor.set_locked(&id, !locked);
            }
        }
        if k.help {
            self.panels.help = !self.panels.help;
        }
        if k.stage && !self.editor.show_mode {
            self.canvas.stage = !self.canvas.stage;
        }
        if (k.gamma_down || k.gamma_up) && !self.editor.show_mode {
            if let Some(id) = self.canvas.output.clone() {
                let delta = if k.gamma_up { 0.05 } else { -0.05 };
                self.editor.adjust_blend_gamma(&id, delta);
            }
        }
        if k.reset_view {
            self.canvas.reset_view();
        }
        if let Some(step) = k.step {
            if !self.editor.show_mode && !self.panels.home {
                self.panels.step = step;
            }
        }
    }
}
