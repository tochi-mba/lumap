//! The routing table: every route, every refusal, and the wire format of every action.

use super::routes::{dispatch, Dispatch};
use super::{route, Action, LabRequest, Pattern, ScanImage, UiEvent};
use serde_json::json;

fn ok(method: &str, path: &str, body: &str) -> LabRequest {
    route(method, path, body.as_bytes()).unwrap()
}

fn refused(method: &str, path: &str, body: &str) -> (u16, String) {
    route(method, path, body.as_bytes()).unwrap_err()
}

fn status(method: &str, path: &str, body: &str) -> u16 {
    refused(method, path, body).0
}

#[test]
fn every_get_route() {
    assert_eq!(ok("GET", "/lab/state", ""), LabRequest::State);
    assert_eq!(
        ok("GET", "/lab/state", "a GET body is ignored"),
        LabRequest::State
    );
    assert_eq!(
        ok("GET", "/lab/camera.png", ""),
        LabRequest::Camera {
            settle_ms: 300,
            frames: 3
        }
    );
    assert_eq!(
        ok("GET", "/lab/camera.png?settle_ms=0&frames=60", ""),
        LabRequest::Camera {
            settle_ms: 0,
            frames: 60
        }
    );
    assert_eq!(
        ok("GET", "/lab/camera.png?&frames=1&", ""),
        LabRequest::Camera {
            settle_ms: 300,
            frames: 1
        }
    );
    assert_eq!(ok("GET", "/lab/camera.json", ""), LabRequest::CameraInfo);
    assert_eq!(
        ok("GET", "/lab/projector/left.png", ""),
        LabRequest::Projector { id: "left".into() }
    );
    assert_eq!(ok("GET", "/lab/ui.png", ""), LabRequest::Ui);
    assert_eq!(ok("GET", "/lab/ui/tree", ""), LabRequest::UiTree);
    for which in ScanImage::ALL {
        assert_eq!(
            ok("GET", &format!("/lab/scan/p_2-b/{}.png", which.name()), ""),
            LabRequest::ScanImage {
                projector: "p_2-b".into(),
                which
            }
        );
    }
    assert_eq!(
        ok("GET", "/lab/found/left", ""),
        LabRequest::Found {
            projector: "left".into()
        }
    );
    assert_eq!(
        ok("GET", "/lab/vision/shadow_mask.png", ""),
        LabRequest::Vision {
            name: "shadow_mask".into()
        }
    );
}

#[test]
fn every_post_route() {
    assert_eq!(
        ok("POST", "/lab/act", r#"[{"do":"demo"}]"#),
        LabRequest::Act(vec![Action::Demo])
    );
    assert_eq!(
        ok(
            "POST",
            "/lab/act",
            " \n{\"actions\":[{\"do\":\"undo\"},{\"do\":\"redo\"}]}"
        ),
        LabRequest::Act(vec![Action::Undo, Action::Redo])
    );
    assert_eq!(
        ok("POST", "/lab/ui", r#"{"kind":"click","label":"Map"}"#),
        LabRequest::UiInput(UiEvent::Click {
            label: "Map".into(),
            nth: None
        })
    );
    assert_eq!(
        ok("POST", "/lab/record", r#"{"seconds":5}"#),
        LabRequest::Record { seconds: 5.0 }
    );
    assert_eq!(
        ok("POST", "/lab/record", r#"{"seconds":600}"#),
        LabRequest::Record { seconds: 600.0 }
    );
    for every in ["", " \r\n", "{}"] {
        assert_eq!(
            ok("POST", "/lab/scan", every),
            LabRequest::Scan { projector: None },
            "{every:?}"
        );
    }
    assert_eq!(
        ok("POST", "/lab/scan", r#"{"projector":"left"}"#),
        LabRequest::Scan {
            projector: Some("left".into())
        }
    );
    assert_eq!(ok("POST", "/lab/realign", ""), LabRequest::Realign);
}

#[test]
fn unknown_paths_and_wrong_methods() {
    for path in [
        "/",
        "/lab",
        "/lab/",
        "/lab/state/",
        "/lab/nope",
        "/api/state",
        "/lab/ping",
        "/lab/projector/left",
        "/lab/projector/left.jpg",
        "/lab/scan/left/photo",
        "/lab/scan/left/selfie.png",
        "/lab/scan/left/photo.png/more",
        "/lab/found",
        "/lab/vision/edges",
    ] {
        assert_eq!(status("GET", path, ""), 404, "{path}");
    }
    assert!(refused("GET", "/lab/nope", "").1.contains("/lab/nope"));
    assert_eq!(status("POST", "/lab/state", ""), 405);
    assert_eq!(status("GET", "/lab/act", ""), 405);
    assert_eq!(status("PUT", "/lab/act", "[]"), 405);
    assert_eq!(status("HEAD", "/lab/ui.png", ""), 405);
    assert_eq!(
        refused("GET", "/lab/record", "").1,
        "/lab/record answers POST, not GET"
    );
}

#[test]
fn dot_dot_and_bad_ids_are_refused() {
    for path in [
        "/lab/../secret",
        "/lab/projector/..png",
        "/lab/vision/a..b.png",
        "/lab/scan/../photo.png",
    ] {
        assert!(refused("GET", path, "").1.contains("`..`"), "{path}");
    }
    for path in [
        "/lab/projector/.png",
        "/lab/projector/my%20projector.png",
        "/lab/scan/a%2Fb/map.png",
        "/lab/found/a.b",
        "/lab/vision/x y.png",
    ] {
        assert_eq!(status("GET", path, ""), 400, "{path}");
    }
    assert!(refused("POST", "/lab/scan", r#"{"projector":"a/b"}"#)
        .1
        .contains("bad projector id"));
}

#[test]
fn queries_are_checked() {
    for query in [
        "frames=0",
        "frames=61",
        "frames=-1",
        "settle_ms=10001",
        "settle_ms=soon",
        "settle_ms",
        "exposure=1",
    ] {
        assert_eq!(
            status("GET", &format!("/lab/camera.png?{query}"), ""),
            400,
            "{query}"
        );
    }
    assert!(refused("GET", "/lab/camera.png?frames=0", "")
        .1
        .contains("frames must be a whole number from 1 to 60"));
    assert_eq!(
        refused("GET", "/lab/state?x=1", "").1,
        "/lab/state takes no query parameter `x`"
    );
}

#[test]
fn bodies_are_checked() {
    let bad = refused("POST", "/lab/act", "[{").1;
    assert!(bad.starts_with("bad JSON: ") && bad.contains("line 1"), "{bad}");
    assert!(refused("POST", "/lab/act", r#"[{"do":"fly"}]"#)
        .1
        .contains("unknown variant `fly`"));
    assert!(
        refused("POST", "/lab/act", r#"[{"do":"go","cue_id":"c1"}]"#)
            .1
            .contains("unknown field `cue_id`"),
        "typos are caught, not ignored"
    );
    for body in [
        r#"{"actions":[{"do":"fly"}]}"#,
        r#"{"steps":[]}"#,
        "[]",
        "",
    ] {
        assert_eq!(status("POST", "/lab/act", body), 400, "{body:?}");
    }
    assert_eq!(refused("POST", "/lab/act", "[]").1, "no actions to run");
    assert_eq!(status("POST", "/lab/ui", r#"{"kind":"tickle"}"#), 400);
    for body in ["{}", r#"{"seconds":0}"#, r#"{"seconds":600.5}"#] {
        assert_eq!(status("POST", "/lab/record", body), 400, "{body}");
    }
    assert_eq!(status("POST", "/lab/scan", "[1"), 400);
    assert_eq!(
        status("POST", "/lab/scan", r#"{"projector":"left","fast":true}"#),
        400
    );
}

#[test]
fn actions_are_checked() {
    let act = |actions: &str| route("POST", "/lab/act", actions.as_bytes());
    assert!(act(r#"[{"do":"step","step":1}]"#).is_ok());
    assert!(act(r#"[{"do":"step","step":4}]"#).is_ok());
    assert_eq!(act(r#"[{"do":"step","step":0}]"#).unwrap_err().0, 400);
    assert_eq!(
        act(r#"[{"do":"demo"},{"do":"step","step":5}]"#).unwrap_err(),
        (400, "actions[1]: step must be 1 to 4, not 5".into())
    );
    assert!(act(r#"[{"do":"param","address":"/show/master","value":0.5}]"#).is_ok());
    assert!(act(r#"[{"do":"param","address":"show master","value":0.5}]"#)
        .unwrap_err()
        .1
        .contains("must start with '/'"));
    assert!(act(r#"[{"do":"store_cue","name":"Intro","fade":2}]"#).is_ok());
    assert!(act(r#"[{"do":"store_cue","name":"Intro","fade":-1}]"#).is_err());
    let checker = |size: u32| {
        act(&format!(
            r#"[{{"do":"show_pattern","projector":"left","pattern":{{"kind":"checker","size":{size}}}}}]"#
        ))
    };
    assert!(checker(8).is_ok());
    assert!(checker(0).is_err());
    assert!(act(r#"[{"do":"wait","ms":15000},{"do":"wait","ms":5000}]"#).is_ok());
    assert!(act(r#"[{"do":"wait","ms":15000},{"do":"wait","ms":5001}]"#)
        .unwrap_err()
        .1
        .contains("20001 ms"));
    let huge = format!(r#"[{{"do":"wait","ms":{}}},{{"do":"wait","ms":9}}]"#, u64::MAX);
    assert!(act(&huge).is_err(), "adding waits does not overflow");
}

#[test]
fn ping_is_for_the_server_and_the_rest_for_the_app() {
    assert_eq!(dispatch("GET", "/lab/ping", b""), Ok(Dispatch::Ping));
    assert_eq!(dispatch("GET", "/lab/ping?t=1", b""), Ok(Dispatch::Ping));
    assert_eq!(
        dispatch("POST", "/lab/ping", b"").unwrap_err(),
        (405, "/lab/ping answers GET, not POST".into())
    );
    assert_eq!(
        dispatch("GET", "/lab/state", b""),
        Ok(Dispatch::App(LabRequest::State))
    );
    assert_eq!(dispatch("GET", "/lab/nope", b"").unwrap_err().0, 404);
}

#[test]
fn the_wire_format_of_every_action() {
    let wire = json!([
        {"do": "open", "path": "show.lumap.toml"},
        {"do": "demo"},
        {"do": "new"},
        {"do": "save"},
        {"do": "save", "path": "copy.lumap.toml"},
        {"do": "step", "step": 2},
        {"do": "select", "what": "surface", "id": "wall"},
        {"do": "add_surface", "projector": "left"},
        {"do": "set_corners", "surface": "wall",
         "corners": [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]]},
        {"do": "apply_look", "look": "Aurora"},
        {"do": "param", "address": "/show/master", "value": 0.5},
        {"do": "store_cue", "name": "Intro", "fade": 2.0},
        {"do": "go"},
        {"do": "go", "cue": "c2"},
        {"do": "identify"},
        {"do": "overlay", "projector": "left", "overlay": "grid"},
        {"do": "show_pattern", "projector": "left", "pattern": {"kind": "solid", "rgb": [255, 255, 255]}},
        {"do": "show_pattern", "projector": "left", "pattern": {"kind": "grid"}},
        {"do": "show_pattern", "projector": "left", "pattern": {"kind": "checker", "size": 32}},
        {"do": "show_pattern", "projector": "left", "pattern": {"kind": "png", "path": "a.png"}},
        {"do": "show_pattern", "projector": "left", "pattern": {"kind": "clear"}},
        {"do": "open_window", "projector": "left"},
        {"do": "close_window", "projector": "left"},
        {"do": "go_live"},
        {"do": "leave_live"},
        {"do": "undo"},
        {"do": "redo"},
        {"do": "wait", "ms": 250},
        {"do": "camera", "device": "Integrated Webcam"},
    ]);
    let pattern = |pattern| Action::ShowPattern {
        projector: "left".into(),
        pattern,
    };
    let expected = vec![
        Action::Open {
            path: "show.lumap.toml".into(),
        },
        Action::Demo,
        Action::New,
        Action::Save { path: None },
        Action::Save {
            path: Some("copy.lumap.toml".into()),
        },
        Action::Step { step: 2 },
        Action::Select {
            what: "surface".into(),
            id: Some("wall".into()),
        },
        Action::AddSurface {
            projector: "left".into(),
        },
        Action::SetCorners {
            surface: "wall".into(),
            corners: [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]],
        },
        Action::ApplyLook {
            look: "Aurora".into(),
            surface: None,
        },
        Action::Param {
            address: "/show/master".into(),
            value: json!(0.5),
        },
        Action::StoreCue {
            name: "Intro".into(),
            fade: 2.0,
        },
        Action::Go { cue: None },
        Action::Go {
            cue: Some("c2".into()),
        },
        Action::Identify,
        Action::Overlay {
            projector: "left".into(),
            overlay: Some("grid".into()),
        },
        pattern(Pattern::Solid { rgb: [255; 3] }),
        pattern(Pattern::Grid),
        pattern(Pattern::Checker { size: 32 }),
        pattern(Pattern::Png {
            path: "a.png".into(),
        }),
        pattern(Pattern::Clear),
        Action::OpenWindow {
            projector: "left".into(),
        },
        Action::CloseWindow {
            projector: "left".into(),
        },
        Action::GoLive,
        Action::LeaveLive,
        Action::Undo,
        Action::Redo,
        Action::Wait { ms: 250 },
        Action::Camera {
            device: "Integrated Webcam".into(),
        },
    ];
    let actions: Vec<Action> = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(actions, expected);
    assert_eq!(
        ok("POST", "/lab/act", &wire.to_string()),
        LabRequest::Act(expected.clone())
    );
    let again: Vec<Action> =
        serde_json::from_value(serde_json::to_value(&actions).unwrap()).unwrap();
    assert_eq!(again, expected, "what serialises reads back the same");
}

#[test]
fn the_wire_format_of_every_ui_event() {
    let wire = json!([
        {"kind": "click", "label": "Map"},
        {"kind": "click", "label": "Delete", "nth": 1},
        {"kind": "click_at", "x": 10.5, "y": 20.0},
        {"kind": "type", "text": "Wall"},
        {"kind": "type", "label": "Name", "text": "Wall"},
        {"kind": "key", "key": "Enter"},
        {"kind": "key", "key": "Z", "modifiers": ["ctrl", "shift"]},
        {"kind": "drag", "from": [1.0, 2.0], "to": [3.0, 4.0]},
        {"kind": "scroll", "x": 5.0, "y": 6.0, "dy": -120.0},
    ]);
    let events: Vec<UiEvent> = serde_json::from_value(wire).unwrap();
    assert_eq!(
        events,
        vec![
            UiEvent::Click {
                label: "Map".into(),
                nth: None
            },
            UiEvent::Click {
                label: "Delete".into(),
                nth: Some(1)
            },
            UiEvent::ClickAt { x: 10.5, y: 20.0 },
            UiEvent::Type {
                label: None,
                text: "Wall".into()
            },
            UiEvent::Type {
                label: Some("Name".into()),
                text: "Wall".into()
            },
            UiEvent::Key {
                key: "Enter".into(),
                modifiers: vec![]
            },
            UiEvent::Key {
                key: "Z".into(),
                modifiers: vec!["ctrl".into(), "shift".into()]
            },
            UiEvent::Drag {
                from: [1.0, 2.0],
                to: [3.0, 4.0]
            },
            UiEvent::Scroll {
                x: 5.0,
                y: 6.0,
                dy: -120.0
            },
        ]
    );
    let again: Vec<UiEvent> =
        serde_json::from_value(serde_json::to_value(&events).unwrap()).unwrap();
    assert_eq!(again, events);
}

#[test]
fn requests_serialise_for_logs_and_read_back() {
    let requests = vec![
        LabRequest::State,
        LabRequest::Act(vec![Action::Demo]),
        LabRequest::Camera {
            settle_ms: 300,
            frames: 3,
        },
        LabRequest::ScanImage {
            projector: "left".into(),
            which: ScanImage::Footprint,
        },
        LabRequest::UiInput(UiEvent::ClickAt { x: 1.0, y: 2.0 }),
    ];
    let wire = serde_json::to_value(&requests).unwrap();
    assert_eq!(wire[0], json!("state"));
    assert_eq!(
        wire[2],
        json!({"camera": {"settle_ms": 300, "frames": 3}})
    );
    assert_eq!(wire[3]["scan_image"]["which"], json!("footprint"));
    let again: Vec<LabRequest> = serde_json::from_value(wire).unwrap();
    assert_eq!(again, requests);
}
