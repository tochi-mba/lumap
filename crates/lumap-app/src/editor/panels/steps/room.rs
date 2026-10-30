//! The content panel's controls for camera content: a room effect (which one, tint, speed),
//! paint (light or hand, brush, colour, width, fade, clear) and the live camera.

use super::super::super::{camera::CameraHub, Editor};
use super::super::PanelState;
use eframe::egui::{self, RichText};
use lumap_core::project::{Brush, PaintInput, RoomEffect, SourceKind};

/// Controls for a `Room`, `Paint` or `Camera` source; nothing for other kinds.
#[allow(clippy::too_many_lines)]
pub fn controls(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    id: &str,
    kind: &SourceKind,
    camera: &mut CameraHub,
    state: &mut PanelState,
) {
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
                        "Draw on the wall with a phone torch, a laser pointer or a glow stick."
                    }
                    PaintInput::Hand => {
                        "Pinch thumb and finger to draw, open your hand to stop, make a fist to clear."
                    }
                })
                .weak()
                .small(),
            );
            if i == PaintInput::Hand {
                drawing_camera(ui, editor, camera, state);
            }
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

/// Desk drawing: choose a camera independently of the wall-scan camera and show a mirrored
/// selfie preview with the tracked fingertips. Leaving this inspector releases it immediately.
fn drawing_camera(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    camera: &mut CameraHub,
    state: &mut PanelState,
) {
    ui.separator();
    crate::theme::section_header(ui, "Draw at this computer");
    let names: Vec<String> = camera.devices().iter().map(|d| d.name.clone()).collect();
    if names.is_empty() {
        ui.colored_label(crate::theme::LIVE, "No webcam found.");
        return;
    }
    let wall = editor.project.camera.device.clone();
    let chosen = editor.project.camera.draw_device.clone();
    let device = if chosen.is_empty() {
        if wall.is_empty() {
            names[0].clone()
        } else {
            wall
        }
    } else {
        chosen.clone()
    };
    egui::ComboBox::from_label("Drawing webcam")
        .selected_text(&device)
        .show_ui(ui, |ui| {
            for name in &names {
                if ui.selectable_label(*name == device, name).clicked() {
                    let name = name.clone();
                    editor.edit(move |p| p.camera.draw_device = name);
                }
            }
        });
    ui.label(
        RichText::new(
            "The scan stays calibrated to the wall camera. Pinch in this mirrored preview to draw; open your hand to lift the brush.",
        )
        .weak()
        .small(),
    );
    let now = std::time::Instant::now();
    let Some(stream) = camera.ensure(&device, now) else {
        ui.colored_label(crate::theme::LIVE, "The drawing webcam did not start.");
        return;
    };
    let t = ui.input(|input| input.time);
    if state
        .camera_tex
        .as_ref()
        .is_none_or(|(at, _)| t - *at >= 0.08)
    {
        if let Some(frame) = stream.latest() {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [frame.width as usize, frame.height as usize],
                &frame.rgba,
            );
            let texture = ui.ctx().load_texture(
                "drawing-camera-preview",
                image,
                egui::TextureOptions::LINEAR,
            );
            state.camera_tex = Some((t, texture));
        }
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(80));
    let Some((_, texture)) = &state.camera_tex else {
        ui.label(RichText::new("Starting the drawing webcam…").weak());
        return;
    };
    let size = texture.size_vec2();
    let width = ui.available_width().min(300.0);
    let height = width * size.y / size.x.max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(1.0, 0.0), egui::pos2(0.0, 1.0)),
        egui::Color32::WHITE,
    );
    if let Some(seen) = &state.seen {
        let (cw, ch) = (f64::from(seen.size.0.max(1)), f64::from(seen.size.1.max(1)));
        for hand in &seen.hands {
            let p = lumap_vision::gestures::pinch_point(hand);
            let at = egui::pos2(
                rect.right() - (p.x / cw) as f32 * rect.width(),
                rect.top() + (p.y / ch) as f32 * rect.height(),
            );
            ui.painter().circle_filled(at, 7.0, crate::theme::SIGNAL);
            ui.painter()
                .circle_stroke(at, 10.0, egui::Stroke::new(2.0, egui::Color32::WHITE));
        }
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
