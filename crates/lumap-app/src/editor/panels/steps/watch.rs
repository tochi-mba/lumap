//! Show's Camera card: zones drawn on the camera picture that report motion or presence, hand
//! gestures, and Camera learn, which ties a zone or gesture to any parameter the way MIDI learn
//! does (GO a cue when someone walks up the path; blackout while a hand is held open).

use super::super::super::Editor;
use super::super::PanelState;
use eframe::egui::{self, RichText, Stroke};
use lumap_core::cammap::CamKey;
use lumap_core::geom::Vec2;
use lumap_core::midimap::MidiMode;
use lumap_core::project::Zone;

/// The Camera card.
#[allow(clippy::too_many_lines)]
pub fn card(ui: &mut egui::Ui, editor: &mut Editor, state: &mut PanelState) {
    let zones = editor.project.camera.zones.clone();
    state.camera_meters = !zones.is_empty();
    ui.label(
        RichText::new(
            "Draw zones on the camera picture; motion in a zone, or a hand gesture, can GO a cue \
             or move any fader.",
        )
        .weak()
        .small(),
    );
    picture(ui, editor, state, &zones);
    let readings = state.seen.as_ref().map(|s| s.zones.clone()).unwrap_or_default();
    for z in &zones {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&z.id).family(crate::theme::strong_family()));
            let v = readings
                .iter()
                .find(|(id, _)| *id == z.id)
                .map_or(0.0, |(_, v)| *v);
            ui.add(
                egui::ProgressBar::new(v as f32)
                    .desired_width(80.0)
                    .desired_height(8.0)
                    .fill(crate::theme::SIGNAL),
            );
            let mut sens = z.sensitivity;
            let label = ui.label("Sensitivity");
            if ui
                .add(egui::DragValue::new(&mut sens).range(0.2..=5.0).speed(0.05))
                .labelled_by(label.id)
                .drag_stopped()
            {
                let id = z.id.clone();
                editor.edit(move |p| {
                    if let Some(zone) = p.camera.zones.iter_mut().find(|q| q.id == id) {
                        zone.sensitivity = sens;
                    }
                });
            }
            if ui.small_button(format!("Delete zone {}", z.id)).clicked() {
                let id = z.id.clone();
                editor.edit(move |p| {
                    p.camera.zones.retain(|q| q.id != id);
                    p.camera.map.forget_zone(&id);
                });
            }
        });
    }
    if ui.button("Add a zone").on_hover_text("Or drag across the camera picture").clicked() {
        add_zone(editor, Vec2::new(0.05, 0.55), Vec2::new(0.35, 0.95));
    }
    if let Some(seen) = &state.seen {
        if !seen.gestures.is_empty() {
            crate::theme::section_header(ui, "Gestures");
            for g in &seen.gestures {
                ui.horizontal(|ui| {
                    ui.label(g.gesture.label());
                    ui.add(
                        egui::ProgressBar::new(g.value as f32)
                            .desired_width(80.0)
                            .desired_height(8.0),
                    );
                });
            }
        }
    }
    crate::theme::section_header(ui, "Camera learn");
    ui.horizontal(|ui| {
        for (what, mode) in [
            ("Learn as fader", MidiMode::Fader),
            ("Learn as button", MidiMode::Button),
            ("Learn as switch", MidiMode::Switch),
        ] {
            if ui
                .button(what)
                .on_hover_text("Drives the address in the MIDI card's Learn address box")
                .clicked()
            {
                editor.cam_learn = Some((state.midi_address.trim().to_string(), mode));
            }
        }
    });
    if let Some((address, _)) = editor.cam_learn.clone() {
        ui.horizontal(|ui| {
            crate::theme::pill(ui, "learning", true);
            ui.label(format!("move in a zone or make a gesture for {address}"));
            if ui.button("Cancel camera learn").clicked() {
                editor.cam_learn = None;
            }
        });
    }
    let mappings = editor.project.camera.map.mappings.clone();
    for m in mappings {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} → {}", m.key.label(), m.address)).small());
            let what = match &m.key {
                CamKey::Zone { zone } => zone.clone(),
                CamKey::Gesture { gesture } => gesture.label().to_string(),
            };
            if ui.small_button(format!("Forget {what} {}", m.address)).clicked() {
                let key = m.key.clone();
                let address = m.address.clone();
                editor.edit(move |p| {
                    p.camera
                        .map
                        .mappings
                        .retain(|x| !(x.key == key && x.address == address));
                });
            }
        });
    }
}

/// The camera picture with the zones on it; dragging across it draws a new zone.
fn picture(ui: &mut egui::Ui, editor: &mut Editor, state: &mut PanelState, zones: &[Zone]) {
    state.want_camera = true;
    let Some(frame) = state.camera_frame.clone() else {
        ui.label(RichText::new("Waiting for the camera…").weak().small());
        return;
    };
    let t = ui.input(|i| i.time);
    if state.camera_tex.as_ref().is_none_or(|(at, _)| t - *at >= 0.1) {
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [frame.width as usize, frame.height as usize],
            &frame.rgba,
        );
        let tex = ui
            .ctx()
            .load_texture("camera-preview", image, egui::TextureOptions::LINEAR);
        state.camera_tex = Some((t, tex));
    }
    let Some((_, tex)) = &state.camera_tex else { return };
    let w = ui.available_width().min(300.0);
    let h = w * frame.height as f32 / frame.width.max(1) as f32;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::drag());
    ui.painter().image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Camera picture with zones"));
    let to_screen = |p: Vec2| egui::pos2(rect.min.x + p.x as f32 * w, rect.min.y + p.y as f32 * h);
    let to_norm = |p: egui::Pos2| {
        Vec2::new(
            f64::from(((p.x - rect.min.x) / w).clamp(0.0, 1.0)),
            f64::from(((p.y - rect.min.y) / h).clamp(0.0, 1.0)),
        )
    };
    for z in zones {
        let pts: Vec<egui::Pos2> = z.points.iter().map(|p| to_screen(*p)).collect();
        ui.painter().add(egui::Shape::closed_line(
            pts.clone(),
            Stroke::new(2.0, crate::theme::SIGNAL),
        ));
        if let Some(first) = pts.first() {
            ui.painter().text(
                *first + egui::vec2(3.0, 2.0),
                egui::Align2::LEFT_TOP,
                &z.id,
                egui::FontId::proportional(11.0),
                crate::theme::SIGNAL,
            );
        }
    }
    if response.drag_started() {
        state.zone_drag = response.interact_pointer_pos();
    }
    if let (Some(start), Some(now)) = (state.zone_drag, response.interact_pointer_pos()) {
        ui.painter().rect_stroke(
            egui::Rect::from_two_pos(start, now),
            0.0,
            Stroke::new(1.5, crate::theme::SIGNAL),
            egui::StrokeKind::Inside,
        );
        if response.drag_stopped() {
            let (a, b) = (to_norm(start), to_norm(now));
            let min = Vec2::new(a.x.min(b.x), a.y.min(b.y));
            let max = Vec2::new(a.x.max(b.x), a.y.max(b.y));
            if (max.x - min.x) > 0.02 && (max.y - min.y) > 0.02 {
                add_zone(editor, min, max);
            }
            state.zone_drag = None;
        }
    }
}

/// Adds a rectangle zone with a fresh id ("zone", "zone-2"…), as one undo step.
fn add_zone(editor: &mut Editor, min: Vec2, max: Vec2) {
    let taken: Vec<String> = editor.project.camera.zones.iter().map(|z| z.id.clone()).collect();
    let id = (1..)
        .map(|i| if i == 1 { "zone".to_string() } else { format!("zone-{i}") })
        .find(|c| !taken.contains(c))
        .unwrap_or_else(|| "zone".into());
    let zone = Zone::rect(&id, min, max);
    editor.edit(move |p| p.camera.zones.push(zone));
}
