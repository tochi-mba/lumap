//! Choosing built-in looks through the real UI: the welcome chips, the surface inspector and
//! the "+ content" menu grouped by category.

use super::*;
use crate::editor::Editor;
use egui_kittest::kittest::Queryable;
use lumap_core::project::{demo_project, BlendMode, GeneratorKind, SourceKind};

#[test]
fn a_welcome_chip_puts_the_look_on_every_surface_and_closes_the_welcome() {
    let mut h = harness(Editor::new(demo_project(), None));
    h.state_mut().panels.home = true;
    settle(&mut h);
    click(&mut h, "Campfire");
    let state = h.state();
    assert!(!state.panels.home);
    assert_eq!(
        state.panels.step,
        crate::editor::panels::shell::Step::Content
    );
    assert!(state
        .editor
        .project
        .surfaces
        .iter()
        .all(|s| s.source == "look-campfire"));
    assert!(matches!(
        state.editor.project.source("look-campfire").unwrap().kind,
        SourceKind::Generator {
            pattern: GeneratorKind::Fire,
            ..
        }
    ));
    assert_eq!(state.editor.status, "Campfire on every surface");
    // One undo puts every surface back.
    click(&mut h, "Undo");
    assert!(h
        .state()
        .editor
        .project
        .surfaces
        .iter()
        .all(|s| s.source != "look-campfire"));
}

#[test]
fn the_surface_inspector_applies_the_chosen_look_to_that_surface() {
    let mut h = harness(Editor::new(demo_project(), None));
    click(&mut h, "wall-left");
    click(&mut h, "Look");
    click(&mut h, "Night windows");
    h.get_by_label("Windows lighting up and going dark, like a building at night.");
    click(&mut h, "Apply look");
    let p = &h.state().editor.project;
    let left = p.surface("wall-left").unwrap();
    assert_eq!(left.source, "look-night-windows");
    assert_eq!(left.blend_mode, BlendMode::Normal);
    assert_ne!(
        p.surface("wall-right").unwrap().source,
        "look-night-windows"
    );
}

#[test]
fn the_source_menu_groups_generators_by_category() {
    let mut h = harness(Editor::new(demo_project(), None));
    click(&mut h, "+ content");
    click(&mut h, "Nature");
    click(&mut h, "Generator: Aurora");
    assert!(h.state().editor.project.sources.iter().any(|s| matches!(
        s.kind,
        SourceKind::Generator {
            pattern: GeneratorKind::Aurora,
            ..
        }
    )));
}
