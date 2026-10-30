//! The inspector: outputs, surfaces, sources, the show.

use super::super::{Editor, MonitorInfo, Selection};
use super::shell::Step;
use super::{describe, rename_box, Actions, PanelState};
use eframe::egui::{self, Color32, RichText};
use lumap_core::project::{GeneratorKind, SourceKind};

/// Right-hand inspector for the selection.
pub fn inspector(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    monitors: &[MonitorInfo],
    state: &mut PanelState,
    actions: &mut Actions,
) {
    egui::Panel::right("lumap-right")
        .default_size(310.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match editor.selection.clone() {
                Selection::None => step_inspector(ui, editor, state, actions),
                Selection::Output(id) => {
                    super::steps::setup::projector_inspector(ui, editor, &id, monitors, state);
                }
                Selection::Surface { id, mask } => {
                    super::steps::map::surface_panel(ui, editor, &id, mask, state);
                }
                Selection::Source(id) => source_inspector(ui, editor, &id, state),
            });
        });
}

/// What the inspector shows with nothing selected: the step's own starting points. App
/// settings (updates, ffmpeg) live in the Settings window; the show's name, blackout and master
/// live in the top bar.
fn step_inspector(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    state: &mut PanelState,
    actions: &mut Actions,
) {
    match state.step {
        Step::Setup => {
            ui.heading("Projectors");
            crate::theme::empty_state(
                ui,
                "Select a projector",
                "Its picture, edge blending and calibration appear here.",
            );
            ui.horizontal_wrapped(|ui| {
                if ui.button("Add projector").clicked() {
                    editor.add_output();
                }
                if ui
                    .button("Suggest blends")
                    .on_hover_text("Set blend widths from where placed projectors overlap")
                    .clicked()
                {
                    editor.suggest_blends();
                }
            });
        }
        Step::Map => {
            ui.heading("Surfaces");
            crate::theme::empty_state(
                ui,
                "Select a surface",
                "Click one on the canvas or in the list, then drag its corners onto the wall.",
            );
            if let Some(first) = editor.project.outputs.first().map(|o| o.id.clone()) {
                if ui.button("Add surface").clicked() {
                    editor.add_surface(&first);
                }
            }
        }
        Step::Content => {
            ui.heading("Content");
            crate::theme::empty_state(
                ui,
                "Select a surface",
                "Its content, look, opacity and blend appear here. The library on the left has \
                 everything you can put on it.",
            );
        }
        Step::Show => super::steps::controllers::controllers(ui, editor, state, actions),
    }
}

/// Inspector for a source: preview, transport, generator and file settings.
#[allow(clippy::too_many_lines)]
fn source_inspector(ui: &mut egui::Ui, editor: &mut Editor, id: &str, state: &mut PanelState) {
    ui.heading(format!("Content {id}"));
    if rename_box(ui, editor, state, id) {
        return;
    }
    let Some(source) = editor.project.source(id).cloned() else {
        return;
    };
    ui.label(RichText::new(describe(&source.kind)).weak());
    if let Some(frame) = editor.sources.frame(id).cloned() {
        let (tex, size) = state.thumbs.get(ui.ctx(), id, &frame);
        let w = ui.available_width().min(280.0);
        let h = w * size.1 as f32 / size.0.max(1) as f32;
        ui.add(egui::Image::new((tex, egui::Vec2::new(w, h))));
        ui.label(
            RichText::new(format!("{} × {} px", frame.width, frame.height))
                .weak()
                .small(),
        );
    }
    if let Some(pb) = source.kind.playback() {
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button(if pb.paused { "Play" } else { "Pause" })
                .clicked()
            {
                editor.edit_source(id, |k| {
                    if let Some(p) = k.playback_mut() {
                        p.paused = !p.paused;
                    }
                });
            }
            if ui.button("Restart").clicked() {
                editor.edit_source(id, |k| {
                    if let Some(p) = k.playback_mut() {
                        p.restart_token = p.restart_token.wrapping_add(1);
                    }
                });
                editor.sources.sync(&editor.project);
            }
            let mut looping = pb.looping;
            if ui.checkbox(&mut looping, "Loop").changed() {
                editor.edit_source(id, |k| {
                    if let Some(p) = k.playback_mut() {
                        p.looping = looping;
                    }
                });
            }
        });
        let mut speed = pb.speed;
        let r = ui.add(egui::Slider::new(&mut speed, 0.0..=4.0).text("Speed"));
        if r.drag_stopped() || r.lost_focus() {
            editor.edit_source(id, |k| {
                if let Some(p) = k.playback_mut() {
                    p.speed = speed;
                }
            });
        }
        ui.label(
            RichText::new(format!("OSC: /source/{id}/play|loop|speed|restart"))
                .weak()
                .small(),
        );
    }
    match &source.kind {
        SourceKind::Generator {
            pattern,
            speed,
            color,
        } => {
            ui.separator();
            let mut chosen = *pattern;
            egui::ComboBox::from_label("Pattern")
                .selected_text(chosen.label())
                .show_ui(ui, |ui| {
                    for g in GeneratorKind::ALL {
                        ui.selectable_value(&mut chosen, g, g.label());
                    }
                });
            if chosen != *pattern {
                editor.edit_source(id, |k| {
                    if let SourceKind::Generator { pattern, .. } = k {
                        *pattern = chosen;
                    }
                });
            }
            let mut sp = *speed;
            let r = ui.add(egui::Slider::new(&mut sp, 0.0..=4.0).text("Animation speed"));
            if r.drag_stopped() || r.lost_focus() {
                editor.edit_source(id, |k| {
                    if let SourceKind::Generator { speed, .. } = k {
                        *speed = sp;
                    }
                });
            }
            let mut c = *color;
            ui.horizontal(|ui| {
                ui.label("Tint");
                if ui.color_edit_button_rgba_unmultiplied(&mut c).changed()
                    && c.iter()
                        .zip(color.iter())
                        .any(|(a, b)| (a - b).abs() > f32::EPSILON)
                {
                    editor.edit_source(id, |k| {
                        if let SourceKind::Generator { color, .. } = k {
                            *color = c;
                        }
                    });
                }
            });
        }
        SourceKind::Sequence { fps, path, .. } => {
            ui.separator();
            ui.label(format!("Path: {path}"));
            if !path.to_ascii_lowercase().ends_with(".gif") {
                let mut f = *fps;
                let r = ui.add(egui::Slider::new(&mut f, 1.0..=120.0).text("Frames per second"));
                if r.drag_stopped() || r.lost_focus() {
                    editor.edit_source(id, |k| {
                        if let SourceKind::Sequence { fps, .. } = k {
                            *fps = f;
                        }
                    });
                }
            }
        }
        SourceKind::Video { path, .. } | SourceKind::Image { path } => {
            ui.separator();
            ui.label(format!("Path: {path}"));
            if matches!(source.kind, SourceKind::Video { .. }) && !state.ffmpeg_ok {
                ui.colored_label(
                    crate::theme::LIVE,
                    "ffmpeg not found: this video cannot play. Set the path in the Show panel.",
                );
            }
        }
        SourceKind::Solid { color } => {
            ui.separator();
            let mut c = *color;
            ui.horizontal(|ui| {
                ui.label("Colour");
                if ui.color_edit_button_rgba_unmultiplied(&mut c).changed()
                    && c.iter()
                        .zip(color.iter())
                        .any(|(a, b)| (a - b).abs() > f32::EPSILON)
                {
                    editor.edit_source(id, |k| {
                        if let SourceKind::Solid { color } = k {
                            *color = c;
                        }
                    });
                    editor.reload_sources();
                }
            });
        }
        SourceKind::TestPattern { .. }
        | SourceKind::Spout { .. }
        | SourceKind::Camera { .. }
        | SourceKind::Room { .. }
        | SourceKind::Paint { .. } => {}
    }
    ui.separator();
    let used = editor
        .project
        .surfaces
        .iter()
        .filter(|s| s.source == *id)
        .count();
    ui.label(RichText::new(format!("used by {used} surface(s)")).weak());
    if ui
        .add_enabled(used == 0, egui::Button::new("Remove from the show"))
        .clicked()
    {
        editor.selection = Selection::Source(id.to_string());
        editor.delete_selection();
    }
}

/// Source preview with a draggable crop box.
pub(in crate::editor::panels) fn crop_preview(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    state: &mut PanelState,
    id: &str,
    surface: &lumap_core::project::Surface,
) {
    let Some(frame) = editor.sources.frame(&surface.source).cloned() else {
        ui.label(RichText::new("(no frame yet)").weak());
        return;
    };
    let (tex, size) = state.thumbs.get(ui.ctx(), &surface.source, &frame);
    // A margin on the right keeps the corner handles clear of the panel's scrollbar.
    let w = (ui.available_width() - 16.0).clamp(80.0, 280.0);
    let h = (w * size.1 as f32 / size.0.max(1) as f32).max(40.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::Vec2::new(w, h), egui::Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Crop preview"));
    ui.painter().image(
        tex,
        rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
    let c = surface.crop;
    let to_screen = |x: f64, y: f64| {
        egui::Pos2::new(
            rect.min.x + x as f32 * rect.width(),
            rect.min.y + y as f32 * rect.height(),
        )
    };
    let box_rect = egui::Rect::from_min_max(to_screen(c.x, c.y), to_screen(c.x + c.w, c.y + c.h));
    ui.painter().rect_stroke(
        box_rect,
        0.0,
        egui::Stroke::new(1.5, crate::theme::SIGNAL),
        egui::StrokeKind::Outside,
    );
    for p in [box_rect.min, box_rect.max] {
        ui.painter().circle_filled(p, 5.0, crate::theme::SIGNAL);
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            let nx = f64::from(((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0));
            let ny = f64::from(((pos.y - rect.min.y) / rect.height()).clamp(0.0, 1.0));
            let mut crop = c;
            if origin.distance(box_rect.min) <= 12.0 {
                let (x1, y1) = (c.x + c.w, c.y + c.h);
                crop.x = nx.min(x1 - 0.01);
                crop.y = ny.min(y1 - 0.01);
                crop.w = x1 - crop.x;
                crop.h = y1 - crop.y;
            } else if origin.distance(box_rect.max) <= 12.0 {
                crop.w = (nx - c.x).max(0.01);
                crop.h = (ny - c.y).max(0.01);
            } else {
                let d = response.drag_delta();
                crop.x = (c.x + f64::from(d.x / rect.width())).clamp(0.0, 1.0 - c.w);
                crop.y = (c.y + f64::from(d.y / rect.height())).clamp(0.0, 1.0 - c.h);
            }
            editor.set_crop(id, crop);
        }
    }
    if response.drag_stopped() {
        editor.commit();
    }
    if response.double_clicked() {
        editor.set_crop(id, lumap_core::project::Crop::default());
        editor.commit();
    }
    ui.label(
        RichText::new(format!(
            "x {:.3}  y {:.3}  w {:.3}  h {:.3}  (double-click to reset)",
            c.x, c.y, c.w, c.h
        ))
        .weak()
        .small(),
    );
}
