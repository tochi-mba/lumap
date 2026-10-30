//! Go live through the real UI: the check before show mode, its one-click fixes, and going live
//! when everything is ticked.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::editor::panels::shell::Step;
use crate::editor::Editor;
use egui_kittest::kittest::Queryable;
use lumap_core::project::{demo_project, Playback, Source, SourceKind};

#[test]
fn go_live_checks_the_show_and_fixes_what_it_can() {
    let _settings = settings_guard();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.lumap.toml");
    let mut h = harness(Editor::new(demo_project(), None));
    click(&mut h, "Go live");
    assert!(!h.state().editor.show_mode, "the check comes first");
    assert!(h.query_by_label("Before you go live").is_some());
    // Two floating projectors and one spare display: Setup decides where they go.
    assert!(h
        .query_by_label("left and right have no display yet")
        .is_some());
    click(&mut h, "Open Setup");
    assert_eq!(h.state().panels.step, Step::Setup);
    assert!(!h.state().panels.golive);

    // Test patterns are not content: the fix goes to the Content step.
    click(&mut h, "Go live");
    assert!(h
        .query_by_label("3 surfaces still show a test pattern")
        .is_some());
    click(&mut h, "Choose content");
    assert_eq!(h.state().panels.step, Step::Content);
    assert!(!h.state().panels.golive);
    let look = lumap_core::looks::find("Campfire").unwrap();
    let ids: Vec<String> = h
        .state()
        .editor
        .project
        .surfaces
        .iter()
        .map(|s| s.id.clone())
        .collect();
    for id in &ids {
        look.apply(&mut h.state_mut().editor.project, id);
    }
    h.state_mut().editor.reload_sources();

    // Video without ffmpeg sends you to Settings.
    h.state_mut().panels.ffmpeg_ok = false;
    h.state_mut().editor.project.sources.push(Source {
        id: "clip".into(),
        kind: SourceKind::Video {
            path: "clip.mp4".into(),
            playback: Playback::default(),
        },
    });
    click(&mut h, "Go live");
    assert!(h
        .query_by_label("Video needs ffmpeg, and lumap cannot find it")
        .is_some());
    click(&mut h, "Open Settings");
    assert!(h.state().panels.settings_open);
    assert!(!h.state().panels.golive);
    click(&mut h, "Close settings");
    h.state_mut()
        .editor
        .project
        .sources
        .retain(|s| s.id != "clip");

    // With one projector off in show, the other fits the spare display: placed in place.
    h.state_mut().editor.project.outputs[1].enabled = false;
    click(&mut h, "Go live");
    assert!(h.query_by_label("left has no display yet").is_some());
    click(&mut h, "Place on free displays");
    let placement = h
        .state()
        .editor
        .project
        .output("left")
        .unwrap()
        .placement
        .clone()
        .unwrap();
    assert_eq!(placement.monitor, "DISPLAY2");
    assert!(h.query_by_label("1 projector on its display").is_some());
    assert!(
        h.state().panels.golive,
        "fixes in place keep the sheet open"
    );

    // A first cue, then save: every line ticks.
    click(&mut h, "Store the first cue");
    assert_eq!(h.state().editor.project.cues.len(), 1);
    assert!(h.query_by_label("1 cue ready for GO").is_some());
    fake(&mut h, |d| d.save = Some(path.clone()));
    click(&mut h, "Save the show");
    assert!(path.exists());
    assert!(h
        .query_by_label("Saved, and autosaved while you work")
        .is_some());
    assert!(h.query_by_label("Ready to go live").is_some());
    assert!(h.query_by_label("Go live anyway").is_none());

    // Not yet closes it; Go live now goes live.
    click(&mut h, "Not yet");
    assert!(!h.state().panels.golive);
    click(&mut h, "Go live");
    click(&mut h, "Go live now");
    assert!(h.state().editor.show_mode);
    assert!(!h.state().panels.golive);
    assert_eq!(h.state().editor.open_outputs, vec!["left".to_string()]);
}
