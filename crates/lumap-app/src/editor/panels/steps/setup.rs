//! Step 1, Setup: the projectors. Cards on the left say at a glance where each projector is and
//! what it shows; the inspector on the right sets one up: display, window, picture, edge
//! blending, calibration, and the precise numbers tucked away until wanted.

use super::super::super::{monitor_at, Editor, MonitorInfo, Selection};
use super::super::{param_slider, param_toggle, rename_box, Actions, PanelState};
use eframe::egui::{self, RichText, Sense, Stroke};
use lumap_core::blend::BlendCurve;
use lumap_core::project::{Output, Overlay, Placement};
use std::time::Instant;

/// A display's name as people say it: "Display 2 · 1280×720" for `\\.\DISPLAY2`, the raw name
/// when it has no number.
pub fn display_label(m: &MonitorInfo) -> String {
    let digits: String = m
        .name
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let name = if digits.is_empty() {
        m.name.clone()
    } else {
        format!("Display {digits}")
    };
    format!("{name} · {}×{}", m.width, m.height)
}

/// Where a projector is, in words: its display, a custom position, or a floating window.
pub fn placement_label(output: &Output, monitors: &[MonitorInfo]) -> String {
    match &output.placement {
        None => "Floating window".into(),
        Some(p) if p.monitor.is_empty() => format!("Custom position {}, {}", p.x, p.y),
        Some(p) => monitors
            .iter()
            .find(|m| m.name == p.monitor)
            .map_or_else(|| format!("{} (not connected)", p.monitor), display_label),
    }
}

/// The left panel in Setup: one card per projector.
pub fn projector_list(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    monitors: &[MonitorInfo],
    camera: &mut crate::editor::camera::CameraHub,
    state: &mut super::super::PanelState,
    actions: &mut Actions,
) {
    egui::Panel::left("lumap-left")
        .default_size(260.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Projectors");
                    if ui
                        .small_button("+ projector")
                        .on_hover_text("Add a projector")
                        .clicked()
                    {
                        editor.add_output();
                    }
                });
                ui.add_space(4.0);
                if editor.project.outputs.is_empty() {
                    crate::theme::empty_state(
                        ui,
                        "No projectors yet",
                        "Plug them in and create one per display, or add one by hand.",
                    );
                }
                let outputs = editor.project.outputs.clone();
                for (index, output) in outputs.iter().enumerate() {
                    card(ui, editor, monitors, index, output);
                    ui.add_space(6.0);
                }
                if monitors.len() > 1 && ui.button("One projector per display").clicked() {
                    actions.create_outputs = true;
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Click a projector to set it up; drag it on the stage to move it.",
                    )
                    .weak()
                    .small(),
                );
                super::camera::card(ui, editor, camera, state, actions);
            });
        });
}

/// One projector's card: colour, name, resolution, display, and what is on.
fn card(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    monitors: &[MonitorInfo],
    index: usize,
    output: &Output,
) {
    let selected = editor.selection == Selection::Output(output.id.clone());
    let open = editor.open_outputs.contains(&output.id);
    egui::Frame::new()
        .fill(if selected {
            crate::theme::RAISED
        } else {
            crate::theme::PANEL
        })
        .stroke(Stroke::new(
            1.0,
            if selected {
                crate::theme::SIGNAL
            } else {
                crate::theme::LINE
            },
        ))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                ui.painter().circle_filled(
                    dot.center(),
                    4.5,
                    super::super::super::handles::output_color(index),
                );
                if ui
                    .selectable_label(
                        selected,
                        RichText::new(&output.id).family(crate::theme::strong_family()),
                    )
                    .clicked()
                {
                    editor.selection = Selection::Output(output.id.clone());
                    editor.extra_selected.clear();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{}×{}", output.width, output.height))
                            .weak()
                            .small(),
                    );
                });
            });
            ui.label(
                RichText::new(placement_label(output, monitors))
                    .weak()
                    .small(),
            );
            ui.horizontal_wrapped(|ui| {
                if open {
                    crate::theme::pill(ui, "window open", false);
                }
                if let Some(o) = output.overlay {
                    crate::theme::pill(ui, o.label(), false);
                }
                if !output.enabled {
                    crate::theme::pill(ui, "off in show", true);
                }
            });
        });
}

/// The inspector for one projector.
#[allow(clippy::too_many_lines)]
pub fn projector_inspector(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    id: &str,
    monitors: &[MonitorInfo],
    state: &mut PanelState,
) {
    ui.horizontal(|ui| {
        ui.heading(format!("Projector {id}"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button("Identify on the projector")
                .on_hover_text("Flash every projector's name on its wall (I)")
                .clicked()
            {
                editor.identify(Instant::now());
            }
        });
    });
    if rename_box(ui, editor, state, id) {
        return;
    }
    let Some(output) = editor.project.output(id).cloned() else {
        return;
    };

    crate::theme::section_header(ui, "Placement");
    let current = placement_label(&output, monitors);
    let mut choice: Option<Option<&MonitorInfo>> = None;
    egui::ComboBox::from_label("Display")
        .selected_text(&current)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(output.placement.is_none(), "Floating window")
                .clicked()
            {
                choice = Some(None);
            }
            for m in monitors {
                let on = output
                    .placement
                    .as_ref()
                    .is_some_and(|p| p.monitor == m.name);
                if ui.selectable_label(on, display_label(m)).clicked() {
                    choice = Some(Some(m));
                }
            }
        });
    if let Some(target) = choice {
        editor.place_output(id, target);
    }
    ui.horizontal(|ui| {
        let open = editor.open_outputs.contains(&id.to_string());
        if ui
            .button(if open { "Close window" } else { "Open window" })
            .clicked()
        {
            editor.toggle_output_window(id);
        }
        param_toggle(
            ui,
            editor,
            "On in show mode",
            &format!("/output/{id}/enabled"),
        );
    });

    crate::theme::section_header(ui, "Picture");
    for (label, key, range) in [
        ("Brightness", "brightness", 0.0..=2.0),
        ("Contrast", "contrast", 0.0..=2.0),
        ("Gamma", "gamma", 0.5..=2.5),
    ] {
        param_slider(ui, editor, label, &format!("/output/{id}/{key}"), range);
    }

    crate::theme::section_header(ui, "Edge blending");
    if ui
        .button("Suggest blends")
        .on_hover_text("Set blend widths from where placed projectors overlap")
        .clicked()
    {
        editor.suggest_blends();
    }
    for (label, edge) in [
        ("Left", "left"),
        ("Right", "right"),
        ("Top", "top"),
        ("Bottom", "bottom"),
    ] {
        ui.horizontal(|ui| {
            param_slider(
                ui,
                editor,
                label,
                &format!("/output/{id}/blend/{edge}/width"),
                0.0..=0.5,
            );
            let now = match edge {
                "left" => output.blend.left.curve,
                "right" => output.blend.right.curve,
                "top" => output.blend.top.curve,
                _ => output.blend.bottom.curve,
            };
            let mut curve = now;
            egui::ComboBox::from_id_salt(("curve", id, edge))
                .selected_text(curve.label())
                .show_ui(ui, |ui| {
                    for c in BlendCurve::ALL {
                        ui.selectable_value(&mut curve, c, c.label());
                    }
                });
            if curve != now {
                let id = id.to_string();
                editor.edit(move |p| {
                    if let Some(o) = p.output_mut(&id) {
                        match edge {
                            "left" => o.blend.left.curve = curve,
                            "right" => o.blend.right.curve = curve,
                            "top" => o.blend.top.curve = curve,
                            _ => o.blend.bottom.curve = curve,
                        }
                    }
                });
            }
        });
    }
    param_slider(
        ui,
        editor,
        "Blend gamma",
        &format!("/output/{id}/blend/gamma"),
        1.0..=3.0,
    );

    crate::theme::section_header(ui, "Calibration");
    let mut overlay = output.overlay;
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut overlay, None, "Off");
        for k in Overlay::ALL {
            ui.selectable_value(&mut overlay, Some(k), k.label());
        }
    });
    if overlay != output.overlay {
        editor.set_overlay(id, overlay);
    }
    ui.horizontal(|ui| {
        if ui
            .button("Gamma match")
            .on_hover_text("Grey steps on this projector; match them across the seam with [ and ]")
            .clicked()
        {
            editor.set_overlay(id, Some(Overlay::GreySteps));
        }
        ui.label(
            RichText::new("Overlays never show in show mode.")
                .weak()
                .small(),
        );
    });

    ui.add_space(6.0);
    egui::CollapsingHeader::new("Precise")
        .id_salt(("precise-projector", id))
        .show(ui, |ui| precise(ui, editor, state, id, &output, monitors));

    ui.add_space(10.0);
    if crate::theme::danger_button(ui, "Delete projector").clicked() {
        editor.selection = Selection::Output(id.to_string());
        editor.delete_selection();
    }
}

/// Size and position in pixels, for when dragging is not exact enough.
fn precise(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    state: &mut PanelState,
    id: &str,
    output: &Output,
    monitors: &[MonitorInfo],
) {
    ui.horizontal(|ui| {
        // The size being typed or dragged lives in the panel state until the edit ends, so a
        // drag accumulates and one undo step covers it.
        let (mut w, mut h) = match &state.size_edit {
            Some((sid, w, h)) if sid == id => (*w, *h),
            _ => (output.width, output.height),
        };
        let lw = ui.label("Width");
        let rw = ui
            .add(egui::DragValue::new(&mut w).range(1..=16384).suffix(" px"))
            .labelled_by(lw.id);
        let lh = ui.label("Height");
        let rh = ui
            .add(egui::DragValue::new(&mut h).range(1..=16384).suffix(" px"))
            .labelled_by(lh.id);
        if rw.changed() || rh.changed() {
            state.size_edit = Some((id.to_string(), w, h));
        }
        let done = rw.lost_focus() || rh.lost_focus() || rw.drag_stopped() || rh.drag_stopped();
        if done {
            state.size_edit = None;
        }
        if done && (w, h) != (output.width, output.height) {
            let id = id.to_string();
            editor.edit(move |p| {
                if let Some(o) = p.output_mut(&id) {
                    o.width = w;
                    o.height = h;
                }
            });
        }
    });
    if let Some(p) = &output.placement {
        ui.label(
            RichText::new(format!("Placed at {}, {} on the desktop", p.x, p.y))
                .weak()
                .small(),
        );
    } else {
        ui.label(
            RichText::new("Floating: the window opens where you drag it.")
                .weak()
                .small(),
        );
        if let Some((x, y)) = editor.window_positions.get(id).copied() {
            if ui.button("Use current window position").clicked() {
                let scale = monitor_at(monitors, x as i32, y as i32).map_or(1.0, |m| m.scale);
                let placement = Placement {
                    monitor: String::new(),
                    x: (f64::from(x) * scale).round() as i32,
                    y: (f64::from(y) * scale).round() as i32,
                };
                let id = id.to_string();
                editor.edit(move |p| {
                    if let Some(o) = p.output_mut(&id) {
                        o.placement = Some(placement);
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str) -> MonitorInfo {
        MonitorInfo {
            name: name.into(),
            x: 1920,
            y: 0,
            width: 1280,
            height: 720,
            scale: 1.0,
        }
    }

    #[test]
    fn displays_and_placements_read_in_plain_words() {
        assert_eq!(
            display_label(&monitor(r"\\.\DISPLAY2")),
            "Display 2 · 1280×720"
        );
        assert_eq!(display_label(&monitor("Projector")), "Projector · 1280×720");
        let monitors = [monitor("DISPLAY2")];
        let mut o = lumap_core::project::demo_project().outputs[0].clone();
        o.placement = None;
        assert_eq!(placement_label(&o, &monitors), "Floating window");
        o.placement = Some(Placement {
            monitor: "DISPLAY2".into(),
            x: 1920,
            y: 0,
        });
        assert_eq!(placement_label(&o, &monitors), "Display 2 · 1280×720");
        o.placement = Some(Placement {
            monitor: "DISPLAY9".into(),
            x: 0,
            y: 0,
        });
        assert_eq!(placement_label(&o, &monitors), "DISPLAY9 (not connected)");
        o.placement = Some(Placement {
            monitor: String::new(),
            x: 10,
            y: 20,
        });
        assert_eq!(placement_label(&o, &monitors), "Custom position 10, 20");
    }
}
