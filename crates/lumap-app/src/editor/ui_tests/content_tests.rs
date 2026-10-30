//! Step 3, Content, through the real UI: the library's tiles, filters, drag-and-drop onto the
//! canvas, and the right-click menu of content already in the show.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::editor::Editor;
use egui_kittest::kittest::Queryable;
use lumap_core::project::demo_project;

fn on_content() -> Harness<'static, LumapApp> {
    let mut h = harness(Editor::new(demo_project(), None));
    step(&mut h, "3 Content");
    h
}

fn source_of(h: &Harness<'static, LumapApp>, surface: &str) -> String {
    h.state()
        .editor
        .project
        .surface(surface)
        .unwrap()
        .source
        .clone()
}

#[test]
fn a_tile_goes_on_the_selected_surface_or_on_the_whole_projector() {
    let mut h = on_content();
    assert!(h.query_by_label("Library").is_some());
    let right = source_of(&h, "wall-right");
    // Nothing selected: every surface on the current projector.
    click(&mut h, "Campfire");
    assert_eq!(source_of(&h, "wall-left"), "look-campfire");
    assert_eq!(
        source_of(&h, "wall-right"),
        right,
        "the other projector is untouched"
    );
    // A selected surface takes the tile alone.
    h.state_mut().editor.selection = Selection::Surface {
        id: "logo".into(),
        mask: None,
    };
    settle(&mut h);
    click(&mut h, "Anniversary");
    assert_eq!(source_of(&h, "logo"), "look-anniversary");
    assert_eq!(source_of(&h, "wall-right"), right);
    click(&mut h, "White");
    assert!(
        source_of(&h, "logo").starts_with("solid"),
        "{}",
        source_of(&h, "logo")
    );
    click(&mut h, "Checker");
    assert!(
        source_of(&h, "logo").starts_with("test-pattern")
            || source_of(&h, "logo").starts_with("pattern"),
        "{}",
        source_of(&h, "logo")
    );
    // One undo puts it back.
    click(&mut h, "Undo");
    assert!(source_of(&h, "logo").starts_with("solid"));
}

#[test]
fn filters_narrow_the_library() {
    let mut h = on_content();
    assert!(h.query_by_label("Anniversary").is_some());
    click(&mut h, "Romance");
    assert!(h.query_by_label("Anniversary").is_some());
    assert!(h.query_by_label("Campfire").is_none());
    assert!(
        h.query_by_label("Patterns").is_some(),
        "the chip is still there"
    );
    click(&mut h, "Patterns");
    assert!(h.query_by_label("Grey ramp").is_some());
    assert!(h.query_by_label("Anniversary").is_none());
    click(&mut h, "Your media");
    assert!(h.query_by_label("In this show").is_some());
    assert!(h.query_by_label("Grey ramp").is_none());
    click(&mut h, "All");
    assert!(h.query_by_label("Grey ramp").is_some());
}

#[test]
fn dragging_a_tile_onto_a_surface_on_the_canvas_puts_it_there() {
    let mut h = on_content();
    let canvas = h.state().canvas.last_rect.unwrap();
    // The demo's wall-left covers most of the left projector.
    let target = canvas.center();
    h.get_by_label("Northern lights").scroll_to_me();
    settle(&mut h);
    let tile = h.get_by_label("Northern lights").rect().center();
    drag(&mut h, tile, target);
    assert_eq!(source_of(&h, "wall-left"), "look-northern-lights");
    assert_eq!(
        h.state().editor.selection,
        Selection::Surface {
            id: "wall-left".into(),
            mask: None
        },
        "the surface that took it is selected"
    );
}

#[test]
fn content_in_the_show_opens_its_settings_or_is_removed_when_unused() {
    let mut h = on_content();
    // A new generator is in the show but on no surface: its menu removes it.
    click(&mut h, "+ generator");
    h.get_all_by_label("Classic").next().unwrap().click();
    settle(&mut h);
    click(&mut h, "Generator: Plasma");
    assert!(h.state().editor.project.source("generator").is_some());
    h.get_by_label("generator").click_secondary();
    settle(&mut h);
    click(&mut h, "Remove");
    assert!(h.state().editor.project.source("generator").is_none());
    // Content a surface shows cannot be removed; its menu opens its settings.
    h.get_by_label("grid").click_secondary();
    settle(&mut h);
    click(&mut h, "Remove");
    assert!(
        h.state().editor.project.source("grid").is_some(),
        "Remove is disabled while a surface shows it"
    );
    h.key_press(Key::Escape);
    settle(&mut h);
    h.get_by_label("grid").click_secondary();
    settle(&mut h);
    click(&mut h, "Settings");
    assert_eq!(h.state().editor.selection, Selection::Source("grid".into()));
    assert!(h.query_by_label("Content grid").is_some());
    assert!(h.state().editor.project.source("grid").is_some());
}
