//! Scan sessions against fake cameras: when a frame counts as a photo of the pattern that is up
//! (a camera that lags, one that settles, one that cannot see the projector), and a whole scan
//! through a camera that lags like a phone streamed as a webcam.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::editor::camera::CameraHub;
use lumap_core::project::{Output, Project};
use lumap_sources::live_input::camera::{CameraInfo, FakeCamera, FrameFn};
use lumap_vision::studio::{self, CameraModel, Room};
use std::sync::Mutex;

/// A projector small enough to scan quickly in a debug build.
const PW: u32 = 64;
const PH: u32 = 36;
/// The camera watching it.
const CW: u32 = 128;
const CH: u32 = 72;

fn editor_with(id: &str) -> Editor {
    let mut project = Project::default();
    project.outputs.push(Output {
        id: id.into(),
        width: PW,
        height: PH,
        enabled: true,
        blend: lumap_core::blend::EdgeBlend::none(),
        color: lumap_core::project::ColorAdjust::default(),
        placement: None,
        overlay: None,
        scan: None,
    });
    Editor::new(project, None)
}

fn frame(v: u8) -> Arc<Frame> {
    Arc::new(Frame::solid(CW, CH, [v, v, v, 255]))
}

fn session(now: Instant) -> ScanSession {
    let editor = editor_with("p");
    let dir = std::env::temp_dir().join("lumap-scan-tests");
    ScanSession::start(Kind::Full, &editor, &["p".into()], "cam", dir, now)
}

#[test]
fn thumbnails_compare_by_mean_brightness() {
    let dark = thumbnail(&frame(10));
    let light = thumbnail(&frame(30));
    assert_eq!(dark.len(), (THUMB.0 * THUMB.1) as usize);
    assert!((difference(&dark, &light) - 20.0).abs() < 1.0);
    assert_eq!(difference(&dark, &dark), 0.0);
    assert_eq!(difference(&dark, &[]), f64::MAX);
}

#[test]
fn a_frame_counts_once_the_camera_shows_the_change_and_holds_still() {
    let t0 = Instant::now();
    let mut s = session(t0);
    s.shown_at = t0;
    s.reference = Some(thumbnail(&frame(20)));
    let at = |ms| t0 + Duration::from_millis(ms);
    // The camera still shows the old picture: wait.
    assert_eq!(s.ready(&frame(20), PatternFrame::Black, at(200)), Ready::Wait);
    // The new pattern arrives 400 ms later: that is the lag; one more frame to see it hold.
    assert_eq!(s.ready(&frame(120), PatternFrame::Black, at(400)), Ready::Wait);
    assert_eq!(s.lag(), Some(Duration::from_millis(400)));
    // Auto-exposure still moving: wait; then steady: photograph.
    assert_eq!(s.ready(&frame(150), PatternFrame::Black, at(433)), Ready::Wait);
    assert_eq!(s.ready(&frame(151), PatternFrame::Black, at(466)), Ready::Collect);
    // Once collecting, the next frames count too.
    s.collected.push(frame(151));
    assert_eq!(s.ready(&frame(170), PatternFrame::Black, at(500)), Ready::Collect);
    // Later patterns: 300 ms, then one slow 2.5 s frame; the lag is the median, not the worst.
    for (start, seen_after) in [(1_000, 300), (5_000, 2_500)] {
        s.shown_at = at(start);
        s.changed_at = None;
        s.collected.clear();
        s.reference = Some(thumbnail(&frame(20)));
        assert_eq!(
            s.ready(&frame(200), PatternFrame::Black, at(start + seen_after)),
            Ready::Wait
        );
    }
    assert_eq!(s.lag(), Some(Duration::from_millis(400)));
}

#[test]
fn stripes_the_camera_cannot_see_count_after_the_lag_and_a_blind_camera_is_caught() {
    let t0 = Instant::now();
    let mut s = session(t0);
    // With no picture when the pattern went up, the first fresh one is the reference.
    s.shown_at = t0;
    assert_eq!(s.ready(&frame(50), PatternFrame::White, t0), Ready::Wait);
    assert!(s.reference.is_some());
    // Lag unknown and no change: the first pattern still counts after the first wait.
    assert_eq!(
        s.ready(&frame(50), PatternFrame::White, t0 + FIRST_WAIT),
        Ready::Collect
    );
    // Going from white to black changes nothing: this camera cannot see the projector.
    let mut blind = session(t0);
    blind.shown_at = t0;
    blind.reference = Some(thumbnail(&frame(50)));
    assert_eq!(
        blind.ready(&frame(50), PatternFrame::Black, t0 + FIRST_WAIT),
        Ready::Blind
    );
    // Once the lag is known, an unchanged picture counts after the lag and a margin.
    let mut fine = session(t0);
    fine.lag = Some(Duration::from_millis(300));
    fine.shown_at = t0;
    fine.reference = Some(thumbnail(&frame(80)));
    let bit = PatternFrame::Bit {
        axis: lumap_vision::graycode::Axis::X,
        bit: 0,
        inverted: false,
    };
    assert_eq!(
        fine.ready(&frame(80), bit, t0 + Duration::from_millis(400)),
        Ready::Wait
    );
    assert_eq!(
        fine.ready(&frame(80), bit, t0 + Duration::from_millis(600)),
        Ready::Collect
    );
}

#[test]
fn a_scan_through_a_camera_that_lags_like_a_phone_still_decodes() {
    // What the projector showed at each camera tick; the camera sees it nine ticks (about
    // 300 ms at 30 frames a second) later.
    const LAG: usize = 9;
    let shown: Arc<Mutex<Vec<Option<Arc<Frame>>>>> = Arc::new(Mutex::new(Vec::new()));
    let room = Room::flat_wall(PW, PH, CW, CH);
    let cam = CameraModel {
        width: CW,
        height: CH,
        ..CameraModel::default()
    };
    let history = Arc::clone(&shown);
    let frames: FrameFn = Arc::new(move |n| {
        let h = history.lock().unwrap();
        let i = (n as usize).saturating_sub(LAG);
        let gray = h
            .get(i)
            .cloned()
            .flatten()
            .map_or_else(|| Gray::new(PW, PH), |f| {
                Gray::from_rgba(&Rgba {
                    width: f.width,
                    height: f.height,
                    data: f.rgba.clone(),
                })
            });
        let img = studio::photograph(&room, &gray, &cam);
        Frame {
            width: img.width,
            height: img.height,
            rgba: img.data,
        }
    });
    let name = "lagging phone (scan test)";
    let fake = Arc::new(FakeCamera::stepped(
        vec![CameraInfo::new(name, name)],
        frames,
    ));
    let mut hub = CameraHub::new(Arc::clone(&fake) as Arc<_>);
    let mut editor = editor_with("p");
    let dir = tempfile::tempdir().unwrap();
    let start = Instant::now();
    let mut session = ScanSession::start(
        Kind::Full,
        &editor,
        &["p".into()],
        name,
        dir.path().to_path_buf(),
        start,
    );
    let mut now = start;
    for _ in 0..40_000 {
        shown
            .lock()
            .unwrap()
            .push(editor.scan_pattern.as_ref().map(|(_, f)| Arc::clone(f)));
        fake.advance();
        session.tick(&mut editor, &mut hub, now);
        now += Duration::from_millis(33);
        if session.finished() {
            break;
        }
        if *session.phase() == Phase::Decoding {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    assert_eq!(session.phase(), &Phase::Done, "{:?}", session.problems());
    let lag = session.lag().unwrap();
    assert!(lag >= Duration::from_millis(250), "measured lag {lag:?}");
    assert_eq!(session.adopt(&mut editor), 1);
    let scan = editor.scans.get("p").unwrap();
    assert!(scan.stats.coverage > 0.5, "{:?}", scan.stats);
    assert!(editor.scan_pattern.is_none(), "the projector is given back");
}
