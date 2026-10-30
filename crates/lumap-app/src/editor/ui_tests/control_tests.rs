//! Cue list, MIDI learn, OSC feedback and the phone remote through the real UI.

#![allow(clippy::unwrap_used, clippy::float_cmp, clippy::too_many_lines)]

use super::*;
use crate::editor::Editor;
use eframe::egui::{self, accesskit::Role, Key};
use egui_kittest::kittest::Queryable;
use lumap_core::midimap::{MidiEvent, MidiKey, MidiMode};
use lumap_core::params::ParamValue;
use lumap_core::project::demo_project;

#[test]
fn cue_cards_store_go_update_rename_reorder_and_delete() {
    let mut h = harness(Editor::new(demo_project(), None));
    step(&mut h, "4 Show");
    assert!(h.query_by_label("No cues yet").is_some());
    assert!(h.query_by_label("Store a cue first").is_some());

    let field = h.get_by_role_and_label(Role::TextInput, "Cue name");
    field.focus();
    field.type_text("opening");
    settle(&mut h);
    click(&mut h, "Store this look");
    assert_eq!(h.state().editor.project.cues.len(), 1);
    assert_eq!(h.state().editor.project.cues[0].name, "opening");
    assert_eq!(h.state().editor.project.cues[0].fade_seconds, 2.0);
    assert!(h.state().panels.cue_name.is_empty(), "the name box clears");

    // A second cue with blackout on, stored after the first.
    h.state_mut()
        .editor
        .set_param("/show/blackout", ParamValue::Bool(true));
    h.state_mut().editor.commit();
    click(&mut h, "Store this look");
    assert_eq!(h.state().editor.project.cues.len(), 2);
    let second = h.state().editor.project.cues[1].id.clone();
    assert_eq!(second, "cue");
    assert_eq!(
        h.state().editor.project.cues[1].values["/show/blackout"],
        ParamValue::Bool(true)
    );
    assert!(h.query_by_label("Next: 1 · opening").is_some());

    // GO the first cue by its own button: blackout fades back off.
    click(&mut h, "GO opening");
    assert_eq!(h.state().editor.cues.current, Some(0));
    assert!(h.query_by_label("fading").is_some());
    assert!(
        h.query_by_label("live").is_some(),
        "the running cue says so"
    );
    h.state_mut().editor.cues.fade.as_mut().unwrap().duration = 0.0;
    settle(&mut h);
    assert!(!h.state().editor.project.blackout);
    assert!(h.query_by_label("Next: 2 · cue").is_some());

    // Space GOs the next cue.
    h.key_press(Key::Space);
    settle(&mut h);
    assert_eq!(h.state().editor.cues.current, Some(1));
    click(&mut h, "Stop fade");
    assert!(h.state().editor.cues.fade.is_none());

    // The selected (just fired) cue: update it with the live look, rename it, change its fade.
    h.state_mut()
        .editor
        .set_param("/show/master", ParamValue::Number(0.25));
    h.state_mut().editor.commit();
    click(&mut h, "Update cue");
    assert_eq!(
        h.state().editor.project.cues[1].values["/show/master"],
        ParamValue::Number(0.25)
    );
    let field = h.get_by_role_and_label(Role::TextInput, "Rename");
    field.focus();
    field.type_text("finale");
    settle(&mut h);
    h.key_press(Key::Enter);
    settle(&mut h);
    assert_eq!(h.state().editor.project.cues[1].name, "finale");
    assert_eq!(
        h.state().editor.project.cues[1].id,
        second,
        "OSC keeps its id"
    );
    assert!(h.query_by_label("GO finale").is_some());

    // Drag the second card by its dots onto the first: it moves up, and stays the running cue.
    let finale = h.get_by_label("finale").rect();
    let opening = h.get_by_label("opening").rect();
    let grip = egui::pos2(finale.left() - 30.0, finale.center().y);
    drag(&mut h, grip, opening.center());
    assert_eq!(h.state().editor.project.cues[0].id, second);
    assert_eq!(
        h.state().editor.cues.current,
        Some(0),
        "the running cue moved with it"
    );
    assert_eq!(h.state().editor.selected_cue, Some(0));

    // Right-click: move it back down.
    h.get_by_label("finale").click_secondary();
    settle(&mut h);
    click(&mut h, "Move down");
    assert_eq!(h.state().editor.project.cues[1].id, second);
    assert_eq!(h.state().editor.cues.current, Some(1));
    click(&mut h, "Delete cue");
    assert_eq!(h.state().editor.project.cues.len(), 1);
    assert_eq!(h.state().editor.project.cues[0].id, "opening");

    // The big GO fires the next cue; Space while typing does not.
    click(&mut h, "GO");
    assert_eq!(h.state().editor.cues.current, Some(0));
    let field = h.get_by_role_and_label(Role::TextInput, "Cue name");
    field.focus();
    field.type_text(" ");
    settle(&mut h);
    h.key_press(Key::Space);
    settle(&mut h);
    assert_eq!(
        h.state().editor.cues.current,
        Some(0),
        "typing a space is not GO"
    );
    // Clicking a card selects it.
    h.state_mut().editor.selected_cue = None;
    settle(&mut h);
    click(&mut h, "opening");
    assert_eq!(h.state().editor.selected_cue, Some(0));
}

#[test]
fn control_section_learns_midi_and_runs_feedback_and_remote() {
    let _settings = settings_guard();
    let mut h = harness(Editor::new(demo_project(), None));
    step(&mut h, "4 Show");
    assert!(h.query_by_label("Control").is_some());
    assert!(h.query_by_label("Always on").is_some(), "the keyboard card");
    assert_eq!(
        h.get_all_by_label("Off").count(),
        3,
        "phone, OSC and MIDI are off"
    );
    click(&mut h, "Refresh MIDI ports");
    // Opening no port is fine; opening a nonexistent one reports an error.
    click(&mut h, "Open MIDI port");
    assert!(h.state().editor.midi.is_none());
    assert!(h.state().panels.error.is_none());
    h.state_mut().panels.midi_port = "no such midi port".into();
    click(&mut h, "Open MIDI port");
    assert!(h.state().panels.error.is_some());
    assert!(h.state().editor.midi.is_none());
    h.state_mut().panels.midi_port.clear();
    click(&mut h, "Open MIDI port");
    assert!(h.state().panels.error.is_none());

    // Learn the master fader from the typed address, then feed a CC message.
    click(&mut h, "Learn fader");
    assert_eq!(
        h.state().editor.midi_learn,
        Some(("/show/master".to_string(), MidiMode::Fader))
    );
    assert!(h
        .query_by_label("move a control for /show/master")
        .is_some());
    click(&mut h, "Cancel learn");
    assert!(h.state().editor.midi_learn.is_none());
    click(&mut h, "Learn button");
    let cc = MidiEvent {
        key: MidiKey::Cc {
            channel: 0,
            controller: 7,
        },
        value: 127,
    };
    h.state_mut().editor.handle_midi_event(cc, 0.0);
    settle(&mut h);
    assert_eq!(h.state().editor.project.midi.mappings.len(), 1);
    assert!(h
        .query_by_label("Last message: CC 7 · channel 1 = 127")
        .is_some());
    assert!(h
        .query_by_label("CC 7 · channel 1 → /show/master (button)")
        .is_some());
    click(&mut h, "Forget /show/master");
    assert!(h.state().editor.project.midi.mappings.is_empty());

    // The surface inspector has a one-click learn for opacity.
    h.state_mut().editor.selection = Selection::Surface {
        id: "logo".into(),
        mask: None,
    };
    settle(&mut h);
    click(&mut h, "Precise");
    click(&mut h, "MIDI learn opacity");
    assert_eq!(
        h.state().editor.midi_learn,
        Some(("/surface/logo/opacity".to_string(), MidiMode::Fader))
    );
    h.state_mut().editor.handle_midi_event(cc, 0.0);
    h.state_mut()
        .editor
        .handle_midi_event(MidiEvent { value: 0, ..cc }, 0.0);
    settle(&mut h);
    assert_eq!(
        h.state().editor.project.surface("logo").unwrap().opacity,
        0.0
    );

    // OSC feedback: a bad target errors, a loopback target is accepted and saved.
    h.state_mut().editor.selection = Selection::None;
    settle(&mut h);
    h.state_mut().panels.feedback_target = "not a host".into();
    click(&mut h, "Apply feedback");
    assert!(h.state().panels.error.is_some());
    assert!(h.state().editor.feedback.is_none());
    h.state_mut().panels.feedback_target = "127.0.0.1:1".into();
    click(&mut h, "Apply feedback");
    assert!(h.state().editor.feedback.is_some());
    assert_eq!(h.state().settings.osc_feedback, "127.0.0.1:1");
    assert!(h.query_by_label("Sending changes to 127.0.0.1:1").is_some());
    h.state_mut().panels.feedback_target.clear();
    click(&mut h, "Apply feedback");
    assert!(h.state().editor.feedback.is_none());

    // Phone remote on an ephemeral port: the URL and a QR code appear, Stop removes them.
    assert!(h
        .query_by_label("GO, blackout and the master fader from any phone on the same Wi-Fi.")
        .is_some());
    h.state_mut().panels.remote_port = 0;
    click(&mut h, "Start remote");
    assert!(h.state().editor.remote.is_none(), "port 0 means off");
    let blocker = lumap_control::web::RemoteServer::bind(0).unwrap();
    let free = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    h.state_mut().panels.remote_port = free;
    click(&mut h, "Start remote");
    assert!(
        h.state().editor.remote.is_some(),
        "remote started on {free}"
    );
    assert_eq!(h.state().settings.remote_port, free);
    let url = h.state().editor.remote_url().unwrap();
    assert!(h.query_by_label(url.as_str()).is_some());
    assert!(h
        .query_by_label("Scan with a phone on the same Wi-Fi:")
        .is_some());
    assert!(h.query_by_label("On").is_some());
    click(&mut h, "Stop remote");
    assert!(h.state().editor.remote.is_none());
    h.state_mut().panels.remote_port = blocker.addr().port();
    click(&mut h, "Start remote");
    assert!(h.state().panels.error.is_some(), "a taken port is reported");
    assert!(h.state().editor.remote.is_none());
    drop(blocker);

    // OSC: listen on a free port, then stop; the port is remembered.
    let free = {
        let l = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    h.state_mut().panels.osc_port = free;
    click(&mut h, "Listen");
    assert!(h.state().editor.osc.is_some());
    assert_eq!(h.state().settings.osc_port, free);
    assert!(h.query_by_label(&format!("Port {free}")).is_some());
    click(&mut h, "OSC addresses");
    assert!(h.query_by_label("/cue/<id>/go").is_some());
    click(&mut h, "Stop OSC");
    assert!(h.state().editor.osc.is_none());
    assert_eq!(h.state().settings.osc_port, 0);
}

#[test]
fn settings_open_controllers_and_show_launch_takes_a_remote_port() {
    let _settings = settings_guard();
    let dir = config_dir();
    let free = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let settings = crate::settings::Settings {
        midi_port: "no such midi port".into(),
        osc_feedback: "127.0.0.1:1".into(),
        remote_port: free,
        ..crate::settings::Settings::default()
    };
    let mut a = app(Editor::new(demo_project(), None));
    a.apply_settings(settings);
    assert!(
        a.editor.midi.is_none(),
        "a missing port is logged, not fatal"
    );
    assert_eq!(a.panels.midi_port, "no such midi port");
    assert!(a.editor.feedback.is_some());
    assert!(a.editor.remote.is_some());
    assert_eq!(a.panels.remote_port, free);
    drop(a);

    // A fresh port: Windows keeps the one the dropped app bound busy for a while.
    let fresh = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let launch = Launch {
        project: None,
        osc_port: 0,
        show: true,
        restart_on_stall: None,
        remote_port: Some(fresh),
    };
    let app = LumapApp::new(
        Editor::new(demo_project(), None),
        &launch,
        monitors(),
        wgpu::TextureFormat::Rgba8Unorm,
    );
    assert!(app.editor.remote.is_some());
    let bad = Launch {
        remote_port: Some(app.editor.remote.as_ref().unwrap().addr().port()),
        ..launch
    };
    let second = LumapApp::new(
        Editor::new(demo_project(), None),
        &bad,
        monitors(),
        wgpu::TextureFormat::Rgba8Unorm,
    );
    assert!(second.editor.remote.is_none(), "a taken port is logged");
    let _ = dir;
}

#[test]
fn stage_view_selects_drags_and_the_calibration_tools_work() {
    use lumap_core::project::{Overlay, Placement};
    let mut h = harness(Editor::new(demo_project(), None));
    // Nothing placed: the stage still shows the monitors (no crash) and the hint.
    h.key_press(Key::T);
    settle(&mut h);
    assert!(h.state().canvas.stage);
    assert!(h.state().canvas.stage_rects.is_empty());
    h.state_mut().editor.edit(|p| {
        p.outputs[0].placement = Some(Placement {
            monitor: "DISPLAY1".into(),
            x: 0,
            y: 0,
        });
        p.outputs[1].placement = Some(Placement {
            monitor: "DISPLAY2".into(),
            x: 1728,
            y: 0,
        });
    });
    settle(&mut h);
    assert_eq!(h.state().canvas.stage_rects.len(), 2);
    // Click the right output to select it.
    let right = h.state().canvas.stage_rects[1].1;
    click_at(&mut h, right.center());
    settle(&mut h);
    assert_eq!(
        h.state().editor.selection,
        Selection::Output("right".into())
    );
    assert_eq!(h.state().canvas.output.as_deref(), Some("right"));
    // Selecting swaps the inspector, which can resize the stage: measure again.
    let right = h.state().canvas.stage_rects[1].1;
    // Drag it 40 screen points right: the placement moves by 40 / scale desktop pixels.
    let scale = right.width() / 1920.0;
    drag(
        &mut h,
        right.center(),
        right.center() + egui::vec2(40.0, 0.0),
    );
    let p = h
        .state()
        .editor
        .project
        .output("right")
        .unwrap()
        .placement
        .clone()
        .unwrap();
    let expected = (40.0 / scale).round() as i32;
    assert!(
        (p.x - 1728 - expected).abs() <= 2,
        "{} vs {}",
        p.x,
        1728 + expected
    );
    assert!(h.state().canvas.stage_drag.is_none());
    // Click empty space clears the selection.
    click_at(&mut h, egui::pos2(right.max.x + 2.0, right.max.y + 2.0));
    settle(&mut h);
    // Suggest blends from the overlap, through the Setup step's inspector.
    h.state_mut().editor.selection = Selection::None;
    step(&mut h, "1 Setup");
    click(&mut h, "Suggest blends");
    assert!(
        h.state()
            .editor
            .project
            .output("left")
            .unwrap()
            .blend
            .right
            .width
            > 0.0
    );
    // Back to the output view; overlay combo and gamma keys on the shown output.
    click(&mut h, "Stage view");
    assert!(!h.state().canvas.stage);
    click(&mut h, "left");
    click(&mut h, "Crosshair");
    assert_eq!(
        h.state().editor.project.output("left").unwrap().overlay,
        Some(Overlay::Crosshair)
    );
    click(&mut h, "Gamma match");
    assert_eq!(
        h.state().editor.project.output("left").unwrap().overlay,
        Some(Overlay::GreySteps)
    );
    let g = h.state().editor.project.output("left").unwrap().blend.gamma;
    h.key_press(Key::CloseBracket);
    settle(&mut h);
    assert!((h.state().editor.project.output("left").unwrap().blend.gamma - g - 0.05).abs() < 1e-9);
    h.key_press(Key::OpenBracket);
    settle(&mut h);
    assert!((h.state().editor.project.output("left").unwrap().blend.gamma - g).abs() < 1e-9);
    click(&mut h, "Off");
    assert_eq!(
        h.state().editor.project.output("left").unwrap().overlay,
        None
    );
    // Show mode ignores the stage and gamma keys.
    h.state_mut().editor.enter_show_mode();
    h.key_press(Key::T);
    h.key_press(Key::CloseBracket);
    settle(&mut h);
    assert!(!h.state().canvas.stage);
    assert!((h.state().editor.project.output("left").unwrap().blend.gamma - g).abs() < 1e-9);
}
