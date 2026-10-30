//! Step 2, Map, through the real UI: layers, projector tabs, the toolbar, and the action bar
//! beside the selected surface.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::editor::Editor;
use egui_kittest::kittest::Queryable;
use lumap_core::project::demo_project;
use lumap_core::warp::WarpKind;

fn on_map() -> egui_kittest::Harness<'static, LumapApp> {
    let mut h = harness(Editor::new(demo_project(), None));
    step(&mut h, "2 Map");
    h
}

fn z(h: &egui_kittest::Harness<'static, LumapApp>, id: &str) -> i32 {
    h.state().editor.project.surface(id).unwrap().z
}

#[test]
fn layers_list_one_projector_top_first_and_tabs_switch_it() {
    let mut h = on_map();
    assert!(h.query_by_label("Layers").is_some());
    assert!(h.query_by_label("On left, top first").is_some());
    assert!(h.query_by_label("wall-left").is_some());
    assert!(
        h.query_by_label("logo").is_none(),
        "logo is on the other projector"
    );
    click(&mut h, "right");
    assert!(h.query_by_label("On right, top first").is_some());
    assert!(h.query_by_label("logo").is_some());
    // Hide and show from the eye.
    h.get_all_by_label("Show or hide").next().unwrap().click();
    settle(&mut h);
    let hidden = h
        .state()
        .editor
        .project
        .surfaces
        .iter()
        .filter(|s| s.output == "right" && !s.visible)
        .count();
    assert_eq!(hidden, 1);
    // Add a surface from the toolbar and from the layers header.
    let before = h.state().editor.project.surfaces.len();
    click(&mut h, "Add surface");
    click(&mut h, "+ surface");
    assert_eq!(h.state().editor.project.surfaces.len(), before + 2);
}

#[test]
fn dragging_a_layer_by_its_dots_reorders_it() {
    let mut h = on_map();
    click(&mut h, "right");
    let (logo, wall) = (z(&h, "logo"), z(&h, "wall-right"));
    assert!(logo > wall, "the logo starts on top");
    // Drag the bottom layer's grip onto the top layer's row.
    let wall_row = h.get_by_label("wall-right").rect();
    let logo_row = h.get_by_label("logo").rect();
    let grip = egui::pos2(wall_row.left() - 50.0, wall_row.center().y);
    drag(
        &mut h,
        grip,
        egui::pos2(logo_row.center().x, logo_row.center().y),
    );
    assert!(
        z(&h, "wall-right") > z(&h, "logo"),
        "wall-right is now on top"
    );
}

#[test]
fn the_action_bar_locks_duplicates_flips_rotates_fits_curves_and_deletes() {
    let mut h = on_map();
    click(&mut h, "wall-left");
    click(&mut h, "Lock");
    assert!(h.state().editor.is_locked("wall-left"));
    assert!(h.query_by_label("locked").is_some(), "the layer says so");
    click(&mut h, "Unlock");
    assert!(!h.state().editor.is_locked("wall-left"));
    let warp = h
        .state()
        .editor
        .project
        .surface("wall-left")
        .unwrap()
        .warp
        .clone();
    click(&mut h, "Flip");
    assert_ne!(
        h.state().editor.project.surface("wall-left").unwrap().warp,
        warp
    );
    click(&mut h, "Rotate");
    click(&mut h, "Fit");
    assert_eq!(
        h.state().editor.project.surface("wall-left").unwrap().warp,
        WarpKind::default(),
        "fit covers the projector"
    );
    click(&mut h, "Curve");
    assert!(matches!(
        h.state().editor.project.surface("wall-left").unwrap().warp,
        WarpKind::Bezier(_)
    ));
    click(&mut h, "Corners");
    assert!(matches!(
        h.state().editor.project.surface("wall-left").unwrap().warp,
        WarpKind::Quad(_)
    ));
    click(&mut h, "Add cut-out");
    assert_eq!(
        h.state()
            .editor
            .project
            .surface("wall-left")
            .unwrap()
            .masks
            .len(),
        1
    );
    click(&mut h, "wall-left");
    click(&mut h, "Duplicate");
    assert!(h.state().editor.project.surface("wall-left-copy").is_some());
    click(&mut h, "Delete");
    assert!(h.state().editor.project.surface("wall-left-copy").is_none());
}
