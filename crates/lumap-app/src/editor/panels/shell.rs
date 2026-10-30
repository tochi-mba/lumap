//! The frame every step shares: the top bar with the four step tabs, the coach line that says
//! what to do next, the canvas toolbar and the toasts.

use super::super::canvas::CanvasState;
use super::super::state::Readiness;
use super::super::Editor;
use super::{param_slider, Actions, PanelState};
use crate::settings::Settings;
use eframe::egui::{self, Color32, RichText};
use lumap_core::params::ParamValue;
use std::time::{Duration, Instant};

/// The four steps of making a show, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Step {
    /// Projectors: create, place, identify, blend, calibrate.
    #[default]
    Setup,
    /// Surfaces: draw them and drag their corners onto the wall.
    Map,
    /// What each surface shows: looks, media, live inputs.
    Content,
    /// Cues, controllers, going live.
    Show,
}

impl Step {
    /// Every step, in order.
    pub const ALL: [Self; 4] = [Self::Setup, Self::Map, Self::Content, Self::Show];

    /// 1 to 4.
    pub fn number(self) -> usize {
        match self {
            Self::Setup => 1,
            Self::Map => 2,
            Self::Content => 3,
            Self::Show => 4,
        }
    }

    /// The tab label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Setup => "Setup",
            Self::Map => "Map",
            Self::Content => "Content",
            Self::Show => "Show",
        }
    }

    /// The step after this one.
    pub fn next(self) -> Option<Self> {
        match self {
            Self::Setup => Some(Self::Map),
            Self::Map => Some(Self::Content),
            Self::Content => Some(Self::Show),
            Self::Show => None,
        }
    }

    /// Whether the show has what this step is for.
    pub fn done(self, r: &Readiness) -> bool {
        match self {
            Self::Setup => r.setup_done(),
            Self::Map => r.map_done(),
            Self::Content => r.content_done(),
            Self::Show => r.show_done(),
        }
    }

    /// The first step that still needs doing, or Show when everything is.
    pub fn first_open(r: &Readiness) -> Self {
        Self::ALL
            .into_iter()
            .find(|s| !s.done(r))
            .unwrap_or(Self::Show)
    }

    /// One sentence on what to do in this step, given how far the show has got.
    pub fn coach(self, r: &Readiness) -> &'static str {
        match (self, self.done(r)) {
            (Self::Setup, false) => "Plug in your projectors and create one for each display. Identify flashes every projector's name on its wall.",
            (Self::Setup, true) => "Projectors are ready. Place each one on its display, blend the overlaps, then draw your surfaces.",
            (Self::Map, false) => "Add a surface for each thing you want to light, then drag its corners onto it, here or on the wall itself.",
            (Self::Map, true) => "Surfaces mapped. Drag corners to fine-tune: G shows a grid, S snaps, arrows nudge.",
            (Self::Content, false) => "Give every surface something to show: pick a look, or add your own video, images or a live input.",
            (Self::Content, true) => "Every surface has something to show. Store the looks you like as cues next.",
            (Self::Show, false) => "Store this look as a cue, change something, store another. Space fades between them. Go live when ready.",
            (Self::Show, true) => "Space fades to the next cue. Go live locks the show; press Escape twice to leave it.",
        }
    }
}

/// The top bar: brand, show name, step tabs, the menu, undo, identify, blackout, master and Go
/// live.
#[allow(clippy::too_many_lines)]
pub fn top_bar(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    state: &mut PanelState,
    settings: &Settings,
    actions: &mut Actions,
) {
    let readiness = editor.readiness();
    egui::Panel::top("lumap-top").show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("lumap")
                    .size(16.0)
                    .family(crate::theme::strong_family())
                    .color(crate::theme::SIGNAL),
            );
            let mut name = editor.project.name.clone();
            let label = ui.label(RichText::new("Show").weak().small());
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(170.0));
            let r = r.labelled_by(label.id).on_hover_text("The show's name");
            if r.lost_focus() && name != editor.project.name {
                editor.edit(|p| p.name = name);
            }
            ui.add_space(10.0);
            for step in Step::ALL {
                let current = state.step == step;
                if crate::theme::step_tab(
                    ui,
                    step.number(),
                    step.label(),
                    current,
                    step.done(&readiness),
                )
                .on_hover_text(format!("Press {} to come here", step.number()))
                .clicked()
                {
                    state.step = step;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::theme::primary_button(ui, "Go live")
                    .on_hover_text("Lock editing and open every projector (F10)")
                    .clicked()
                {
                    editor.enter_show_mode();
                }
                param_slider(ui, editor, "Master", "/show/master", 0.0..=1.0);
                let blackout = editor.project.blackout;
                if ui
                    .add(
                        egui::Button::new(if blackout {
                            RichText::new("BLACKOUT").color(crate::theme::LIVE)
                        } else {
                            RichText::new("Blackout")
                        })
                        .fill(if blackout {
                            crate::theme::LIVE.linear_multiply(0.2)
                        } else {
                            Color32::TRANSPARENT
                        }),
                    )
                    .on_hover_text("Every projector black (B)")
                    .clicked()
                {
                    editor.set_param("/show/blackout", ParamValue::Bool(!blackout));
                    editor.commit();
                }
                if ui
                    .button("Identify")
                    .on_hover_text("Flash each projector's name on its wall (I)")
                    .clicked()
                {
                    editor.identify(Instant::now());
                }
                if ui
                    .add_enabled(editor.history.can_redo(), egui::Button::new("Redo"))
                    .on_hover_text("Ctrl+Y")
                    .clicked()
                {
                    editor.redo();
                }
                if ui
                    .add_enabled(editor.history.can_undo(), egui::Button::new("Undo"))
                    .on_hover_text("Ctrl+Z")
                    .clicked()
                {
                    editor.undo();
                }
                menu(ui, state, settings, actions);
            });
        });
        ui.add_space(4.0);
    });
}

/// The one menu: home, files, settings, shortcuts, updates.
fn menu(ui: &mut egui::Ui, state: &mut PanelState, settings: &Settings, actions: &mut Actions) {
    ui.menu_button("Menu", |ui| {
        if ui.button("Home").clicked() {
            state.home = true;
            ui.close();
        }
        ui.separator();
        if ui.button("New show").on_hover_text("Ctrl+N").clicked() {
            actions.new = true;
            ui.close();
        }
        if ui.button("Open…").on_hover_text("Ctrl+O").clicked() {
            actions.open = true;
            ui.close();
        }
        let recent = settings.existing_recent();
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button("Recent shows", |ui| {
                for p in recent.iter().take(8) {
                    let name = p.file_name().map_or_else(
                        || p.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    if ui
                        .button(name)
                        .on_hover_text(p.display().to_string())
                        .clicked()
                    {
                        actions.open_path = Some(p.clone());
                        ui.close();
                    }
                }
            });
        });
        if ui.button("Save").on_hover_text("Ctrl+S").clicked() {
            actions.save = true;
            ui.close();
        }
        if ui
            .button("Save as…")
            .on_hover_text("Ctrl+Shift+S")
            .clicked()
        {
            actions.save_as = true;
            ui.close();
        }
        ui.separator();
        if ui.button("Add media…").clicked() {
            actions.add_media = true;
            ui.close();
        }
        ui.separator();
        if ui.button("Settings…").clicked() {
            state.settings_open = true;
            ui.close();
        }
        if ui.button("Keyboard shortcuts").on_hover_text("?").clicked() {
            state.help = true;
            ui.close();
        }
        if ui.button("Check for updates").clicked() {
            actions.update = Some(crate::update::Request::CheckNow);
            ui.close();
        }
    });
}

/// The coach line under the top bar: what to do now, a shortcut for the first thing to do,
/// and the way on to the next step once this one is done.
pub fn coach(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    state: &mut PanelState,
    monitors: usize,
    actions: &mut Actions,
) {
    let r = editor.readiness();
    let step = state.step;
    egui::Panel::top("lumap-coach")
        .frame(
            egui::Frame::new()
                .fill(crate::theme::INK)
                .inner_margin(egui::Margin::symmetric(12, 6)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Step {} of 4", step.number()))
                        .small()
                        .color(crate::theme::SIGNAL),
                );
                ui.label(RichText::new(step.coach(&r)).weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if step.done(&r) {
                        if let Some(next) = step.next() {
                            if crate::theme::primary_button(ui, &format!("Next: {}", next.label()))
                                .clicked()
                            {
                                state.step = next;
                            }
                        }
                        return;
                    }
                    match step {
                        Step::Setup if monitors > 1 => {
                            if ui.button("One projector per display").clicked() {
                                actions.create_outputs = true;
                            }
                        }
                        Step::Setup => {
                            if ui.button("Add projector").clicked() {
                                editor.add_output();
                            }
                        }
                        Step::Map => {
                            if let Some(first) =
                                editor.project.outputs.first().map(|o| o.id.clone())
                            {
                                if ui.button("Add your first surface").clicked() {
                                    editor.add_surface(&first);
                                }
                            }
                        }
                        Step::Content => {
                            if ui.button("Add your media…").clicked() {
                                actions.add_media = true;
                            }
                        }
                        Step::Show => {}
                    }
                });
            });
        });
}

/// The toolbar above the canvas: on Setup's stage only the stage toggle applies; everywhere
/// else, the Map toolbar (projector tabs, surface tools, canvas toggles).
pub fn canvas_toolbar(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    canvas: &mut CanvasState,
    step: Step,
) {
    if step == Step::Setup && canvas.stage {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.toggle_value(&mut canvas.stage, "Stage view")
                .on_hover_text("Every placed projector on your desktop (T)");
        });
        return;
    }
    super::steps::map::toolbar(ui, editor, canvas, step);
}

/// A message in the corner.
#[derive(Debug, Clone)]
pub struct Toast {
    /// What it says.
    pub text: String,
    /// When it appeared.
    pub born: Instant,
}

/// How long an information toast stays.
pub const TOAST_LIFE: Duration = Duration::from_secs(4);

/// Turns status changes into toasts, drops expired ones, and draws them with the error (which
/// stays until dismissed) at the bottom.
pub fn toasts(ctx: &egui::Context, editor: &Editor, state: &mut PanelState, now: Instant) {
    if !state.status_seen {
        state.last_status.clone_from(&editor.status);
        state.status_seen = true;
    } else if editor.status != state.last_status {
        state.last_status.clone_from(&editor.status);
        if !editor.status.is_empty() {
            state.toasts.push(Toast {
                text: editor.status.clone(),
                born: now,
            });
        }
    }
    state
        .toasts
        .retain(|t| now.duration_since(t.born) < TOAST_LIFE);
    let excess = state.toasts.len().saturating_sub(3);
    state.toasts.drain(..excess);
    if state.toasts.is_empty() && state.error.is_none() {
        return;
    }
    // Information toasts never take a click: whatever is under them stays usable. The error
    // toast has a Dismiss button, so it alone is interactive.
    egui::Area::new(egui::Id::unique("lumap-toasts"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0))
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_max_width(380.0);
            for t in &state.toasts {
                crate::theme::toast_frame(false).show(ui, |ui| ui.label(&t.text));
                ui.add_space(6.0);
            }
        });
    if let Some(e) = state.error.clone() {
        egui::Area::new(egui::Id::unique("lumap-error-toast"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 96.0))
            .show(ctx, |ui| {
                ui.set_max_width(480.0);
                crate::theme::toast_frame(true).show(ui, |ui| {
                    ui.colored_label(crate::theme::LIVE, &e);
                    if ui.button("Dismiss").clicked() {
                        state.error = None;
                    }
                });
            });
    }
    if !state.toasts.is_empty() {
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_number_name_and_chain() {
        assert_eq!(Step::ALL.map(Step::number), [1, 2, 3, 4]);
        assert_eq!(
            Step::ALL.map(Step::label),
            ["Setup", "Map", "Content", "Show"]
        );
        assert_eq!(Step::Setup.next(), Some(Step::Map));
        assert_eq!(Step::Content.next(), Some(Step::Show));
        assert_eq!(Step::Show.next(), None);
        assert_eq!(Step::default(), Step::Setup);
    }

    #[test]
    fn the_first_open_step_follows_readiness_and_every_step_coaches_both_ways() {
        let none = Readiness::default();
        assert_eq!(Step::first_open(&none), Step::Setup);
        let mapped = Readiness {
            projectors: 1,
            surfaces: 2,
            surfaces_with_content: 1,
            ..Readiness::default()
        };
        assert_eq!(Step::first_open(&mapped), Step::Content);
        let all = Readiness {
            projectors: 1,
            projectors_open: 1,
            surfaces: 1,
            surfaces_with_content: 1,
            cues: 1,
        };
        assert_eq!(Step::first_open(&all), Step::Show);
        for step in Step::ALL {
            assert!(!step.done(&none));
            assert!(step.done(&all));
            assert_ne!(step.coach(&none), step.coach(&all), "{step:?}");
        }
    }
}
