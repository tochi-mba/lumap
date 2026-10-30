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

/// A frame lit on its left half (or its right half), at brightness `v`.
fn half(v: u8, left: bool) -> Arc<Frame> {
    Arc::new(Frame::from_fn(CW, CH, |x, _| {
        let lit = (x < CW / 2) == left;
        let l = if lit { v } else { v / 8 };
        [l, l, l, 255]
    }))
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
    // Shape: the same picture brighter is unchanged; the other half lit is a new pattern.
    let left = thumbnail(&half(200, true));
    assert!(reshaped(&left, &thumbnail(&half(100, true))) < 0.01);
    assert!(reshaped(&left, &thumbnail(&half(200, false))) > 1.5);
    // Flat pictures compare by brightness.
    assert!(reshaped(&dark, &light) > 0.5);
    assert!(reshaped(&dark, &dark) < 0.01);
    assert_eq!(reshaped(&dark, &[]), 1.0);
}

#[test]
fn a_frame_counts_once_the_camera_shows_the_change_and_holds_still() {
    let t0 = Instant::now();
    let mut s = session(t0);
    s.shown_at = t0;
    s.reference = Some(thumbnail(&frame(20)));
    let at = |ms| t0 + Duration::from_millis(ms);
    s.reference = Some(thumbnail(&half(200, true)));
    // The camera still shows the old picture, only darker (auto-exposure): wait.
    assert_eq!(
        s.ready(&half(120, true), PatternFrame::Black, at(200)),
        Ready::Wait
    );
    // The new pattern arrives 400 ms later: that is the lag; one more frame to see it hold.
    assert_eq!(
        s.ready(&half(120, false), PatternFrame::Black, at(400)),
        Ready::Wait
    );
    assert_eq!(s.lag(), Some(Duration::from_millis(400)));
    // Auto-exposure still moving: wait; still frames too soon after the change (a phone fading
    // the pattern in): wait; still for two frames after the fade time: photograph.
    assert_eq!(
        s.ready(&half(150, false), PatternFrame::Black, at(433)),
        Ready::Wait
    );
    assert_eq!(
        s.ready(&half(151, false), PatternFrame::Black, at(466)),
        Ready::Wait
    );
    assert_eq!(
        s.ready(&half(151, false), PatternFrame::Black, at(500)),
        Ready::Wait
    );
    assert_eq!(
        s.ready(&half(151, false), PatternFrame::Black, at(850)),
        Ready::Collect
    );
    // Once collecting, the next frames count too.
    s.collected.push(half(151, false));
    assert_eq!(
        s.ready(&half(170, false), PatternFrame::Black, at(900)),
        Ready::Collect
    );
    // Later patterns: 300 ms, then one slow 2.5 s frame; the lag is the median, not the worst.
    for (start, seen_after) in [(1_000, 300), (5_000, 2_500)] {
        s.shown_at = at(start);
        s.changed_at = None;
        s.collected.clear();
        s.reference = Some(thumbnail(&half(200, true)));
        assert_eq!(
            s.ready(
                &half(200, false),
                PatternFrame::Black,
                at(start + seen_after)
            ),
            Ready::Wait
        );
    }
    // A change sooner than half the lag is the camera adjusting, not the pattern.
    s.shown_at = at(9_000);
    s.changed_at = None;
    s.reference = Some(thumbnail(&half(200, true)));
    assert_eq!(
        s.ready(&half(200, false), PatternFrame::Black, at(9_100)),
        Ready::Wait
    );
    assert!(s.changed_at.is_none());
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
        let gray = h.get(i).cloned().flatten().map_or_else(
            || Gray::new(PW, PH),
            |f| {
                Gray::from_rgba(&Rgba {
                    width: f.width,
                    height: f.height,
                    data: f.rgba.clone(),
                })
            },
        );
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
        if matches!(session.phase(), Phase::Adjust(_)) {
            session.accept();
        }
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

/// Decodes a folder of photos saved by a scan with `LUMAP_SCAN_DEBUG` (`LUMAP_CAPTURES`, plus
/// `LUMAP_PROJECTOR=WxH`), and writes `map.png` and the numbers beside them: for tuning the
/// decoder against a real room without running the scan again.
#[test]
#[ignore = "needs a folder of real captures"]
#[allow(clippy::print_stderr)] // It reports what it decoded for a person to read.
fn decode_saved_captures() {
    let dir = std::path::PathBuf::from(std::env::var("LUMAP_CAPTURES").unwrap());
    let size = std::env::var("LUMAP_PROJECTOR").unwrap_or_else(|_| "1920x1080".into());
    let (w, h) = size.split_once('x').unwrap();
    let code = GrayCode::new(w.parse().unwrap(), h.parse().unwrap());
    let mut decoder = None;
    for i in 0..code.frames().len() {
        let img = image::open(dir.join(format!("{i:02}.png")))
            .unwrap()
            .into_luma8();
        let d = decoder.get_or_insert_with(|| Decoder::new(&code, img.width(), img.height()));
        d.add(
            i,
            &Gray {
                width: img.width(),
                height: img.height(),
                data: img.into_raw(),
            },
        );
    }
    let scan = decoder.unwrap().finish(&DecodeSettings::default());
    let c = &scan.correspondence;
    let pixels: Vec<u8> = (0..c.cam_h)
        .flat_map(|y| (0..c.cam_w).map(move |x| (x, y)))
        .flat_map(|(x, y)| match c.get(x, y) {
            Some(p) => [
                (p.x / f64::from(c.proj_w) * 255.0) as u8,
                (p.y / f64::from(c.proj_h) * 255.0) as u8,
                96,
                255,
            ],
            None => [0, 0, 0, 255],
        })
        .collect();
    image::RgbaImage::from_raw(c.cam_w, c.cam_h, pixels)
        .unwrap()
        .save(dir.join("map.png"))
        .unwrap();
    let inverse = c.invert();
    let color = image::open(dir.join("00.png")).unwrap().into_rgba8();
    let color = Rgba {
        width: color.width(),
        height: color.height(),
        data: color.into_raw(),
    };
    let eye = lumap_vision::photo::projector_eye(&inverse, &color);
    // How long each step after decoding takes on a real scan.
    let t = std::time::Instant::now();
    let grey = Gray::from_rgba(&eye);
    let _ = lumap_vision::features::canny(&lumap_vision::features::blur(&grey, 1.2), 25.0, 70.0);
    eprintln!("edges {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let found = lumap_vision::find::find_surfaces(
        &eye,
        Some(&inverse.cam),
        &lumap_vision::find::FindSettings::default(),
    );
    eprintln!("find {:?} ({} found)", t.elapsed(), found.len());
    let t = std::time::Instant::now();
    let _ = lumap_vision::snap::SnapIndex::build(&eye);
    eprintln!("snap {:?}", t.elapsed());
    image::RgbaImage::from_raw(eye.width, eye.height, eye.data)
        .unwrap()
        .save(dir.join("photo.png"))
        .unwrap();
    eprintln!(
        "valid {:.3} coverage {:.3} contrast {:.1}",
        scan.stats.valid_fraction,
        inverse.coverage(),
        scan.stats.mean_contrast
    );
}
