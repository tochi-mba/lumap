//! The camera's live eyes: a worker thread that watches the newest camera frame and reports a
//! torch or laser spot, motion in each zone, people's shadows and hands, for paint, room effects,
//! Camera learn and gestures. The editor never waits on it: it reads the last report.
//!
//! The worker only runs while something wants it (a paint or room source, a zone or gesture
//! mapping, the lab), so an ordinary show pays nothing for the camera.

use lumap_core::geom::Vec2;
use lumap_sources::live_input::camera::CameraStream;
use lumap_vision::gestures::{GestureClassifier, GestureReading, HandPose};
use lumap_vision::motion::MotionDetector;
use lumap_vision::spots::{SpotSettings, SpotTracker};
use lumap_vision::{shadow, Gray, Rgba};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// What the worker should look for.
#[derive(Debug, Clone, Default)]
pub struct Wants {
    /// Track the brightest moving spot (paint with a light).
    pub spot: bool,
    /// Zones to measure: id, polygon (normalised camera coordinates), sensitivity.
    pub zones: Vec<(String, Vec<Vec2>, f64)>,
    /// Find shadows: for each scanned projector its white and black camera views and what it
    /// is projecting now (low resolution luma in projector space, and its size).
    pub shadows: Vec<ShadowInput>,
    /// Track hands and gestures.
    pub hands: bool,
}

/// What shadow finding needs for one projector.
#[derive(Clone)]
pub struct ShadowInput {
    /// The projector.
    pub projector: String,
    /// Camera view, projector white.
    pub white: Arc<Gray>,
    /// Camera view, projector black.
    pub black: Arc<Gray>,
    /// What the projector shows now, as the camera should see it (camera space luma).
    pub projected: Arc<Gray>,
}

impl std::fmt::Debug for ShadowInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShadowInput")
            .field("projector", &self.projector)
            .finish_non_exhaustive()
    }
}

/// The worker's latest report.
#[derive(Debug, Clone, Default)]
pub struct Seen {
    /// The spot, in camera pixels, and how strong it is.
    pub spot: Option<(Vec2, f64)>,
    /// Each zone's motion or presence, `0..=1`.
    pub zones: Vec<(String, f64)>,
    /// People's shadows in camera space (255 = shadow), per projector.
    pub shadows: HashMap<String, Arc<Gray>>,
    /// Hands, in camera pixels.
    pub hands: Vec<HandPose>,
    /// Gesture readings for the first hand.
    pub gestures: Vec<GestureReading>,
    /// Camera picture size.
    pub size: (u32, u32),
    /// Frames analysed per second.
    pub fps: f32,
    /// When the report was made.
    pub at: Option<Instant>,
}

/// Shared between the editor (writes wants, reads reports) and the worker.
#[derive(Default)]
pub struct Shared {
    /// What to look for.
    pub wants: Mutex<Wants>,
    /// The last report.
    pub seen: Mutex<Seen>,
    stop: AtomicBool,
}

/// The running worker.
pub struct Vision {
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
    camera: String,
}

impl Vision {
    /// Starts watching `stream` (the camera named `camera`); `models` is where hand models live.
    pub fn start(
        camera: &str,
        stream: Arc<dyn CameraStream>,
        models: Option<std::path::PathBuf>,
    ) -> Self {
        let shared = Arc::new(Shared::default());
        let worker = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("lumap-vision".into())
            .spawn(move || run(&worker, &stream, models))
            .ok();
        Self {
            shared,
            thread,
            camera: camera.to_string(),
        }
    }

    /// The camera it watches.
    pub fn camera(&self) -> &str {
        &self.camera
    }

    /// Sets what to look for.
    pub fn want(&self, wants: Wants) {
        *self
            .shared
            .wants
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = wants;
    }

    /// The last report.
    pub fn seen(&self) -> Seen {
        self.shared
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The shared state, for sources that read reports from their own threads.
    pub fn shared(&self) -> Arc<Shared> {
        Arc::clone(&self.shared)
    }
}

impl Drop for Vision {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The worker loop: analyse each new frame, publish a report.
fn run(shared: &Shared, stream: &Arc<dyn CameraStream>, models: Option<std::path::PathBuf>) {
    let mut last = 0u64;
    let mut spots = SpotTracker::new(SpotSettings::default());
    let mut motion: Option<MotionDetector> = None;
    let mut classifier = GestureClassifier::new();
    let mut hands = HandRunner::new(models);
    let started = Instant::now();
    let mut count = 0u32;
    let mut window = Instant::now();
    let mut fps = 0.0f32;
    while !shared.stop.load(Ordering::Relaxed) {
        let seen = stream.frames_seen();
        let Some(frame) = (seen != last).then(|| stream.latest()).flatten() else {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        };
        last = seen;
        let wants = shared
            .wants
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let t = started.elapsed().as_secs_f64();
        let rgba = Rgba {
            width: frame.width,
            height: frame.height,
            data: frame.rgba.clone(),
        };
        let grey = Gray::from_rgba(&rgba);
        let mut report = Seen {
            size: (frame.width, frame.height),
            ..Seen::default()
        };
        if wants.spot {
            report.spot = spots.update(&rgba, None, t).map(|s| (s.pos, s.strength));
        }
        if !wants.zones.is_empty() {
            let m = motion.get_or_insert_with(|| MotionDetector::new(frame.width, frame.height));
            m.update(&grey);
            report.zones = wants
                .zones
                .iter()
                .map(|(id, poly, sens)| (id.clone(), m.zone(poly, *sens)))
                .collect();
        }
        for s in &wants.shadows {
            if s.white.width != grey.width || s.white.height != grey.height {
                continue;
            }
            let expected = shadow::expected(&s.white, &s.black, &s.projected);
            let mask = shadow::mask(&grey, &expected, &shadow::ShadowSettings::default());
            report.shadows.insert(s.projector.clone(), Arc::new(mask));
        }
        if wants.hands {
            report.hands = hands.track(&rgba);
            report.gestures = classifier.update(report.hands.first(), t);
        }
        count += 1;
        let dt = window.elapsed().as_secs_f32();
        if dt >= 1.0 {
            fps = count as f32 / dt;
            count = 0;
            window = Instant::now();
        }
        report.fps = fps;
        report.at = Some(Instant::now());
        *shared.seen.lock().unwrap_or_else(PoisonError::into_inner) = report;
    }
}

/// Hand tracking when this build has it and the models are installed; nothing otherwise.
struct HandRunner {
    #[cfg(feature = "hands")]
    tracker: Option<lumap_vision::hands::HandTracker>,
}

impl HandRunner {
    fn new(models: Option<std::path::PathBuf>) -> Self {
        #[cfg(feature = "hands")]
        {
            let tracker = models.and_then(|dir| {
                lumap_vision::hands::HandTracker::load(&dir)
                    .map_err(|e| log::warn!("hand tracking off: {e}"))
                    .ok()
            });
            Self { tracker }
        }
        #[cfg(not(feature = "hands"))]
        {
            let _ = models;
            Self {}
        }
    }

    #[allow(clippy::unused_self)]
    fn track(&mut self, frame: &Rgba) -> Vec<HandPose> {
        #[cfg(feature = "hands")]
        {
            if let Some(t) = &mut self.tracker {
                return t.track(frame);
            }
        }
        let _ = frame;
        Vec::new()
    }
}

/// Where lumap keeps hand models: beside the executable (`models/`), or `LUMAP_MODELS`.
pub fn models_dir() -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("LUMAP_MODELS") {
        return Some(dir.into());
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.join("models");
    dir.is_dir().then_some(dir)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use lumap_sources::live_input::camera::{CameraBackend, CaptureSettings, FakeCamera};

    #[test]
    fn the_worker_reports_zones_and_stops_when_dropped() {
        let fake = FakeCamera::solid("cam", 32, 24, [40, 40, 40, 255]);
        let stream = fake.open("cam", &CaptureSettings::default()).unwrap();
        let v = Vision::start("cam", stream, None);
        assert_eq!(v.camera(), "cam");
        v.want(Wants {
            zones: vec![(
                "door".into(),
                vec![
                    Vec2::new(0.0, 0.0),
                    Vec2::new(1.0, 0.0),
                    Vec2::new(1.0, 1.0),
                    Vec2::new(0.0, 1.0),
                ],
                1.0,
            )],
            spot: true,
            ..Wants::default()
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = v.seen();
        while seen.zones.is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            seen = v.seen();
        }
        assert_eq!(seen.zones.len(), 1);
        assert!(seen.zones[0].1 < 0.2, "a still picture has no motion");
        assert_eq!(seen.size, (32, 24));
        drop(v);
    }

    #[test]
    fn hand_models_come_from_the_environment_first() {
        std::env::set_var("LUMAP_MODELS", "/somewhere/models");
        assert_eq!(
            models_dir(),
            Some(std::path::PathBuf::from("/somewhere/models"))
        );
        std::env::remove_var("LUMAP_MODELS");
    }
}
