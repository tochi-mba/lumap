//! The check before going live: one line per thing a show needs, each ticked or offering the
//! one click that puts it right, and Go live (or Go live anyway) at the bottom.

use super::super::state::Fix;
use super::super::{Editor, MonitorInfo};
use super::shell::Step;
use super::{Actions, PanelState};
use eframe::egui::{self, RichText, Sense, Stroke};

/// The sheet, while `state.golive` is set.
pub fn sheet(
    ctx: &egui::Context,
    editor: &mut Editor,
    state: &mut PanelState,
    monitors: &[MonitorInfo],
    actions: &mut Actions,
) {
    if !state.golive {
        return;
    }
    let checks = editor.preflight(monitors, state.ffmpeg_ok);
    let ready = checks.iter().all(|c| c.ok);
    let mut fix = None;
    let mut go = false;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::unique("lumap-golive"))
        .frame(
            egui::Frame::new()
                .fill(crate::theme::PANEL)
                .stroke(Stroke::new(1.0, crate::theme::LINE))
                .corner_radius(egui::CornerRadius::same(12))
                .inner_margin(egui::Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_width(500.0);
            ui.heading(if ready {
                "Ready to go live"
            } else {
                "Before you go live"
            });
            ui.label(
                RichText::new(
                    "Show mode locks editing and opens every projector. Press Escape twice to \
                     leave it.",
                )
                .weak()
                .small(),
            );
            ui.add_space(10.0);
            // Mark, sentence (wrapping in its own column), fix: the button never covers the text.
            egui::Grid::new("lumap-golive-checks")
                .num_columns(3)
                .spacing(egui::vec2(8.0, 8.0))
                .show(ui, |ui| {
                    for check in &checks {
                        mark(ui, check.ok);
                        ui.allocate_ui(egui::vec2(330.0, 0.0), |ui| {
                            ui.set_width(330.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&check.text).color(crate::theme::TEXT),
                                )
                                .wrap(),
                            );
                        });
                        match check.fix {
                            Some(f) => {
                                if ui.small_button(f.label()).clicked() {
                                    fix = Some(f);
                                }
                            }
                            None => {
                                ui.label("");
                            }
                        }
                        ui.end_row();
                    }
                });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ready {
                    go = crate::theme::primary_button(ui, "Go live now").clicked();
                } else {
                    go = ui
                        .button("Go live anyway")
                        .on_hover_text("Everything above is advice; the show runs either way")
                        .clicked();
                }
                if ui.button("Not yet").clicked() {
                    close = true;
                }
            });
        });
    if let Some(f) = fix {
        apply(f, editor, state, monitors, actions);
    }
    if go {
        state.golive = false;
        editor.enter_show_mode();
    } else if close || modal.should_close() {
        state.golive = false;
    }
}

/// A painted tick (ready) or an open ring (to do), so no glyph font is needed.
fn mark(ui: &mut egui::Ui, ok: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
    let c = rect.center();
    if ok {
        let tick = vec![
            c + egui::vec2(-4.5, 0.0),
            c + egui::vec2(-1.5, 3.0),
            c + egui::vec2(4.5, -3.5),
        ];
        ui.painter()
            .line(tick, Stroke::new(2.0, crate::theme::SIGNAL));
    } else {
        ui.painter()
            .circle_stroke(c, 5.0, Stroke::new(1.5, crate::theme::LIVE));
    }
}

/// Carries out a fix. Steps and Settings close the sheet so the work can be done there; the
/// rest happen in place and the sheet ticks the line.
fn apply(
    fix: Fix,
    editor: &mut Editor,
    state: &mut PanelState,
    monitors: &[MonitorInfo],
    actions: &mut Actions,
) {
    let go_to = |step: Step, state: &mut PanelState| {
        state.step = step;
        state.golive = false;
    };
    match fix {
        Fix::Place => {
            editor.place_on_free_displays(monitors);
        }
        Fix::Setup => go_to(Step::Setup, state),
        Fix::Map => go_to(Step::Map, state),
        Fix::Content => go_to(Step::Content, state),
        Fix::Settings => {
            state.settings_open = true;
            state.golive = false;
        }
        Fix::Save => actions.save = true,
        Fix::StoreCue => {
            editor.store_cue("", state.cue_fade);
        }
    }
}
