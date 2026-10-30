//! Step 4, Show: the cue list. A big GO on top, the live look stored as a new cue under it, and
//! one card per cue that you click to select, drag by its dots to reorder and right-click for
//! more.

use super::super::super::Editor;
use super::super::{draft_field, PanelState};
use eframe::egui::{self, RichText, Sense, Stroke};

/// What a dragged cue card carries: its index.
#[derive(Debug, Clone, Copy)]
struct CueDrag(usize);

/// A cue's title: its name, or its id when it has none.
fn title(cue: &lumap_core::cues::Cue) -> &str {
    if cue.name.is_empty() {
        &cue.id
    } else {
        &cue.name
    }
}

/// The left panel on Show.
pub fn cue_list(ui: &mut egui::Ui, editor: &mut Editor, state: &mut PanelState, now: f64) {
    egui::Panel::left("lumap-cues")
        .default_size(290.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Cues");
                let n = editor.project.cues.len();
                if n > 0 {
                    ui.label(RichText::new(format!("{n}")).weak());
                }
            });
            go_button(ui, editor, now);
            store_row(ui, editor, state);
            crate::theme::section_header(ui, "Running order");
            let cues = editor.project.cues.clone();
            if cues.is_empty() {
                crate::theme::empty_state(
                    ui,
                    "No cues yet",
                    "Set up a look, then Store this look. Change something, store again. GO \
                     (or Space) fades from one to the next.",
                );
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, cue) in cues.iter().enumerate() {
                    card(ui, editor, state, i, cue, now);
                    ui.add_space(4.0);
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Drag the dots to reorder. Right-click a cue for more.")
                        .weak()
                        .small(),
                );
            });
        });
}

/// GO, as big as the panel is wide, with what it will fade to under it, and the running fade.
fn go_button(ui: &mut egui::Ui, editor: &mut Editor, now: f64) {
    let cues = &editor.project.cues;
    let next = (!cues.is_empty()).then(|| editor.cues.current.map_or(0, |i| (i + 1) % cues.len()));
    let next_text = next.map_or_else(
        || "Store a cue first".to_string(),
        |i| format!("Next: {} · {}", i + 1, title(&cues[i])),
    );
    ui.add_space(6.0);
    let go = egui::Button::new(
        RichText::new("GO")
            .size(28.0)
            .family(crate::theme::strong_family())
            .color(crate::theme::INK),
    )
    .fill(crate::theme::SIGNAL)
    .stroke(Stroke::NONE)
    .corner_radius(egui::CornerRadius::same(10))
    .min_size(egui::vec2(ui.available_width(), 64.0));
    if ui
        .add_enabled(next.is_some(), go)
        .on_hover_text("Fade to the next cue (Space)")
        .clicked()
    {
        editor.cue_go_next(now);
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(next_text).weak().small());
        if let Some(f) = &editor.cues.fade {
            let progress = f.progress(now) as f32;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Stop fade").clicked() {
                    editor.cues.stop();
                }
                ui.add(
                    egui::ProgressBar::new(progress)
                        .desired_width(70.0)
                        .desired_height(8.0)
                        .fill(crate::theme::SIGNAL)
                        .text("fading"),
                );
            });
        }
    });
}

/// Name and fade for the next cue, and the button that stores the live look.
fn store_row(ui: &mut egui::Ui, editor: &mut Editor, state: &mut PanelState) {
    crate::theme::section_header(ui, "New cue");
    ui.horizontal(|ui| {
        let label = ui.label("Cue name");
        ui.add(
            egui::TextEdit::singleline(&mut state.cue_name)
                .desired_width(110.0)
                .hint_text("optional"),
        )
        .labelled_by(label.id);
        let label = ui.label("Fade");
        ui.add(
            egui::DragValue::new(&mut state.cue_fade)
                .range(0.0..=600.0)
                .speed(0.1)
                .suffix(" s"),
        )
        .labelled_by(label.id)
        .on_hover_text("How long GO takes to fade into this cue");
    });
    if ui
        .button("Store this look")
        .on_hover_text("Keep every live setting as a new cue after the selected one")
        .clicked()
    {
        editor.store_cue(&state.cue_name.clone(), state.cue_fade);
        state.cue_name.clear();
    }
}

/// One cue: grip, number, title, fade, its own GO; the selected one opens to edit.
fn card(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    state: &mut PanelState,
    i: usize,
    cue: &lumap_core::cues::Cue,
    now: f64,
) {
    let current = editor.cues.current == Some(i);
    let selected = editor.selected_cue == Some(i);
    let name = title(cue).to_string();
    let frame = egui::Frame::new()
        .fill(if selected {
            crate::theme::RAISED
        } else {
            crate::theme::PANEL
        })
        .stroke(Stroke::new(
            1.0,
            if current {
                crate::theme::SIGNAL
            } else {
                crate::theme::LINE
            },
        ))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                grip(ui, i);
                ui.label(
                    RichText::new(format!("{}", i + 1))
                        .family(crate::theme::strong_family())
                        .color(if current {
                            crate::theme::SIGNAL
                        } else {
                            crate::theme::MUTED
                        }),
                );
                let text = if current {
                    RichText::new(&name).color(crate::theme::SIGNAL)
                } else {
                    RichText::new(&name)
                };
                let r = ui.selectable_label(selected, text).on_hover_text(format!(
                    "Cue {}, id {} (OSC /cue/{}/go)",
                    i + 1,
                    cue.id,
                    cue.id
                ));
                if r.clicked() {
                    editor.selected_cue = Some(i);
                }
                r.context_menu(|ui| menu(ui, editor, i, now));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(format!("GO {name}"))
                        .on_hover_text("Fade to this cue now")
                        .clicked()
                    {
                        editor.cue_go(i, now);
                    }
                    if current {
                        crate::theme::pill(ui, "live", false);
                    }
                    ui.label(
                        RichText::new(format!("{:.1} s", cue.fade_seconds))
                            .weak()
                            .small(),
                    );
                });
            });
            if selected {
                edit_row(ui, editor, state, i, cue);
            }
        });
    // Dropping a card here moves it to this card's place.
    let rect = frame.response.rect;
    let target = ui.interact(rect, ui.scope_id().with(("cue", i)), Sense::hover());
    if let Some(dragged) = target.dnd_hover_payload::<CueDrag>() {
        if dragged.0 != i {
            let y = if dragged.0 < i {
                rect.bottom()
            } else {
                rect.top()
            };
            ui.painter()
                .hline(rect.x_range(), y, Stroke::new(2.0, crate::theme::SIGNAL));
        }
    }
    if let Some(dragged) = target.dnd_release_payload::<CueDrag>() {
        editor.move_cue_to(dragged.0, i);
    }
}

/// The six dots a cue card is dragged by.
fn grip(ui: &mut egui::Ui, i: usize) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(12.0, 20.0), Sense::drag());
    for (dx, dy) in [
        (3.0, 5.0),
        (8.0, 5.0),
        (3.0, 10.0),
        (8.0, 10.0),
        (3.0, 15.0),
        (8.0, 15.0),
    ] {
        ui.painter()
            .circle_filled(rect.min + egui::vec2(dx, dy), 1.3, crate::theme::MUTED);
    }
    response
        .on_hover_cursor(egui::CursorIcon::Grab)
        .dnd_set_drag_payload(CueDrag(i));
}

/// Name, fade and the buttons of the selected cue.
fn edit_row(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    state: &mut PanelState,
    i: usize,
    cue: &lumap_core::cues::Cue,
) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let label = ui.label("Rename");
        let key = format!("cue:{}", cue.id);
        let (r, done) = draft_field(ui, &mut state.draft, &key, &cue.name, 120.0, &cue.id);
        r.labelled_by(label.id);
        if let Some(text) = done {
            editor.rename_cue(i, &text);
        }
        let mut fade = cue.fade_seconds;
        let label = ui.label("Cue fade");
        if ui
            .add(
                egui::DragValue::new(&mut fade)
                    .range(0.0..=600.0)
                    .speed(0.1)
                    .suffix(" s"),
            )
            .labelled_by(label.id)
            .changed()
        {
            editor.edit(move |p| {
                if let Some(c) = p.cues.get_mut(i) {
                    c.fade_seconds = fade;
                }
            });
        }
    });
    ui.horizontal(|ui| {
        if ui
            .button("Update cue")
            .on_hover_text("Overwrite this cue with the live look")
            .clicked()
        {
            editor.update_cue(i);
        }
        if crate::theme::danger_button(ui, "Delete cue").clicked() {
            editor.delete_cue(i);
        }
    });
}

/// Right-click on a cue.
fn menu(ui: &mut egui::Ui, editor: &mut Editor, i: usize, now: f64) {
    if ui.button("GO this cue").clicked() {
        editor.cue_go(i, now);
        ui.close();
    }
    if ui.button("Update with the live look").clicked() {
        editor.update_cue(i);
        ui.close();
    }
    if ui.button("Move up").clicked() {
        editor.move_cue(i, -1);
        ui.close();
    }
    if ui.button("Move down").clicked() {
        editor.move_cue(i, 1);
        ui.close();
    }
    if ui.button("Delete").clicked() {
        editor.delete_cue(i);
        ui.close();
    }
}
