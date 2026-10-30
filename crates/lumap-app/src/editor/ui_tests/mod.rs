//! Headless tests of the real editor UI, driven through egui's accessibility tree.

#![allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::print_stderr
)]

use super::handles::Mapping;
use super::panels::FakeDialogs;
use super::{Launch, LumapApp, MonitorInfo, Pending, Selection};
use crate::editor::Editor;
use eframe::egui::{self, accesskit::Role, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use lumap_core::geom::Vec2;
use lumap_core::project::demo_project;
use lumap_core::warp::WarpKind;
use std::path::PathBuf;

fn monitors() -> Vec<MonitorInfo> {
    vec![
        MonitorInfo {
            name: "DISPLAY1".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
        },
        MonitorInfo {
            name: "DISPLAY2".into(),
            x: 1920,
            y: 0,
            width: 1280,
            height: 720,
            scale: 1.0,
        },
    ]
}

/// Tests that write `settings.toml` share one file (the config dir is process-wide), so each
/// of them holds this lock for its whole body.
fn settings_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn config_dir() -> PathBuf {
    static DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let dir = DIR.get_or_init(|| tempfile::tempdir().unwrap());
    std::env::set_var("LUMAP_CONFIG_DIR", dir.path());
    dir.path().to_path_buf()
}

fn app(editor: Editor) -> LumapApp {
    config_dir();
    let launch = Launch {
        project: None,
        new_show: false,
        open_show: false,
        check_updates: false,
        osc_port: 0,
        show: false,
        restart_on_stall: None,
        remote_port: None,
        lab: None,
    };
    let mut app = LumapApp::new(editor, &launch, monitors(), wgpu::TextureFormat::Rgba8Unorm);
    app.panels.dialogs = Box::new(FakeDialogs::default());
    // A fake camera, so no test touches a real webcam or lists the machine's devices.
    app.camera = super::camera::CameraHub::new(std::sync::Arc::new(
        lumap_sources::live_input::camera::FakeCamera::solid(
            "Test camera",
            64,
            36,
            [30, 30, 30, 255],
        ),
    ));
    app
}

fn harness(editor: Editor) -> Harness<'static, LumapApp> {
    let mut h = Harness::builder()
        .with_size(egui::Vec2::new(1400.0, 900.0))
        .build_ui_state(|ui, app: &mut LumapApp| app.frame(ui), app(editor));
    crate::theme::apply(&h.ctx);
    h.run_steps(3);
    h
}

/// Runs a few frames. Show mode and live sources repaint continuously, so a fixed number of
/// steps is used instead of waiting for the UI to settle.
fn settle(h: &mut Harness<'static, LumapApp>) {
    h.run_steps(3);
}

fn click(h: &mut Harness<'static, LumapApp>, label: &str) {
    // Inspectors scroll; bring the control into view before clicking it.
    h.get_by_label(label).scroll_to_me();
    settle(h);
    h.get_by_label(label).click();
    settle(h);
}

/// Clicks a step tab: "1 Setup", "2 Map", "3 Content" or "4 Show".
fn step(h: &mut Harness<'static, LumapApp>, tab: &str) {
    click(h, tab);
}

fn menu(h: &mut Harness<'static, LumapApp>, menu: &str, item: &str) {
    click(h, menu);
    click(h, item);
}

fn fake(h: &mut Harness<'static, LumapApp>, f: impl FnOnce(&mut FakeDialogs)) {
    let mut d = FakeDialogs::default();
    f(&mut d);
    h.state_mut().panels.dialogs = Box::new(d);
}

fn surface_warp(h: &Harness<'static, LumapApp>, id: &str) -> WarpKind {
    h.state().editor.project.surface(id).unwrap().warp.clone()
}

/// A primary click at a screen position (press, release, settle).
fn click_at(h: &mut Harness<'static, LumapApp>, pos: egui::Pos2) {
    h.hover_at(pos);
    settle(h);
    h.drag_at(pos);
    settle(h);
    h.drop_at(pos);
    settle(h);
}

fn drag(h: &mut Harness<'static, LumapApp>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    settle(h);
    h.drag_at(from);
    settle(h);
    h.hover_at(from + (to - from) * 0.5);
    settle(h);
    h.hover_at(to);
    settle(h);
    h.drop_at(to);
    settle(h);
}

#[test]
fn closing_with_unsaved_work_never_discards_after_cancelled_save() {
    let _lock = settings_guard();
    let _config = config_dir();
    let mut h = harness(Editor::new(demo_project(), None));
    h.state_mut().editor.dirty = true;
    h.state_mut().request(Pending::Close);
    settle(&mut h);
    h.get_by_label("Unsaved changes");
    click(&mut h, "Save and quit");
    assert_eq!(h.state().editor.pending, Some(Pending::Close));
    assert!(h.state().editor.dirty);
    assert!(!h.state().close_confirmed);
    click(&mut h, "Cancel");
    assert_eq!(h.state().editor.pending, None);
    assert!(h.state().editor.dirty);
    assert!(!h.state().close_confirmed);
}

#[test]
fn saving_before_exit_must_succeed_before_app_closes() {
    let _lock = settings_guard();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("my-show.lumap.toml");
    let mut h = harness(Editor::new(demo_project(), None));
    h.state_mut().editor.dirty = true;
    h.state_mut().request(Pending::Close);
    fake(&mut h, |d| d.save = Some(dir.path().to_path_buf()));
    settle(&mut h);
    click(&mut h, "Save and quit");
    assert_eq!(
        h.state().editor.pending,
        Some(Pending::Close),
        "failed save keeps prompt"
    );
    assert!(h.state().panels.error.is_some());
    assert!(!h.state().close_confirmed);

    fake(&mut h, |d| d.save = Some(target.clone()));
    click(&mut h, "Save and quit");
    assert!(target.is_file());
    assert!(!h.state().editor.dirty);
    assert!(h.state().close_confirmed);
}

#[test]
fn installing_with_unsaved_work_requires_confirmation() {
    let _lock = settings_guard();
    let mut h = harness(Editor::new(demo_project(), None));
    h.state_mut().editor.dirty = true;
    h.state_mut().request(Pending::InstallUpdate);
    settle(&mut h);
    h.get_by_label("Save and install");
    h.get_by_label("Install without saving");
    click(&mut h, "Cancel");
    assert_eq!(h.state().editor.pending, None);
    assert!(!h.state().updater.closing);
}

#[test]
fn starting_a_new_show_uses_a_blank_project() {
    let _lock = settings_guard();
    let mut h = harness(Editor::new(demo_project(), None));
    menu(&mut h, "Menu", "New show");
    assert!(h.state().editor.project.outputs.is_empty());
    assert!(h.state().editor.project.surfaces.is_empty());
}

#[test]
fn panels_render_and_selection_drives_the_inspector() {
    let mut h = harness(Editor::new(demo_project(), None));
    step(&mut h, "1 Setup");
    assert!(h.query_by_label("+ projector").is_some());
    click(&mut h, "left");
    assert_eq!(h.state().editor.selection, Selection::Output("left".into()));
    assert!(h.query_by_label("Projector left").is_some());
    step(&mut h, "2 Map");
    click(&mut h, "right");
    click(&mut h, "logo");
    assert_eq!(
        h.state().editor.selection,
        Selection::Surface {
            id: "logo".into(),
            mask: None
        }
    );
    assert!(h.query_by_label("Surface logo").is_some());
    assert!(h.state().canvas.last_rect.is_some());
    assert_eq!(h.state().canvas.output.as_deref(), Some("right"));
    // Shift-click extends the selection and the status bar says so.
    h.get_by_label("wall-right")
        .click_modifiers(Modifiers::SHIFT);
    settle(&mut h);
    assert_eq!(h.state().editor.selected_surfaces().len(), 2);
    assert!(h.query_by_label("2 surfaces selected").is_some());
    // With nothing selected, each step's inspector says where to start.
    h.key_press(Key::Escape);
    settle(&mut h);
    assert!(h.query_by_label("Select a surface").is_some());
    step(&mut h, "4 Show");
    assert!(h.query_by_label("Control").is_some());
}

#[test]
fn surface_inspector_buttons_edit_the_project() {
    let mut h = harness(Editor::new(demo_project(), None));
    step(&mut h, "2 Map");
    click(&mut h, "wall-left");
    click(&mut h, "+ cut-out");
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
    click(&mut h, "Add point");
    assert_eq!(
        h.state().editor.project.surface("wall-left").unwrap().masks[0]
            .points
            .len(),
        5
    );
    click(&mut h, "Hide what is inside (off: show only inside)");
    assert!(!h.state().editor.project.surface("wall-left").unwrap().masks[0].subtract);
    click(&mut h, "Delete cut-out");
    assert!(h
        .state()
        .editor
        .project
        .surface("wall-left")
        .unwrap()
        .masks
        .is_empty());

    click(&mut h, "Make it curved");
    assert!(matches!(surface_warp(&h, "wall-left"), WarpKind::Bezier(_)));
    click(&mut h, "Flatten the curve");
    click(&mut h, "Back to four corners");
    assert_eq!(surface_warp(&h, "wall-left"), WarpKind::default());
    click(&mut h, "Visible");
    assert!(
        !h.state()
            .editor
            .project
            .surface("wall-left")
            .unwrap()
            .visible
    );
    click(&mut h, "Visible");

    h.get_by_value("Normal").click();
    settle(&mut h);
    click(&mut h, "Light (screen)");
    assert_eq!(
        h.state()
            .editor
            .project
            .surface("wall-left")
            .unwrap()
            .blend_mode,
        lumap_core::project::BlendMode::Screen
    );
    h.get_all_by_role_and_label(Role::ComboBox, "Content")
        .next()
        .unwrap()
        .click();
    settle(&mut h);
    h.get_all_by_label("bars").last().unwrap().click();
    settle(&mut h);
    assert_eq!(
        h.state()
            .editor
            .project
            .surface("wall-left")
            .unwrap()
            .source,
        "bars"
    );

    // Lock, duplicate, order, align.
    click(&mut h, "Lock");
    assert!(h.state().editor.is_locked("wall-left"));
    click(&mut h, "Unlock");
    click(&mut h, "Duplicate");
    assert_eq!(
        h.state().editor.selection,
        Selection::Surface {
            id: "wall-left-copy".into(),
            mask: None
        }
    );
    click(&mut h, "Send backward");
    click(&mut h, "Bring forward");
    h.state_mut().editor.set_warp(
        "wall-left-copy",
        WarpKind::Quad(lumap_core::warp::Quad {
            corners: [
                Vec2::new(0.1, 0.2),
                Vec2::new(0.9, 0.1),
                Vec2::new(0.8, 0.9),
                Vec2::new(0.2, 0.8),
            ],
        }),
    );
    h.state_mut().editor.commit();
    settle(&mut h);
    let before = surface_warp(&h, "wall-left-copy");
    menu(&mut h, "Align", "Flip horizontal");
    assert_ne!(surface_warp(&h, "wall-left-copy"), before);
    for item in [
        "Flip vertical",
        "Rotate 90°",
        "Centre",
        "Match the content's shape",
        "Fit the projector",
    ] {
        menu(&mut h, "Align", item);
    }
    assert_eq!(surface_warp(&h, "wall-left-copy"), WarpKind::default());
    click(&mut h, "Reset shape");

    click(&mut h, "Delete surface");
    assert!(h.state().editor.project.surface("wall-left-copy").is_none());
    assert_eq!(h.state().editor.selection, Selection::None);
    click(&mut h, "Undo");
    assert!(h.state().editor.project.surface("wall-left-copy").is_some());
    click(&mut h, "Redo");
    assert!(h.state().editor.project.surface("wall-left-copy").is_none());
}

#[test]
fn the_show_name_keeps_what_is_typed_and_follows_undo() {
    let mut h = harness(Editor::new(demo_project(), None));
    let before = h.state().editor.project.name.clone();
    let field = h.get_by_role_and_label(Role::TextInput, "Show");
    field.focus();
    field.type_text(" at the pier");
    settle(&mut h);
    assert_eq!(
        h.state().editor.project.name,
        before,
        "nothing changes while typing"
    );
    h.key_press(Key::Enter);
    settle(&mut h);
    assert_eq!(
        h.state().editor.project.name,
        format!("{before} at the pier")
    );
    click(&mut h, "Undo");
    assert_eq!(h.state().editor.project.name, before);
    assert_eq!(
        h.get_by_role_and_label(Role::TextInput, "Show").value(),
        Some(before),
        "the field shows the name again after undo"
    );
}

#[test]
fn inline_rename_through_the_inspector() {
    let mut h = harness(Editor::new(demo_project(), None));
    step(&mut h, "2 Map");
    click(&mut h, "right");
    click(&mut h, "logo");
    let field = h.get_by_role_and_label(Role::TextInput, "Name");
    field.focus();
    field.type_text("-renamed");
    settle(&mut h);
    h.key_press(Key::Enter);
    settle(&mut h);
    assert!(
        h.state().editor.project.surface("logo-renamed").is_some(),
        "{:?}",
        h.state()
            .editor
            .project
            .surfaces
            .iter()
            .map(|s| &s.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        h.state().editor.selection,
        Selection::Surface {
            id: "logo-renamed".into(),
            mask: None
        }
    );
    // An invalid id is reported and nothing changes.
    let field = h.get_by_role_and_label(Role::TextInput, "Name");
    field.focus();
    field.type_text(" bad");
    settle(&mut h);
    h.key_press(Key::Enter);
    settle(&mut h);
    assert!(h.state().panels.error.is_some());
    click(&mut h, "Dismiss");
    assert!(h.state().panels.error.is_none());
}

#[test]
fn output_inspector_places_and_opens_windows() {
    let mut h = harness(Editor::new(demo_project(), None));
    step(&mut h, "1 Setup");
    click(&mut h, "left");
    assert!(h.query_by_label("Projector left").is_some());
    assert!(
        h.get_all_by_label("Floating window").count() >= 2,
        "the card and the display picker both say where it is"
    );
    click(&mut h, "Open window");
    assert_eq!(h.state().editor.open_outputs, ["left"]);
    settle(&mut h);
    assert!(
        h.query_by_label("window open").is_some(),
        "the card shows the open window"
    );
    assert!(h.state().editor.window_positions.contains_key("left"));
    click(&mut h, "Precise");
    click(&mut h, "Use current window position");
    assert!(h
        .state()
        .editor
        .project
        .output("left")
        .unwrap()
        .placement
        .is_some());
    click(&mut h, "Display");
    // The open picker's option is the newest "Floating window" on screen.
    h.get_all_by_label("Floating window")
        .last()
        .unwrap()
        .click();
    settle(&mut h);
    assert!(h
        .state()
        .editor
        .project
        .output("left")
        .unwrap()
        .placement
        .is_none());
    click(&mut h, "Display");
    click(&mut h, "Display 2 · 1280×720");
    let o = h.state().editor.project.output("left").unwrap().clone();
    assert_eq!(
        o.placement.as_ref().map(|p| p.monitor.as_str()),
        Some("DISPLAY2")
    );
    assert_eq!(
        (o.width, o.height),
        (1280, 720),
        "it takes the display's size"
    );
    click(&mut h, "Identify on the projector");
    assert!(h.state().editor.identify_until.is_some());
    settle(&mut h);
    click(&mut h, "Close window");
    assert!(h.state().editor.open_outputs.is_empty());
    click(&mut h, "On in show mode");
    assert!(!h.state().editor.project.output("left").unwrap().enabled);
    assert!(h.query_by_label("off in show").is_some());
    h.get_all_by_value("Smooth").next().unwrap().click();
    settle(&mut h);
    click(&mut h, "Soft");
    assert_eq!(
        h.state()
            .editor
            .project
            .output("left")
            .unwrap()
            .blend
            .left
            .curve,
        lumap_core::blend::BlendCurve::Cosine
    );
    // Precise size: scrub the width like a user would.
    h.get_by_role_and_label(Role::SpinButton, "Width")
        .scroll_to_me();
    settle(&mut h);
    let width = h.get_by_role_and_label(Role::SpinButton, "Width").rect();
    drag(
        &mut h,
        width.center(),
        width.center() + egui::vec2(30.0, 0.0),
    );
    assert!(h.state().editor.project.output("left").unwrap().width > 1280);
    click(&mut h, "Delete projector");
    assert!(h.state().editor.project.output("left").is_none());
}

#[test]
fn show_inspector_and_media_dialog() {
    let mut h = harness(Editor::new(demo_project(), None));
    step(&mut h, "3 Content");
    click(&mut h, "Add Spout receiver");
    assert!(h
        .state()
        .editor
        .project
        .sources
        .iter()
        .any(|s| s.id == "spout"));
    h.get_all_by_label("Blackout").next().unwrap().click();
    settle(&mut h);
    assert!(h.state().editor.project.blackout);
    let dir = tempfile::tempdir().unwrap();
    let img = dir.path().join("pic.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([9, 9, 9, 255]))
        .save(&img)
        .unwrap();
    fake(&mut h, |d| d.media = vec![img.clone()]);
    click(&mut h, "Add media…");
    assert!(h
        .state()
        .editor
        .project
        .sources
        .iter()
        .any(|s| s.id == "pic"));
}

#[test]
fn canvas_drag_moves_a_corner_and_keys_nudge_it() {
    let _settings = settings_guard();
    let mut h = harness(Editor::new(demo_project(), None));
    step(&mut h, "2 Map");
    click(&mut h, "wall-left");
    h.state_mut().canvas.handles.snap = false;
    let rect = h.state().canvas.last_rect.unwrap();
    let map = Mapping { rect };
    let WarpKind::Quad(q) = surface_warp(&h, "wall-left") else {
        panic!()
    };
    let start = map.to_screen(q.corners[2]);
    let end = start + egui::Vec2::new(-40.0, -30.0);
    drag(&mut h, start, end);
    let WarpKind::Quad(moved) = surface_warp(&h, "wall-left") else {
        panic!()
    };
    let want = map.to_norm(end);
    assert!(
        moved.corners[2].distance(want) < 0.01,
        "{:?} vs {want:?}",
        moved.corners[2]
    );
    assert_eq!(
        h.state().editor.history.undo_len(),
        1,
        "one drag is one undo step"
    );

    // Click a handle to make it active, then nudge with the keyboard (plain, Shift, Alt).
    let pos = map.to_screen(moved.corners[2]);
    h.hover_at(pos);
    h.drag_at(pos);
    h.drop_at(pos);
    settle(&mut h);
    assert!(h.state().canvas.readout.is_some());
    h.key_press(Key::ArrowRight);
    settle(&mut h);
    let WarpKind::Quad(n1) = surface_warp(&h, "wall-left") else {
        panic!()
    };
    assert!(n1.corners[2].x > moved.corners[2].x);
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowRight);
    settle(&mut h);
    let WarpKind::Quad(n2) = surface_warp(&h, "wall-left") else {
        panic!()
    };
    assert!((n2.corners[2].x - n1.corners[2].x) > 5.0 * (n1.corners[2].x - moved.corners[2].x));
    h.key_press_modifiers(Modifiers::ALT, Key::ArrowDown);
    settle(&mut h);
    let WarpKind::Quad(n3) = surface_warp(&h, "wall-left") else {
        panic!()
    };
    assert!(
        n3.corners[2].y > n2.corners[2].y
            && (n3.corners[2].y - n2.corners[2].y) < (n1.corners[2].x - moved.corners[2].x)
    );

    // Shift-drag on empty canvas moves everything.
    let mid = map.to_screen(Vec2::new(0.5, 0.5));
    h.event_modifiers(egui::Event::PointerMoved(mid), Modifiers::SHIFT);
    h.event_modifiers(
        egui::Event::PointerButton {
            pos: mid,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::SHIFT,
        },
        Modifiers::SHIFT,
    );
    settle(&mut h);
    h.event_modifiers(
        egui::Event::PointerMoved(mid + egui::Vec2::new(20.0, 0.0)),
        Modifiers::SHIFT,
    );
    settle(&mut h);
    h.event_modifiers(
        egui::Event::PointerButton {
            pos: mid + egui::Vec2::new(20.0, 0.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::SHIFT,
        },
        Modifiers::SHIFT,
    );
    settle(&mut h);
    let WarpKind::Quad(shifted) = surface_warp(&h, "wall-left") else {
        panic!()
    };
    assert!(shifted.corners[0].x > n3.corners[0].x);

    // Masks get handles too.
    h.state_mut().editor.add_mask("wall-left");
    settle(&mut h);
    let m = h.state().editor.project.surface("wall-left").unwrap().masks[0].clone();
    let p0 = map.to_screen(m.points[0]);
    drag(&mut h, p0, p0 + egui::Vec2::new(15.0, 15.0));
    assert!(
        h.state().editor.project.surface("wall-left").unwrap().masks[0].points[0].x > m.points[0].x
    );

    // Bezier handles draw and drag as well.
    h.state_mut().editor.selection = Selection::Surface {
        id: "wall-left".into(),
        mask: None,
    };
    h.state_mut().editor.set_warp(
        "wall-left",
        WarpKind::Bezier(lumap_core::warp::BezierMesh::flat(2, 2).unwrap()),
    );
    h.state_mut().editor.commit();
    settle(&mut h);
    let p = map.to_screen(Vec2::new(0.5, 0.5));
    drag(&mut h, p, p + egui::Vec2::new(5.0, 5.0));
    let WarpKind::Bezier(b) = surface_warp(&h, "wall-left") else {
        panic!()
    };
    assert!(b
        .control_points
        .iter()
        .any(|c| (c.x - 0.5).abs() > 1e-6 && (c.x - 0.5).abs() < 0.1));
}

#[test]
fn snapping_locking_and_click_to_select_on_the_canvas() {
    let _settings = settings_guard();
    let mut h = harness(Editor::new(demo_project(), None));
    step(&mut h, "2 Map");
    click(&mut h, "right");
    click(&mut h, "wall-right");
    let rect = h.state().canvas.last_rect.unwrap();
    let map = Mapping { rect };
    // Snap is on: dragging a corner close to the output edge lands exactly on it.
    h.state_mut().canvas.handles.snap = true;
    h.state_mut().editor.set_warp(
        "wall-right",
        WarpKind::Quad(lumap_core::warp::Quad {
            corners: [
                Vec2::new(0.1, 0.1),
                Vec2::new(0.9, 0.1),
                Vec2::new(0.9, 0.9),
                Vec2::new(0.1, 0.9),
            ],
        }),
    );
    h.state_mut().editor.commit();
    settle(&mut h);
    let start = map.to_screen(Vec2::new(0.1, 0.1));
    let target = map.to_screen(Vec2::new(0.005, 0.1));
    drag(&mut h, start, target);
    let WarpKind::Quad(q) = surface_warp(&h, "wall-right") else {
        panic!()
    };
    assert_eq!(q.corners[0].x, 0.0, "snapped to the left edge");

    // A locked surface ignores drags but still gets selected by clicking inside it.
    h.state_mut().editor.set_locked("wall-right", true);
    settle(&mut h);
    let before = surface_warp(&h, "wall-right");
    let c = map.to_screen(Vec2::new(0.9, 0.1));
    drag(&mut h, c, c + egui::Vec2::new(-30.0, 30.0));
    assert_eq!(surface_warp(&h, "wall-right"), before);
    // Click on the logo (drawn above wall-right) selects it.
    let logo_centre = map.to_screen(surface_warp(&h, "logo").centre());
    h.hover_at(logo_centre);
    h.drag_at(logo_centre);
    h.drop_at(logo_centre);
    settle(&mut h);
    assert_eq!(
        h.state().editor.selection,
        Selection::Surface {
            id: "logo".into(),
            mask: None
        }
    );
    // Grid toggles draw without breaking anything; zoom changes the rect.
    h.key_press(Key::G);
    settle(&mut h);
    assert!(h.state().canvas.show_grid);
    h.hover_at(rect.center());
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::Vec2::new(0.0, 120.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    settle(&mut h);
    assert!(h.state().canvas.zoom > 1.0, "{}", h.state().canvas.zoom);
    assert!(h.state().canvas.last_rect.unwrap().width() > rect.width());
    h.key_press(Key::Num0);
    settle(&mut h);
    assert_eq!(h.state().canvas.zoom, 1.0);
    h.key_press(Key::S);
    settle(&mut h);
    assert!(!h.state().canvas.handles.snap);
}

mod content_tests;
mod control_tests;
mod golive_tests;
mod looks_tests;
mod map_tests;
mod show_tests;
mod update_tests;

/// Renders the welcome screen and the editor on the demo project to `docs/images/`, through
/// the same harness the UI tests use, so the README and the website show the real app. Ignored
/// by default because it writes into the repository; run it on purpose after a visual change:
/// `WGPU_BACKEND=dx12 cargo test -p lumap-app -- --ignored render_doc_screenshots`.
#[test]
#[ignore = "writes docs/images; run on purpose after a visual change"]
fn render_doc_screenshots() {
    let _settings = settings_guard();
    let _dir = config_dir();
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/images");
    let mut h = Harness::builder()
        .with_size(egui::Vec2::new(1400.0, 860.0))
        .with_pixels_per_point(1.0)
        .renderer(egui_kittest::wgpu::WgpuTestRenderer::default())
        .build_ui_state(
            |ui, app: &mut LumapApp| app.frame(ui),
            app(Editor::new(demo_project(), None)),
        );
    h.run_steps(4);
    h.render().unwrap().save(out.join("editor.png")).unwrap();
    std::fs::create_dir_all(out.join("steps")).unwrap();
    for step in super::panels::shell::Step::ALL {
        h.state_mut().panels.step = step;
        // Setup shows a projector being set up; the other steps show the surfaces.
        h.state_mut().editor.selection = match step {
            super::panels::shell::Step::Setup => Selection::Output("left".into()),
            super::panels::shell::Step::Map | super::panels::shell::Step::Content => {
                Selection::Surface {
                    id: "wall-left".into(),
                    mask: None,
                }
            }
            super::panels::shell::Step::Show => Selection::None,
        };
        if step == super::panels::shell::Step::Show {
            // A look on every surface and a short running order, the first cue running.
            let e = &mut h.state_mut().editor;
            let look = lumap_core::looks::find("Northern lights").unwrap();
            let ids: Vec<String> = e.project.surfaces.iter().map(|s| s.id.clone()).collect();
            for id in &ids {
                look.apply(&mut e.project, id);
            }
            e.reload_sources();
            e.store_cue("Opening", 3.0);
            e.set_param("/show/master", lumap_core::params::ParamValue::Number(0.6));
            e.commit();
            e.store_cue("Warm wash", 5.0);
            e.set_param(
                "/surface/logo/opacity",
                lumap_core::params::ParamValue::Number(0.0),
            );
            e.commit();
            e.store_cue("Logo out", 1.5);
            e.cue_go(0, 0.0);
            if let Some(f) = e.cues.fade.as_mut() {
                f.duration = 0.0;
            }
        }
        h.run_steps(4);
        let name = step.label().to_lowercase();
        h.render()
            .unwrap()
            .save(out.join("steps").join(format!("{name}.png")))
            .unwrap();
    }
    h.state_mut().panels.golive = true;
    h.run_steps(4);
    h.render()
        .unwrap()
        .save(out.join("steps").join("golive.png"))
        .unwrap();
    h.state_mut().panels.golive = false;
    h.state_mut().panels.home = true;
    h.run_steps(4);
    h.render().unwrap().save(out.join("welcome.png")).unwrap();
}

#[test]
fn setup_cards_list_projectors_and_add_or_create_them() {
    let mut h = harness(Editor::new(lumap_core::project::Project::default(), None));
    assert_eq!(h.state().panels.step, super::panels::shell::Step::Setup);
    assert!(h.query_by_label("No projectors yet").is_some());
    click(&mut h, "+ projector");
    assert_eq!(h.state().editor.project.outputs.len(), 1);
    click(&mut h, "One projector per display");
    assert!(h.state().editor.project.outputs.len() >= 2);
    // The new projectors' windows are separate OS windows in the app; the harness embeds them
    // over the panels, so close them before clicking around.
    h.state_mut().editor.open_outputs.clear();
    settle(&mut h);
    let first = h.state().editor.project.outputs[0].id.clone();
    click(&mut h, &first);
    assert_eq!(h.state().editor.selection, Selection::Output(first.clone()));
    click(&mut h, "Grid");
    assert_eq!(
        h.state().editor.project.output(&first).unwrap().overlay,
        Some(lumap_core::project::Overlay::Grid)
    );
    assert!(
        h.get_all_by_label("Grid").count() >= 2,
        "the card shows the overlay"
    );
}
