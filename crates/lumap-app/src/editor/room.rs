//! Room content: effects drawn from a projector's scan and the live camera (Trace the room,
//! Letters, Shadow fill, Shadow monsters, Snow that settles, Bouncing light, Marble run), and
//! Paint, drawn on the wall with a torch, a laser or a pinching hand.
//!
//! The project names these as `room` and `paint` sources; `lumap-sources` cannot build them (they
//! need the scan and the camera), so the app attaches a live source for each one here. Each runs
//! on its own thread at a third of the projector's resolution and the GPU scales it up, so the
//! render loop only ever picks up the newest frame.

use super::scan::ScanData;
use super::vision::Shared as VisionShared;
use lumap_core::geom::Vec2;
use lumap_core::project::{Brush, PaintInput, RoomEffect, SourceKind};
use lumap_sources::{Frame, FrameSource};
use lumap_vision::{features, letters, paint, physics, shadow, trace, Gray, Rgba};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// Frame rate room content renders at.
const FPS: f64 = 30.0;
/// Fraction of the projector's resolution room content renders at.
const SCALE: u32 = 3;
/// What Letters spells when nothing else is set.
const POEM: &str = "THE WALL IS LISTENING TO YOU TONIGHT";

/// What a room or paint source reads every frame: the scan (once loaded) and the camera's
/// reports (once the camera runs).
#[derive(Default, Clone)]
pub struct Inputs {
    /// The projector's scan.
    pub scan: Option<Arc<ScanData>>,
    /// The camera's live reports.
    pub vision: Option<Arc<VisionShared>>,
}

/// Paint canvases cleared from OSC, MIDI, a gesture or a button, by source id.
fn clear_counters() -> &'static Mutex<HashMap<String, Arc<AtomicU64>>> {
    static COUNTERS: OnceLock<Mutex<HashMap<String, Arc<AtomicU64>>>> = OnceLock::new();
    COUNTERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn counter(id: &str) -> Arc<AtomicU64> {
    let mut map = clear_counters()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    Arc::clone(map.entry(id.to_string()).or_default())
}

/// Clears a paint source's strokes (`/paint/<id>/clear`). False when no such paint is running.
pub fn clear_paint(id: &str) -> bool {
    let map = clear_counters()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    match map.get(id) {
        Some(c) => {
            c.fetch_add(1, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// A live room or paint source: a render thread and its newest frame.
pub struct RoomSource {
    kind: Arc<Mutex<SourceKind>>,
    latest: Arc<Mutex<Option<Arc<Frame>>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RoomSource {
    /// Starts rendering `kind` (a `Room` or `Paint` source) for source `id`.
    pub fn start(id: &str, kind: SourceKind, inputs: Arc<Mutex<Inputs>>) -> Self {
        let kind = Arc::new(Mutex::new(kind));
        let latest = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let clears = counter(id);
        let (k, l, s) = (Arc::clone(&kind), Arc::clone(&latest), Arc::clone(&stop));
        let thread = std::thread::Builder::new()
            .name(format!("lumap-room-{id}"))
            .spawn(move || render_loop(&k, &inputs, &l, &s, &clears))
            .ok();
        Self {
            kind,
            latest,
            stop,
            thread,
        }
    }
}

impl Drop for RoomSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl FrameSource for RoomSource {
    fn frame(&mut self, _t: f64) -> Option<Arc<Frame>> {
        self.latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn is_live(&self) -> bool {
        true
    }

    fn configure(&mut self, kind: &SourceKind) {
        *self.kind.lock().unwrap_or_else(PoisonError::into_inner) = kind.clone();
    }
}

/// What a render thread keeps between frames.
#[derive(Default)]
struct State {
    scan_key: usize,
    size: (u32, u32),
    edges: Option<Gray>,
    trace: Option<trace::Trace>,
    world: Option<(physics::Kind, physics::World)>,
    letters: Option<letters::LetterRain>,
    canvas: Option<paint::Canvas>,
    cleared: u64,
    out: Rgba,
}

fn render_loop(
    kind: &Mutex<SourceKind>,
    inputs: &Mutex<Inputs>,
    latest: &Mutex<Option<Arc<Frame>>>,
    stop: &AtomicBool,
    clears: &AtomicU64,
) {
    let started = Instant::now();
    let mut last = started;
    let mut state = State::default();
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f64().min(0.1);
        last = now;
        let t = now.duration_since(started).as_secs_f64();
        let kind = kind.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let inputs = inputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let wipe = clears.load(Ordering::Relaxed);
        let frame = render(&kind, &inputs, &mut state, dt, t, wipe);
        *latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(frame));
        let spent = now.elapsed();
        let budget = Duration::from_secs_f64(1.0 / FPS);
        if spent < budget {
            std::thread::sleep(budget - spent);
        }
    }
}

/// One frame of a room or paint source.
fn render(
    kind: &SourceKind,
    inputs: &Inputs,
    state: &mut State,
    dt: f64,
    t: f64,
    wipe: u64,
) -> Frame {
    let size = inputs.scan.as_ref().map_or((640, 360), |s| {
        (
            (s.corr.proj_w / SCALE).max(16),
            (s.corr.proj_h / SCALE).max(16),
        )
    });
    let key = inputs.scan.as_ref().map_or(0, |s| Arc::as_ptr(s) as usize);
    if key != state.scan_key || size != state.size {
        *state = State {
            scan_key: key,
            size,
            edges: inputs.scan.as_ref().map(|s| shrink_edges(&s.edges, size)),
            out: Rgba::new(size.0, size.1),
            ..State::default()
        };
    }
    let (w, h) = size;
    let shadow = shadow_mask(inputs, kind, size);
    match kind {
        SourceKind::Room {
            effect,
            color,
            speed,
            ..
        } => {
            let t = t * f64::from(*speed);
            match effect {
                RoomEffect::Trace => {
                    let edges = state.edges.clone().unwrap_or_else(|| Gray::new(w, h));
                    let tr = state.trace.get_or_insert_with(|| trace::Trace::new(&edges));
                    tr.render(&mut state.out, *color, *speed, t);
                }
                RoomEffect::Letters => {
                    let rain = state
                        .letters
                        .get_or_insert_with(|| letters::LetterRain::new(w, h, POEM, 7));
                    rain.step(dt * f64::from(*speed), &shadow);
                    rain.render(&mut state.out, *color, (h as f32 / 120.0).max(1.0));
                }
                RoomEffect::ShadowFill => fill_shadow(&mut state.out, &shadow, *color, t),
                RoomEffect::ShadowMonsters => monsters(&mut state.out, &shadow, *color, t),
                RoomEffect::Snow | RoomEffect::Bounce | RoomEffect::MarbleRun => {
                    let pk = match effect {
                        RoomEffect::Snow => physics::Kind::Snow,
                        RoomEffect::Bounce => physics::Kind::Bounce,
                        _ => physics::Kind::Marble,
                    };
                    if state.world.as_ref().is_none_or(|(k, _)| *k != pk) {
                        let mut world = physics::World::new(w, h, pk, 11);
                        if let Some(e) = &state.edges {
                            world.set_static(&dilate(e));
                        }
                        state.world = Some((pk, world));
                    }
                    if let Some((_, world)) = &mut state.world {
                        world.set_dynamic(&shadow);
                        world.step(dt * f64::from(*speed));
                        world.render(&mut state.out, *color, t);
                    }
                }
            }
        }
        SourceKind::Paint {
            input,
            brush,
            color,
            size: stroke,
            fade_seconds,
            ..
        } => {
            let canvas = state.canvas.get_or_insert_with(|| paint::Canvas::new(w, h));
            if wipe != state.cleared {
                canvas.clear();
                state.cleared = wipe;
            }
            let pen = pen_position(inputs, *input, size);
            canvas.pen(pen, t);
            canvas.render(
                &mut state.out,
                *brush,
                *color,
                *stroke / SCALE as f32,
                *fade_seconds,
                t,
            );
        }
        _ => {}
    }
    Frame {
        width: w,
        height: h,
        rgba: state.out.data.clone(),
    }
}

/// Camera pixel → source pixel, through the scan.
fn to_source(scan: &ScanData, cam: Vec2, size: (u32, u32)) -> Option<Vec2> {
    let p = scan.corr.sample(cam)?;
    Some(Vec2::new(
        p.x * f64::from(size.0) / f64::from(scan.corr.proj_w),
        p.y * f64::from(size.1) / f64::from(scan.corr.proj_h),
    ))
}

/// Where the pen is: the tracked light, or a pinching hand's fingertips.
fn pen_position(inputs: &Inputs, input: PaintInput, size: (u32, u32)) -> Option<Vec2> {
    let (scan, vision) = (inputs.scan.as_ref()?, inputs.vision.as_ref()?);
    let seen = vision.seen.lock().unwrap_or_else(PoisonError::into_inner);
    match input {
        PaintInput::Light => seen.spot.and_then(|(p, _)| to_source(scan, p, size)),
        PaintInput::Hand => {
            let pinching = seen
                .gestures
                .iter()
                .any(|g| g.gesture == lumap_core::cammap::Gesture::Pinch && g.value >= 0.5);
            let hand = seen.hands.first()?;
            if !pinching {
                return None;
            }
            to_source(scan, lumap_vision::gestures::pinch_point(hand), size)
        }
    }
}

/// People's shadows in source pixels (empty without a scan or a camera).
fn shadow_mask(inputs: &Inputs, kind: &SourceKind, size: (u32, u32)) -> Gray {
    let (w, h) = size;
    let wants = matches!(kind, SourceKind::Room { effect, .. } if effect.needs_camera());
    let (Some(scan), Some(vision), true) = (&inputs.scan, &inputs.vision, wants) else {
        return Gray::new(w, h);
    };
    let mask = vision
        .seen
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .shadows
        .get(&scan.projector)
        .cloned();
    let Some(mask) = mask else {
        return Gray::new(w, h);
    };
    let (sx, sy) = (
        f64::from(scan.corr.proj_w) / f64::from(w),
        f64::from(scan.corr.proj_h) / f64::from(h),
    );
    shadow::to_projector(&mask, w, h, &|p: Vec2| {
        scan.map.sample(Vec2::new(p.x * sx, p.y * sy))
    })
}

/// Projector-resolution edges shrunk to the source size, keeping thin lines.
fn shrink_edges(edges: &Gray, size: (u32, u32)) -> Gray {
    let (w, h) = size;
    let mut out = Gray::new(w, h);
    if edges.width == 0 || edges.height == 0 {
        return out;
    }
    for y in 0..edges.height {
        for x in 0..edges.width {
            if edges.get(x, y) > 0 {
                let tx = (u64::from(x) * u64::from(w) / u64::from(edges.width)) as u32;
                let ty = (u64::from(y) * u64::from(h) / u64::from(edges.height)) as u32;
                out.set(tx.min(w - 1), ty.min(h - 1), 255);
            }
        }
    }
    out
}

/// Edges thickened by a pixel, so particles cannot slip through diagonal gaps.
fn dilate(g: &Gray) -> Gray {
    Gray::from_fn(g.width, g.height, |x, y| {
        let (x, y) = (i64::from(x), i64::from(y));
        let hit = (-1..=1).any(|dy| (-1..=1).any(|dx| g.at(x + dx, y + dy) > 0));
        if hit {
            255
        } else {
            0
        }
    })
}

/// Shadows filled with slowly moving colour; everything else clear.
fn fill_shadow(out: &mut Rgba, mask: &Gray, color: [f32; 4], t: f64) {
    for y in 0..out.height {
        for x in 0..out.width {
            let inside = mask.width == out.width && mask.get(x, y) > 0;
            let v = if inside {
                let wave = 0.65
                    + 0.35
                        * ((f64::from(x) * 0.045 + t * 1.7).sin()
                            * (f64::from(y) * 0.05 - t * 1.3).cos());
                [
                    (f64::from(color[0]) * wave * 255.0) as u8,
                    (f64::from(color[1]) * wave * 255.0) as u8,
                    (f64::from(color[2]) * wave * 255.0) as u8,
                    255,
                ]
            } else {
                [0, 0, 0, 0]
            };
            out.set(x, y, v);
        }
    }
}

/// Every big enough shadow grows two blinking eyes and a row of teeth near its top.
fn monsters(out: &mut Rgba, mask: &Gray, color: [f32; 4], t: f64) {
    out.data.fill(0);
    if mask.width != out.width || mask.height != out.height {
        return;
    }
    let min_area = f64::from(out.width * out.height) * 0.004;
    for blob in features::contours(mask) {
        if features::polygon_area(&blob).abs() < min_area {
            continue;
        }
        let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
        for p in &blob {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        let (bw, bh) = (hi.x - lo.x, hi.y - lo.y);
        let r = (bw.min(bh) * 0.09).max(2.0);
        let eye_y = lo.y + bh * 0.22;
        let blink = (t * 0.9).fract() < 0.06;
        for ex in [lo.x + bw * 0.36, lo.x + bw * 0.64] {
            if blink {
                disc(out, Vec2::new(ex, eye_y), r, 0.25, [255, 255, 255, 255]);
            } else {
                disc(out, Vec2::new(ex, eye_y), r, 1.0, [255, 255, 255, 255]);
                let look = Vec2::new((t * 0.7).sin() * r * 0.4, 0.0);
                disc(
                    out,
                    Vec2::new(ex, eye_y) + look,
                    r * 0.45,
                    1.0,
                    [10, 10, 10, 255],
                );
            }
        }
        let tooth = [
            (f64::from(color[0]) * 255.0) as u8,
            (f64::from(color[1]) * 255.0) as u8,
            (f64::from(color[2]) * 255.0) as u8,
            255,
        ];
        let mouth_y = lo.y + bh * 0.4;
        let n = 6;
        for i in 0..n {
            let cx = lo.x + bw * (0.3 + 0.4 * f64::from(i) / f64::from(n - 1));
            triangle(out, Vec2::new(cx, mouth_y), r * 0.6, r * 1.1, tooth);
        }
    }
}

/// A filled ellipse (`squash` < 1 flattens it, for a blink).
fn disc(out: &mut Rgba, c: Vec2, r: f64, squash: f64, v: [u8; 4]) {
    let ry = (r * squash).max(0.5);
    let (x0, x1) = ((c.x - r).floor() as i64, (c.x + r).ceil() as i64);
    let (y0, y1) = ((c.y - ry).floor() as i64, (c.y + ry).ceil() as i64);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (dx, dy) = ((x as f64 + 0.5 - c.x) / r, (y as f64 + 0.5 - c.y) / ry);
            if dx * dx + dy * dy <= 1.0 {
                put(out, x, y, v);
            }
        }
    }
}

/// A downward-pointing tooth.
fn triangle(out: &mut Rgba, top: Vec2, half: f64, height: f64, v: [u8; 4]) {
    let (y0, y1) = (top.y.floor() as i64, (top.y + height).ceil() as i64);
    for y in y0..=y1 {
        let k = 1.0 - ((y as f64 - top.y) / height).clamp(0.0, 1.0);
        let span = half * k;
        for x in (top.x - span).floor() as i64..=(top.x + span).ceil() as i64 {
            put(out, x, y, v);
        }
    }
}

fn put(out: &mut Rgba, x: i64, y: i64, v: [u8; 4]) {
    if x >= 0 && y >= 0 && (x as u32) < out.width && (y as u32) < out.height {
        out.set(x as u32, y as u32, v);
    }
}

/// The live room and paint sources the app has attached, by source id.
#[derive(Default)]
pub struct Runtime {
    attached: HashMap<String, (SourceKind, Arc<Mutex<Inputs>>)>,
    epoch: Option<u64>,
}

impl Runtime {
    /// Attaches a live source for every room and paint source of the show (again after the
    /// sources were reopened), and keeps each one's inputs current.
    pub fn sync(&mut self, editor: &mut super::Editor, vision: Option<&Arc<VisionShared>>) {
        if self.epoch != Some(editor.sources_epoch) {
            self.attached.clear();
            self.epoch = Some(editor.sources_epoch);
        }
        let wanted: Vec<(String, SourceKind)> = editor
            .project
            .sources
            .iter()
            .filter(|s| matches!(s.kind, SourceKind::Room { .. } | SourceKind::Paint { .. }))
            .map(|s| (s.id.clone(), s.kind.clone()))
            .collect();
        self.attached
            .retain(|id, _| wanted.iter().any(|(w, _)| w == id));
        for (id, kind) in wanted {
            let projector = match &kind {
                SourceKind::Room { projector, .. } | SourceKind::Paint { projector, .. } => {
                    projector.clone()
                }
                _ => continue,
            };
            let inputs = Inputs {
                scan: editor.scans.get(&projector),
                vision: vision.cloned(),
            };
            let same_kind = self
                .attached
                .get(&id)
                .is_some_and(|(k, _)| std::mem::discriminant(k) == std::mem::discriminant(&kind));
            if same_kind {
                if let Some((k, shared)) = self.attached.get_mut(&id) {
                    *shared.lock().unwrap_or_else(PoisonError::into_inner) = inputs;
                    k.clone_from(&kind);
                }
                continue;
            }
            let shared = Arc::new(Mutex::new(inputs));
            let source = RoomSource::start(&id, kind.clone(), Arc::clone(&shared));
            editor.sources.insert(id.clone(), Box::new(source));
            self.attached.insert(id, (kind, shared));
        }
    }

    /// True when some attached source watches the camera (paint, or a shadow effect).
    pub fn needs_camera(&self) -> (bool, bool, bool) {
        let mut spot = false;
        let mut hands = false;
        let mut shadows = false;
        for (k, _) in self.attached.values() {
            match k {
                SourceKind::Paint {
                    input: PaintInput::Light,
                    ..
                } => spot = true,
                SourceKind::Paint {
                    input: PaintInput::Hand,
                    ..
                } => hands = true,
                SourceKind::Room { effect, .. } if effect.needs_camera() => shadows = true,
                _ => {}
            }
        }
        (spot, hands, shadows)
    }

    /// Projectors whose room effects need shadows.
    pub fn shadow_projectors(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .attached
            .values()
            .filter_map(|(k, _)| match k {
                SourceKind::Room {
                    projector, effect, ..
                } if effect.needs_camera() => Some(projector.clone()),
                _ => None,
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

/// The brushes and inputs offered for paint, for the inspector.
pub fn brushes() -> [Brush; 4] {
    Brush::ALL
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn edges_shrink_and_thicken_and_clearing_reaches_running_paint() {
        let mut e = Gray::new(9, 9);
        e.set(4, 4, 255);
        let small = shrink_edges(&e, (3, 3));
        assert_eq!(small.get(1, 1), 255);
        let thick = dilate(&small);
        assert_eq!(thick.get(0, 0), 255);
        assert!(!clear_paint("nobody"));
        let c = counter("paint-a");
        assert!(clear_paint("paint-a"));
        assert_eq!(c.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn shadows_fill_and_monsters_draw_inside_the_shadow_only() {
        let mut mask = Gray::new(60, 60);
        for y in 10..50 {
            for x in 15..45 {
                mask.set(x, y, 255);
            }
        }
        let mut out = Rgba::new(60, 60);
        fill_shadow(&mut out, &mask, [1.0, 0.5, 0.0, 1.0], 0.3);
        assert_eq!(out.get(0, 0)[3], 0);
        assert_eq!(out.get(30, 30)[3], 255);
        monsters(&mut out, &mask, [1.0, 1.0, 1.0, 1.0], 0.3);
        assert!(out.data.chunks_exact(4).any(|p| p == [255, 255, 255, 255]));
        assert_eq!(out.get(0, 0)[3], 0);
    }

    #[test]
    fn a_room_source_renders_without_a_scan_and_stops_on_drop() {
        let kind = SourceKind::Room {
            projector: "left".into(),
            effect: RoomEffect::Snow,
            color: [1.0; 4],
            speed: 1.0,
        };
        let mut src = RoomSource::start(
            "snow",
            kind.clone(),
            Arc::new(Mutex::new(Inputs::default())),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut frame = src.frame(0.0);
        while frame.is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            frame = src.frame(0.0);
        }
        let frame = frame.unwrap();
        assert_eq!((frame.width, frame.height), (640, 360));
        assert!(src.is_live());
        src.configure(&kind);
        drop(src);
    }
}
