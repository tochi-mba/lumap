//! Step 2, Map: the surfaces. Layers on the left for the projector being mapped, a toolbar
//! with a tab per projector, a small action bar beside the selected surface on the canvas, and
//! a surface panel whose numbers are folded into Precise.

use super::super::super::canvas::CanvasState;
use super::super::super::handles::{handles, Mapping};
use super::super::super::{AlignOp, Editor, Selection};
use super::super::shell::Step;
use super::super::{param_slider, param_toggle, rename_box, PanelState};
use eframe::egui::{self, RichText, Sense, Stroke};
use lumap_core::midimap::MidiMode;
use lumap_core::params::ParamValue;
use lumap_core::project::{BlendMode, Surface};
use lumap_core::warp::{BezierMesh, WarpKind};

/// The projector the canvas shows: the one the selection is on, else the canvas's own choice,
/// else the first.
pub fn current_projector(editor: &Editor, canvas: &CanvasState) -> Option<String> {
    let from_selection = match &editor.selection {
        Selection::Output(id) => Some(id.clone()),
        Selection::Surface { id, .. } => editor.project.surface(id).map(|s| s.output.clone()),
        Selection::None | Selection::Source(_) => None,
    };
    from_selection
        .or_else(|| canvas.output.clone())
        .filter(|o| editor.project.output(o).is_some())
        .or_else(|| editor.project.outputs.first().map(|o| o.id.clone()))
}

/// Plain words for a blend mode.
pub fn blend_label(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => "Normal",
        BlendMode::Add => "Brighten (add)",
        BlendMode::Screen => "Light (screen)",
        BlendMode::Multiply => "Darken (multiply)",
    }
}

/// The toolbar above the canvas on Map, Content and Show: a tab per projector, surface tools,
/// and the canvas toggles.
pub fn toolbar(ui: &mut egui::Ui, editor: &mut Editor, canvas: &mut CanvasState, step: Step) {
    ui.horizontal_wrapped(|ui| {
        ui.add_space(8.0);
        let current = current_projector(editor, canvas);
        let ids: Vec<String> = editor
            .project
            .outputs
            .iter()
            .map(|o| o.id.clone())
            .collect();
        // One tab per projector: the canvas shows the one picked here. Setup has its cards.
        for id in ids.iter().filter(|_| step != Step::Setup) {
            if ui
                .selectable_label(current.as_deref() == Some(id), id)
                .on_hover_text("Show this projector on the canvas")
                .clicked()
            {
                canvas.output = Some(id.clone());
                editor.selection = Selection::Output(id.clone());
                editor.extra_selected.clear();
            }
        }
        if step == Step::Map {
            ui.separator();
            if let Some(out) = current.clone() {
                if ui.button("Add surface").clicked() {
                    editor.add_surface(&out);
                }
            }
            let selected = match &editor.selection {
                Selection::Surface { id, .. } => Some(id.clone()),
                _ => None,
            };
            if ui
                .add_enabled(selected.is_some(), egui::Button::new("Add cut-out"))
                .on_hover_text("A shape that hides (or alone shows) part of the selected surface")
                .clicked()
            {
                if let Some(id) = selected {
                    editor.add_mask(&id);
                }
            }
            ui.separator();
        }
        if let Some(scan) = current.as_deref().and_then(|o| editor.scans.get(o)) {
            ui.toggle_value(&mut canvas.wall_photo, "Wall photo")
                .on_hover_text("What this projector sees, from the camera scan");
            if canvas.wall_photo {
                ui.add(
                    egui::Slider::new(&mut canvas.wall_opacity, 0.1..=1.0)
                        .show_value(false)
                        .text("Photo"),
                );
            }
            ui.toggle_value(
                &mut canvas.show_found,
                format!("Find surfaces ({})", scan.found.len()),
            )
            .on_hover_text("Surfaces the scan found; click one to use it");
            let picking = canvas.tool == super::super::super::canvas::Tool::Pick;
            if ui
                .selectable_label(picking, "Pick an object")
                .on_hover_text("Click an object on the wall photo to outline it")
                .clicked()
            {
                canvas.tool = if picking {
                    super::super::super::canvas::Tool::Edit
                } else {
                    super::super::super::canvas::Tool::Pick
                };
                canvas.picked = None;
            }
            ui.toggle_value(&mut canvas.hands_map, "Hands")
                .on_hover_text(
                    "Pinch near a corner on the wall to grab it (needs Edit on the wall)",
                );
            ui.separator();
        }
        ui.toggle_value(&mut canvas.show_grid, "Grid")
            .on_hover_text("An alignment grid on the canvas (G)");
        ui.toggle_value(&mut canvas.handles.snap, "Snap")
            .on_hover_text("Snap corners to the grid, edges and centre (S)");
        ui.toggle_value(&mut editor.edit_on_wall, "Edit on the wall")
            .on_hover_text("Show the handles on the projectors too, and drag them there");
        ui.toggle_value(&mut canvas.stage, "Stage view")
            .on_hover_text("Every placed projector on your desktop (T)");
        if ui.button("Fit view").on_hover_text("0").clicked() {
            canvas.reset_view();
        }
    });
}

/// The left panel on Map: the current projector's surfaces, top first, with drag handles to
/// reorder them.
pub fn layers_panel(ui: &mut egui::Ui, editor: &mut Editor, canvas: &CanvasState) {
    egui::Panel::left("lumap-left")
        .default_size(250.0)
        .show(ui, |ui| {
            let Some(out) = current_projector(editor, canvas) else {
                ui.heading("Layers");
                crate::theme::empty_state(
                    ui,
                    "No projector yet",
                    "Add one in step 1, Setup, then come back to draw its surfaces.",
                );
                return;
            };
            ui.horizontal(|ui| {
                ui.heading("Layers");
                if ui
                    .small_button("+ surface")
                    .on_hover_text("Add a surface to this projector")
                    .clicked()
                {
                    editor.add_surface(&out);
                }
            });
            ui.label(RichText::new(format!("On {out}, top first")).weak().small());
            ui.add_space(4.0);
            // Bottom to top as stored; drawn top first.
            let ordered: Vec<(String, bool, bool, String)> = editor
                .project
                .surfaces_for_output(&out)
                .iter()
                .map(|s| (s.id.clone(), s.visible, s.locked, s.source.clone()))
                .collect();
            if ordered.is_empty() {
                crate::theme::empty_state(
                    ui,
                    "No surfaces yet",
                    "Add one for each thing you want to light, then drag its corners onto it.",
                );
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (pos, (sid, visible, locked, source)) in ordered.iter().enumerate().rev() {
                    layer_row(ui, editor, &ordered, pos, sid, *visible, *locked, source);
                }
            });
            ui.add_space(8.0);
            ui.label(
                RichText::new("Drag the dots to reorder. Right-click a layer for more.")
                    .weak()
                    .small(),
            );
        });
}

/// One layer: drag handle, eye, name, content, lock.
#[allow(clippy::too_many_arguments)]
fn layer_row(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    ordered: &[(String, bool, bool, String)],
    pos: usize,
    sid: &str,
    visible: bool,
    locked: bool,
    source: &str,
) {
    let selected = matches!(&editor.selection, Selection::Surface { id, .. } if id == sid)
        || editor.extra_selected.iter().any(|e| e == sid);
    let row = ui.horizontal(|ui| {
        // The drag handle: six dots, painted.
        let (grip, grip_response) = ui.allocate_exact_size(egui::vec2(12.0, 20.0), Sense::drag());
        for (dx, dy) in [
            (3.0, 5.0),
            (8.0, 5.0),
            (3.0, 10.0),
            (8.0, 10.0),
            (3.0, 15.0),
            (8.0, 15.0),
        ] {
            ui.painter()
                .circle_filled(grip.min + egui::vec2(dx, dy), 1.3, crate::theme::MUTED);
        }
        grip_response
            .on_hover_cursor(egui::CursorIcon::Grab)
            .dnd_set_drag_payload(sid.to_string());
        let mut v = visible;
        if ui
            .checkbox(&mut v, "")
            .on_hover_text("Show or hide")
            .changed()
        {
            editor.set_param(&format!("/surface/{sid}/visible"), ParamValue::Bool(v));
            editor.commit();
        }
        let r = ui.selectable_label(selected, sid);
        if r.clicked() {
            let shift = ui.input(|i| i.modifiers.shift);
            editor.select_surface(sid, shift);
        }
        r.context_menu(|ui| surface_context_menu(ui, editor, sid, locked));
        if locked {
            crate::theme::pill(ui, "locked", false);
        }
        ui.label(RichText::new(source).weak().small());
    });
    // Dropping a layer here moves it to this layer's place.
    let target = ui.interact(
        row.response.rect,
        ui.scope_id().with(("layer", sid)),
        Sense::hover(),
    );
    if let Some(dragged) = target.dnd_hover_payload::<String>() {
        if dragged.as_str() != sid {
            ui.painter().hline(
                row.response.rect.x_range(),
                row.response.rect.top(),
                Stroke::new(2.0, crate::theme::SIGNAL),
            );
        }
    }
    if let Some(dragged) = target.dnd_release_payload::<String>() {
        if let Some(from) = ordered.iter().position(|(id, ..)| id == dragged.as_str()) {
            editor.move_surface(&dragged, pos as i32 - from as i32);
        }
    }
}

/// Right-click on a layer.
fn surface_context_menu(ui: &mut egui::Ui, editor: &mut Editor, sid: &str, locked: bool) {
    if ui.button("Duplicate  (Ctrl+D)").clicked() {
        editor.duplicate_surface(sid);
        ui.close();
    }
    if ui
        .button(if locked { "Unlock  (L)" } else { "Lock  (L)" })
        .clicked()
    {
        editor.set_locked(sid, !locked);
        ui.close();
    }
    if ui.button("Bring to front").clicked() {
        editor.send_surface(sid, true);
        ui.close();
    }
    if ui.button("Send to back").clicked() {
        editor.send_surface(sid, false);
        ui.close();
    }
    if ui.button("Reset warp to full frame").clicked() {
        editor.set_warp(sid, WarpKind::default());
        editor.commit();
        ui.close();
    }
    if ui.button("Delete this surface").clicked() {
        editor.selection = Selection::Surface {
            id: sid.to_string(),
            mask: None,
        };
        editor.extra_selected.clear();
        editor.delete_selection();
        ui.close();
    }
}

/// The small action bar beside the selected surface on the canvas.
pub fn action_bar(ctx: &egui::Context, editor: &mut Editor, canvas: &CanvasState) {
    let Selection::Surface { id, .. } = editor.selection.clone() else {
        return;
    };
    let (Some(rect), false) = (canvas.last_rect, canvas.stage) else {
        return;
    };
    let Some(surface) = editor.project.surface(&id).cloned() else {
        return;
    };
    if canvas.output.as_deref() != Some(surface.output.as_str()) {
        return;
    }
    let map = Mapping { rect };
    let points: Vec<egui::Pos2> = handles(&surface.warp, None)
        .into_iter()
        .map(|(_, p)| map.to_screen(p))
        .collect();
    let bbox = egui::Rect::from_points(&points);
    let above = bbox.top() - 40.0 > rect.top().min(bbox.top());
    let anchor = if above {
        egui::pos2(bbox.left(), bbox.top() - 40.0)
    } else {
        egui::pos2(bbox.left(), bbox.bottom() + 8.0)
    };
    egui::Area::new(egui::Id::unique("lumap-surface-actions"))
        .fixed_pos(anchor)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(crate::theme::RAISED)
                .stroke(Stroke::new(1.0, crate::theme::LINE))
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| actions(ui, editor, &id, &surface));
                });
        });
}

fn actions(ui: &mut egui::Ui, editor: &mut Editor, id: &str, surface: &Surface) {
    if ui
        .button(if surface.locked { "Unlock" } else { "Lock" })
        .on_hover_text("No handles, no accidental drags (L)")
        .clicked()
    {
        editor.set_locked(id, !surface.locked);
    }
    if ui.button("Duplicate").on_hover_text("Ctrl+D").clicked() {
        editor.duplicate_surface(id);
    }
    if ui
        .button("Flip")
        .on_hover_text("Mirror left and right")
        .clicked()
    {
        editor.align(id, AlignOp::FlipHorizontal);
    }
    if ui
        .button("Rotate")
        .on_hover_text("A quarter turn")
        .clicked()
    {
        editor.align(id, AlignOp::RotateQuarter);
    }
    if ui
        .button("Fit")
        .on_hover_text("Cover the whole projector")
        .clicked()
    {
        editor.align(id, AlignOp::FitOutput);
    }
    let curved = matches!(surface.warp, WarpKind::Bezier(_));
    if ui
        .button(if curved { "Corners" } else { "Curve" })
        .on_hover_text(if curved {
            "Back to four corners"
        } else {
            "Bend the surface onto a curved object"
        })
        .clicked()
    {
        let warp = if curved {
            Ok(WarpKind::default())
        } else {
            surface.warp.to_bezier(2, 2).map(WarpKind::Bezier)
        };
        if let Ok(w) = warp {
            editor.set_warp(id, w);
            editor.commit();
        }
    }
    if crate::theme::danger_button(ui, "Delete").clicked() {
        editor.selection = Selection::Surface {
            id: id.to_string(),
            mask: None,
        };
        editor.extra_selected.clear();
        editor.delete_selection();
    }
}

/// The surface panel.
#[allow(clippy::too_many_lines)]
pub fn surface_panel(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    id: &str,
    mask: Option<usize>,
    state: &mut PanelState,
) {
    ui.heading(format!("Surface {id}"));
    if rename_box(ui, editor, state, id) {
        return;
    }
    let Some(surface) = editor.project.surface(id).cloned() else {
        return;
    };

    crate::theme::section_header(ui, "Content");
    let mut source = surface.source.clone();
    egui::ComboBox::from_label("Content")
        .selected_text(&source)
        .show_ui(ui, |ui| {
            for s in &editor.project.sources {
                ui.selectable_value(&mut source, s.id.clone(), &s.id);
            }
        });
    if source != surface.source {
        let sid = id.to_string();
        editor.edit(move |p| {
            if let Some(s) = p.surface_mut(&sid) {
                s.source = source;
            }
        });
    }
    super::super::looks::surface_section(ui, editor, id, state);

    crate::theme::section_header(ui, "Appearance");
    param_toggle(ui, editor, "Visible", &format!("/surface/{id}/visible"));
    param_slider(
        ui,
        editor,
        "Opacity",
        &format!("/surface/{id}/opacity"),
        0.0..=1.0,
    );
    let mut mode = surface.blend_mode;
    egui::ComboBox::from_label("Blend")
        .selected_text(blend_label(mode))
        .show_ui(ui, |ui| {
            for m in [
                BlendMode::Normal,
                BlendMode::Add,
                BlendMode::Screen,
                BlendMode::Multiply,
            ] {
                ui.selectable_value(&mut mode, m, blend_label(m));
            }
        });
    if mode != surface.blend_mode {
        let sid = id.to_string();
        editor.edit(move |p| {
            if let Some(s) = p.surface_mut(&sid) {
                s.blend_mode = mode;
            }
        });
    }

    crate::theme::section_header(ui, "Shape");
    match &surface.warp {
        WarpKind::Quad(_) => {
            ui.label(
                RichText::new("Four corners: drag them onto the object. Shift-drag moves it all.")
                    .weak()
                    .small(),
            );
            if ui
                .button("Make it curved")
                .on_hover_text("A Bezier mesh with as many patches as set under Precise")
                .clicked()
            {
                let (cols, rows) = (state.mesh_cols.max(1), state.mesh_rows.max(1));
                match surface.warp.to_bezier(cols, rows) {
                    Ok(b) => {
                        editor.set_warp(id, WarpKind::Bezier(b));
                        editor.commit();
                    }
                    Err(e) => state.error = Some(e.to_string()),
                }
            }
        }
        WarpKind::Bezier(b) => {
            ui.label(
                RichText::new(format!(
                    "Curved: {}×{} patches. Drag the control points onto the curve.",
                    b.cols, b.rows
                ))
                .weak()
                .small(),
            );
            ui.horizontal(|ui| {
                if ui.button("Flatten the curve").clicked() {
                    if let Ok(flat) = BezierMesh::flat(b.cols, b.rows) {
                        editor.set_warp(id, WarpKind::Bezier(flat));
                        editor.commit();
                    }
                }
                if ui.button("Back to four corners").clicked() {
                    editor.set_warp(id, WarpKind::default());
                    editor.commit();
                }
            });
        }
    }
    if ui
        .button("Reset shape")
        .on_hover_text("Fill the projector again")
        .clicked()
    {
        editor.set_warp(id, WarpKind::default());
        editor.commit();
    }

    crate::theme::section_header(ui, "Arrange");
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Bring forward")
            .on_hover_text("Draw this surface above the next one")
            .clicked()
        {
            editor.move_surface(id, 1);
        }
        if ui
            .button("Send backward")
            .on_hover_text("Draw this surface below the next one")
            .clicked()
        {
            editor.move_surface(id, -1);
        }
        ui.menu_button("Align", |ui| {
            for (label, op) in [
                ("Flip horizontal", AlignOp::FlipHorizontal),
                ("Flip vertical", AlignOp::FlipVertical),
                ("Rotate 90°", AlignOp::RotateQuarter),
                ("Fit the projector", AlignOp::FitOutput),
                ("Centre", AlignOp::Centre),
                ("Match the content's shape", AlignOp::MatchSourceAspect),
            ] {
                if ui.button(label).clicked() {
                    editor.align(id, op);
                    ui.close();
                }
            }
        });
    });

    egui::CollapsingHeader::new("Crop")
        .id_salt(("crop", id))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Drag the box corners to use part of the content.")
                    .weak()
                    .small(),
            );
            super::super::inspectors::crop_preview(ui, editor, state, id, &surface);
        });

    cut_outs(ui, editor, id, mask, &surface);

    egui::CollapsingHeader::new("Precise")
        .id_salt(("precise-surface", id))
        .show(ui, |ui| precise(ui, editor, state, id, &surface));

    ui.add_space(10.0);
    if crate::theme::danger_button(ui, "Delete surface").clicked() {
        editor.selection = Selection::Surface {
            id: id.to_string(),
            mask: None,
        };
        editor.delete_selection();
    }
    if let Some(e) = &state.error {
        ui.colored_label(crate::theme::LIVE, e);
    }
}

/// Cut-outs (masks): shapes that hide what is inside them, or show only what is inside.
fn cut_outs(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    id: &str,
    mask: Option<usize>,
    surface: &Surface,
) {
    crate::theme::section_header(ui, "Cut-outs");
    if surface.masks.is_empty() {
        ui.label(
            RichText::new(
                "None. Add a cut-out to hide part of the surface, or a window to show only part.",
            )
            .weak()
            .small(),
        );
    }
    if ui.small_button("+ cut-out").clicked() {
        editor.add_mask(id);
    }
    for (i, m) in surface.masks.iter().enumerate() {
        let selected = mask == Some(i);
        let kind = if m.subtract { "Cut-out" } else { "Window" };
        if ui
            .selectable_label(
                selected,
                format!("{kind} {} · {} points", i + 1, m.points.len()),
            )
            .clicked()
        {
            editor.selection = Selection::Surface {
                id: id.to_string(),
                mask: if selected { None } else { Some(i) },
            };
        }
        if selected {
            let mut edited = m.clone();
            let mut changed = false;
            let r = ui.add(egui::Slider::new(&mut edited.feather, 0.0..=0.3).text("Feather"));
            changed |= r.drag_stopped() || r.lost_focus();
            changed |= ui
                .checkbox(
                    &mut edited.subtract,
                    "Hide what is inside (off: show only inside)",
                )
                .changed();
            ui.horizontal(|ui| {
                if ui.button("Add point").clicked() {
                    let last = edited.points.last().copied().unwrap_or_default();
                    let first = edited.points.first().copied().unwrap_or_default();
                    edited.points.push(last.lerp(first, 0.5));
                    changed = true;
                }
                if crate::theme::danger_button(ui, "Delete cut-out").clicked() {
                    editor.selection = Selection::Surface {
                        id: id.to_string(),
                        mask: Some(i),
                    };
                    editor.delete_selection();
                }
            });
            if changed && edited != *m {
                let mut masks = surface.masks.clone();
                masks[i] = edited;
                editor.set_masks(id, masks);
                editor.commit();
            }
        }
    }
}

/// Corner positions in pixels, draw order, mesh size and MIDI learn.
fn precise(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    state: &mut PanelState,
    id: &str,
    surface: &Surface,
) {
    let (w, h) = editor
        .project
        .output(&surface.output)
        .map_or((1.0, 1.0), |o| (f64::from(o.width), f64::from(o.height)));
    if let WarpKind::Quad(q) = &surface.warp {
        for (i, c) in q.corners.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("Corner {}", i + 1));
                let (mut x, mut y) = (c.x * w, c.y * h);
                let rx = ui.add(
                    egui::DragValue::new(&mut x)
                        .speed(0.5)
                        .fixed_decimals(1)
                        .suffix(" px"),
                );
                let ry = ui.add(
                    egui::DragValue::new(&mut y)
                        .speed(0.5)
                        .fixed_decimals(1)
                        .suffix(" px"),
                );
                if rx.changed() {
                    editor.set_param(
                        &format!("/surface/{id}/corner/{i}/x"),
                        ParamValue::Number(x / w),
                    );
                }
                if ry.changed() {
                    editor.set_param(
                        &format!("/surface/{id}/corner/{i}/y"),
                        ParamValue::Number(y / h),
                    );
                }
                if rx.drag_stopped() || ry.drag_stopped() || rx.lost_focus() || ry.lost_focus() {
                    editor.commit();
                }
            });
        }
    }
    param_slider(
        ui,
        editor,
        "Order (z)",
        &format!("/surface/{id}/z"),
        -10.0..=10.0,
    );
    if state.mesh_cols == 0 {
        state.mesh_cols = 2;
        state.mesh_rows = 2;
    }
    ui.horizontal(|ui| {
        ui.label("Curve patches");
        ui.add(
            egui::DragValue::new(&mut state.mesh_cols)
                .range(1..=BezierMesh::MAX_PATCHES)
                .prefix("cols "),
        );
        ui.add(
            egui::DragValue::new(&mut state.mesh_rows)
                .range(1..=BezierMesh::MAX_PATCHES)
                .prefix("rows "),
        );
    });
    if ui
        .button("MIDI learn opacity")
        .on_hover_text("Move a fader to map it to this surface's opacity")
        .clicked()
    {
        editor.arm_midi_learn(&format!("/surface/{id}/opacity"), MidiMode::Fader);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumap_core::project::demo_project;

    #[test]
    fn blend_modes_have_plain_words_and_the_canvas_follows_the_selection() {
        assert_eq!(blend_label(BlendMode::Add), "Brighten (add)");
        assert_eq!(blend_label(BlendMode::Multiply), "Darken (multiply)");
        let mut e = Editor::new(demo_project(), None);
        let mut canvas = CanvasState::default();
        assert_eq!(current_projector(&e, &canvas).as_deref(), Some("left"));
        canvas.output = Some("right".into());
        assert_eq!(current_projector(&e, &canvas).as_deref(), Some("right"));
        e.selection = Selection::Surface {
            id: "wall-left".into(),
            mask: None,
        };
        assert_eq!(current_projector(&e, &canvas).as_deref(), Some("left"));
        canvas.output = Some("gone".into());
        e.selection = Selection::None;
        assert_eq!(current_projector(&e, &canvas).as_deref(), Some("left"));
        let empty = Editor::new(lumap_core::project::Project::default(), None);
        assert_eq!(current_projector(&empty, &canvas), None);
    }
}
