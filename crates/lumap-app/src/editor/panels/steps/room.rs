//! The content panel's controls for camera content: a room effect (which one, tint, speed),
//! paint (light or hand, brush, colour, width, fade, clear) and the live camera.

use super::super::super::Editor;
use eframe::egui::{self, RichText};
use lumap_core::project::{Brush, PaintInput, RoomEffect, SourceKind};

/// Controls for a `Room`, `Paint` or `Camera` source; nothing for other kinds.
#[allow(clippy::too_many_lines)]
pub fn controls(ui: &mut egui::Ui, editor: &mut Editor, id: &str, kind: &SourceKind) {
    match kind {
        SourceKind::Room {
            projector,
            effect,
            color,
            speed,
        } => {
            ui.separator();
            scan_note(ui, editor, projector);
            let mut chosen = *effect;
            egui::ComboBox::from_label("Effect")
                .selected_text(chosen.label())
                .show_ui(ui, |ui| {
                    for e in RoomEffect::ALL {
                        ui.selectable_value(&mut chosen, e, e.label());
                    }
                });
            if chosen != *effect {
                editor.edit_source(id, |k| {
                    if let SourceKind::Room { effect, .. } = k {
                        *effect = chosen;
                    }
                });
            }
            ui.label(RichText::new(chosen.blurb()).weak().small());
            if chosen.needs_camera() {
                ui.label(
                    RichText::new("Watches the camera live for people's shadows.")
                        .weak()
                        .small(),
                );
            }
            tint_and_speed(ui, editor, id, *color, *speed);
        }
        SourceKind::Paint {
            projector,
            input,
            brush,
            color,
            size,
            fade_seconds,
        } => {
            ui.separator();
            scan_note(ui, editor, projector);
            let mut i = *input;
            egui::ComboBox::from_label("Draw with")
                .selected_text(i.label())
                .show_ui(ui, |ui| {
                    for v in PaintInput::ALL {
                        ui.selectable_value(&mut i, v, v.label());
                    }
                });
            let mut b = *brush;
            egui::ComboBox::from_label("Brush")
                .selected_text(b.label())
                .show_ui(ui, |ui| {
                    for v in Brush::ALL {
                        ui.selectable_value(&mut b, v, v.label());
                    }
                });
            if i != *input || b != *brush {
                editor.edit_source(id, |k| {
                    if let SourceKind::Paint { input, brush, .. } = k {
                        *input = i;
                        *brush = b;
                    }
                });
            }
            let mut c = *color;
            ui.horizontal(|ui| {
                ui.label("Colour");
                if ui.color_edit_button_rgba_unmultiplied(&mut c).changed() && c != *color {
                    editor.edit_source(id, |k| {
                        if let SourceKind::Paint { color, .. } = k {
                            *color = c;
                        }
                    });
                }
            });
            let mut s = *size;
            let r = ui.add(egui::Slider::new(&mut s, 2.0..=80.0).text("Width (px)"));
            let mut f = *fade_seconds;
            let r2 = ui.add(egui::Slider::new(&mut f, 0.0..=30.0).text("Fade after (s, 0 keeps)"));
            if r.drag_stopped() || r.lost_focus() || r2.drag_stopped() || r2.lost_focus() {
                editor.edit_source(id, |k| {
                    if let SourceKind::Paint {
                        size, fade_seconds, ..
                    } = k
                    {
                        *size = s;
                        *fade_seconds = f;
                    }
                });
            }
            if ui
                .button("Clear the paint")
                .on_hover_text(format!("Also /paint/{id}/clear, or make a fist"))
                .clicked()
                && !super::super::super::room::clear_paint(id)
            {
                editor.status = "The paint is not running yet.".into();
            }
            ui.label(
                RichText::new(match i {
                    PaintInput::Light => {
                        "Draw on the wall with a phone torch, a laser pointer or a glow stick, in a dark room."
                    }
                    PaintInput::Hand => {
                        "Pinch thumb and finger to draw, open your hand to stop, make a fist to clear."
                    }
                })
                .weak()
                .small(),
            );
        }
        SourceKind::Camera { device } => {
            ui.separator();
            ui.label(if device.is_empty() {
                "The show's camera, live.".to_string()
            } else {
                format!("Camera: {device}")
            });
        }
        _ => {}
    }
}

/// Room and paint content need the projector's scan: say so when there is none.
fn scan_note(ui: &mut egui::Ui, editor: &Editor, projector: &str) {
    if editor.scans.get(projector).is_some() {
        ui.label(
            RichText::new(format!("Drawn on {projector}'s wall."))
                .weak()
                .small(),
        );
    } else if editor.scans.loading(projector) {
        ui.label(RichText::new("Loading the scan…").weak().small());
    } else {
        ui.colored_label(
            crate::theme::LIVE,
            format!("{projector} has no scan yet: scan the room in step 1, Setup."),
        );
    }
}

fn tint_and_speed(ui: &mut egui::Ui, editor: &mut Editor, id: &str, color: [f32; 4], speed: f32) {
    let mut c = color;
    ui.horizontal(|ui| {
        ui.label("Tint");
        if ui.color_edit_button_rgba_unmultiplied(&mut c).changed() && c != color {
            editor.edit_source(id, |k| {
                if let SourceKind::Room { color, .. } = k {
                    *color = c;
                }
            });
        }
    });
    let mut sp = speed;
    let r = ui.add(egui::Slider::new(&mut sp, 0.0..=4.0).text("Speed"));
    if r.drag_stopped() || r.lost_focus() {
        editor.edit_source(id, |k| {
            if let SourceKind::Room { speed, .. } = k {
                *speed = sp;
            }
        });
    }
}
