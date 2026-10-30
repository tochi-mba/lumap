//! The camera features, frame by frame: scans start, run and land in the show; the vision
//! worker runs only while something wants it; zones and gestures drive parameters through
//! Camera learn; hands move a cursor on the wall, paint and grab corners; the lab is answered.

use super::scan::{self, Kind, Phase, ScanSession};
use super::vision::{self, ShadowInput, Vision, Wants};
use super::{panels, LumapApp};
use eframe::egui;
use lumap_core::cammap::{CamEvent, CamKey, Gesture};
use lumap_core::geom::Vec2;
use lumap_core::midimap::MidiMode;
use lumap_core::project::SourceKind;
use lumap_core::warp::WarpKind;
use lumap_vision::Gray;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often shadow inputs (what each projector shows, as the camera should see it) refresh.
const SHADOW_EVERY: Duration = Duration::from_millis(150);
/// How close (normalised) a pinch must be to a corner to grab it.
const GRAB: f64 = 0.04;

/// Camera state the app keeps between frames.
#[derive(Default)]
pub struct LiveState {
    /// Last camera readings, for Camera learn edges.
    previous: Vec<CamEvent>,
    /// Shadow inputs per projector and when they were made.
    shadows: HashMap<String, (Instant, ShadowInput)>,
    /// A corner held by a pinching hand: surface, corner index.
    grab: Option<(String, usize)>,
    /// The outcome of the last scan, for the lab.
    pub outcome: Option<Result<serde_json::Value, String>>,
    /// The last scan was adopted (or reported).
    pub settled: bool,
    /// Drift reports from the last drift check, for the go-live check.
    pub drift: Option<Vec<scan::DriftReport>>,
}

impl LumapApp {
    /// The show's camera (its choice, or the first one), if there is a camera at all.
    pub(crate) fn camera_device(&mut self) -> Option<String> {
        let chosen = self.editor.project.camera.device.clone();
        self.camera.resolve(&chosen)
    }

    /// Starts a scan, drift check or re-align of one projector, or of every projector that is
    /// on in show mode (re-align and drift: every scanned projector).
    pub(crate) fn start_scan(
        &mut self,
        kind: Kind,
        projector: Option<&str>,
        now: Instant,
    ) -> Result<(), String> {
        if self.editor.show_mode {
            return Err("Leave show mode to scan.".into());
        }
        if self.scan.as_ref().is_some_and(|s| !s.finished()) {
            return Err("A scan is already running.".into());
        }
        let device = self
            .camera_device()
            .ok_or("There is no camera. Plug one in, or pick one in Setup.")?;
        let ids: Vec<String> = match projector {
            Some(p) => vec![p.to_string()],
            None => self
                .editor
                .project
                .outputs
                .iter()
                .filter(|o| match kind {
                    Kind::Full => o.enabled,
                    Kind::Drift | Kind::Realign => self.editor.scans.get(&o.id).is_some(),
                })
                .map(|o| o.id.clone())
                .collect(),
        };
        if ids.is_empty() {
            return Err(match kind {
                Kind::Full => "There is no projector to scan.".into(),
                Kind::Drift | Kind::Realign => "No projector has a scan yet.".into(),
            });
        }
        let root = scan::scan_root(self.editor.path.as_deref());
        self.scan = Some(ScanSession::start(
            kind,
            &self.editor,
            &ids,
            &device,
            root,
            now,
        ));
        self.live.outcome = None;
        self.live.settled = false;
        self.panels.scan_sheet = true;
        Ok(())
    }

    /// The finished scan's outcome, once (for the lab).
    pub(crate) fn scan_outcome(&mut self) -> Option<Result<serde_json::Value, String>> {
        self.live.outcome.clone()
    }

    /// Ticks the running scan; when it ends, adopts it into the show and records the outcome.
    pub(crate) fn tick_scan(&mut self, now: Instant) {
        self.editor
            .scans
            .sync(&self.editor.project, &self.editor.base_dir());
        let Some(session) = &mut self.scan else {
            return;
        };
        session.tick(&mut self.editor, &mut self.camera, now);
        if !session.finished() || self.live.settled {
            return;
        }
        self.live.settled = true;
        let kind = session.kind;
        self.live.outcome = Some(match session.phase().clone() {
            Phase::Done => {
                let adopted = session.adopt(&mut self.editor);
                if kind == Kind::Drift {
                    self.live.drift = Some(session.drifts().to_vec());
                } else {
                    self.live.drift = None;
                }
                let drift: Vec<serde_json::Value> = session
                    .drifts()
                    .iter()
                    .map(|d| {
                        serde_json::json!({
                            "projector": d.projector,
                            "mean": d.mean,
                            "max": d.max,
                            "camera_moved": d.camera_moved,
                        })
                    })
                    .collect();
                self.editor.status = match kind {
                    Kind::Full => format!("Scanned {adopted} projector(s)."),
                    Kind::Realign => format!("Re-aligned {adopted} projector(s)."),
                    Kind::Drift => "Checked the projectors with the camera.".into(),
                };
                Ok(serde_json::json!({ "ok": true, "kind": format!("{kind:?}"), "adopted": adopted, "drift": drift }))
            }
            Phase::Failed(e) => {
                self.editor.status = e.clone();
                Err(e)
            }
            Phase::Cancelled => Err("The scan was cancelled.".into()),
            Phase::Showing | Phase::Decoding => return,
        });
    }

    /// Runs the vision worker while something wants it and feeds its reports to Camera learn,
    /// gestures, paint clearing and the hand cursor.
    pub(crate) fn tick_vision(&mut self, now: Instant) {
        let (spot, hand_paint, shadows) = self.room.needs_camera();
        let setup = &self.editor.project.camera;
        let zones: Vec<(String, Vec<Vec2>, f64)> = setup
            .zones
            .iter()
            .filter(|z| z.is_valid())
            .map(|z| (z.id.clone(), z.points.clone(), z.sensitivity))
            .collect();
        let gesture_mapped = setup
            .map
            .mappings
            .iter()
            .any(|m| matches!(m.key, CamKey::Gesture { .. }));
        let learning = self.editor.cam_learn.is_some();
        let hands = hand_paint || gesture_mapped || self.canvas.hands_map || learning;
        let wanted = spot
            || hands
            || shadows
            || (!zones.is_empty() && (!setup.map.mappings.is_empty() || learning || self.panels.camera_meters));
        if !wanted || self.editor.scan_pattern.is_some() {
            self.vision = None;
            self.editor.hand_cursor.clear();
            return;
        }
        let Some(device) = self.camera_device() else {
            self.vision = None;
            return;
        };
        let Some(stream) = self.camera.ensure(&device, now) else {
            self.vision = None;
            return;
        };
        if self.vision.as_ref().is_none_or(|v| v.camera() != device) {
            self.vision = Some(Vision::start(&device, stream, vision::models_dir()));
        }
        let shadow_inputs = if shadows {
            self.shadow_inputs(now)
        } else {
            Vec::new()
        };
        let Some(v) = &self.vision else { return };
        v.want(Wants {
            spot,
            zones,
            shadows: shadow_inputs,
            hands,
        });
        let seen = v.seen();
        self.camera_events(&seen);
        self.hand_cursor(&seen);
    }

    /// What each projector with a shadow effect shows now, as its camera should see it.
    fn shadow_inputs(&mut self, now: Instant) -> Vec<ShadowInput> {
        let mut out = Vec::new();
        for projector in self.room.shadow_projectors() {
            let Some(scan) = self.editor.scans.get(&projector) else {
                continue;
            };
            let fresh = self
                .live
                .shadows
                .get(&projector)
                .is_some_and(|(t, _)| now.duration_since(*t) < SHADOW_EVERY);
            if !fresh {
                let projected = super::scene_for(&mut self.editor, &projector)
                    .map(|scene| lumap_render::software::render_at(&scene, 160, 90))
                    .map_or_else(
                        || Gray::new(160, 90),
                        |img| {
                            Gray::from_rgba(&lumap_vision::Rgba {
                                width: img.width(),
                                height: img.height(),
                                data: img.into_raw(),
                            })
                        },
                    );
                let input = ShadowInput {
                    projector: projector.clone(),
                    white: Arc::new(scan.white.clone()),
                    black: Arc::new(scan.black.clone()),
                    projected: Arc::new(in_camera(&scan.corr, &projected)),
                };
                self.live.shadows.insert(projector.clone(), (now, input));
            }
            if let Some((_, input)) = self.live.shadows.get(&projector) {
                out.push(input.clone());
            }
        }
        out
    }

    /// Zones and gestures: learn the next one when asked, otherwise send what changed.
    fn camera_events(&mut self, seen: &vision::Seen) {
        let mut events: Vec<CamEvent> = seen
            .zones
            .iter()
            .map(|(id, v)| CamEvent {
                key: CamKey::Zone { zone: id.clone() },
                value: *v,
            })
            .collect();
        events.extend(seen.gestures.iter().map(|g| CamEvent {
            key: CamKey::Gesture { gesture: g.gesture },
            value: g.value,
        }));
        if let Some((address, mode)) = self.editor.cam_learn.clone() {
            if let Some(e) = events.iter().find(|e| e.value >= 0.5) {
                let key = e.key.clone();
                let label = key.label();
                self.editor.edit(move |p| p.camera.map.learn(key, &address, mode));
                self.editor.status = format!("Learned: {label}");
                self.editor.cam_learn = None;
            }
            self.live.previous = events;
            return;
        }
        let map = self.editor.project.camera.map.clone();
        let clock = self.editor.sources.time();
        for e in map.edges(&self.live.previous, &events) {
            for patch in map.translate(&e) {
                self.editor.handle_control(&patch, clock);
            }
        }
        // A fist clears hand-drawn paint.
        let was_fist = self.live.previous.iter().any(|e| {
            e.key == CamKey::Gesture { gesture: Gesture::Fist } && e.value >= 0.5
        });
        let fist = events
            .iter()
            .any(|e| e.key == CamKey::Gesture { gesture: Gesture::Fist } && e.value >= 0.5);
        if fist && !was_fist {
            for s in &self.editor.project.sources {
                if matches!(
                    s.kind,
                    SourceKind::Paint {
                        input: lumap_core::project::PaintInput::Hand,
                        ..
                    }
                ) {
                    super::room::clear_paint(&s.id);
                }
            }
        }
        self.live.previous = events;
    }

    /// Where the hand points on each scanned projector, and grabbing corners with a pinch.
    fn hand_cursor(&mut self, seen: &vision::Seen) {
        self.editor.hand_cursor.clear();
        let Some(hand) = seen.hands.first() else {
            self.release_grab();
            return;
        };
        let pinching = seen
            .gestures
            .iter()
            .any(|g| g.gesture == Gesture::Pinch && g.value >= 0.5);
        let tip = lumap_vision::gestures::pinch_point(hand);
        let mut on: Option<(String, Vec2)> = None;
        for scan in self.editor.scans.ready() {
            if let Some(p) = scan.corr.sample(tip) {
                let n = Vec2::new(
                    p.x / f64::from(scan.corr.proj_w),
                    p.y / f64::from(scan.corr.proj_h),
                );
                self.editor
                    .hand_cursor
                    .insert(scan.projector.clone(), (n, pinching));
                on = Some((scan.projector.clone(), n));
            }
        }
        if !(self.canvas.hands_map && self.editor.edit_on_wall && !self.editor.show_mode) {
            return;
        }
        let Some((projector, n)) = on else {
            self.release_grab();
            return;
        };
        if !pinching {
            self.release_grab();
            return;
        }
        if self.live.grab.is_none() {
            self.live.grab = nearest_corner(&self.editor, &projector, n);
        }
        if let Some((sid, corner)) = self.live.grab.clone() {
            let warp = self.editor.project.surface(&sid).map(|s| s.warp.clone());
            if let Some(WarpKind::Quad(mut q)) = warp {
                q.set_corner(corner, n);
                self.editor.set_warp(&sid, WarpKind::Quad(q));
            }
        }
    }

    fn release_grab(&mut self) {
        if self.live.grab.take().is_some() {
            self.editor.commit();
        }
    }

    /// Answers the lab and starts it on the first frame when asked for.
    pub(crate) fn tick_lab(&mut self, ctx: &egui::Context, now: Instant) {
        if self.lab.is_none() {
            if let Some(port) = self.lab_port.take() {
                match super::lab::LabRuntime::start(ctx, port) {
                    Ok(lab) => {
                        self.editor.status = format!("Lab on {}", lab.url());
                        self.lab = Some(lab);
                    }
                    Err(e) => self.panels.error = Some(format!("The lab could not start: {e}")),
                }
            }
        }
        if let Some(mut lab) = self.lab.take() {
            lab.tick(self, ctx, now);
            self.lab = Some(lab);
        }
    }

    /// Carries out a scan request from the panels.
    pub(crate) fn run_scan_action(&mut self, request: Option<(Kind, Option<String>)>, now: Instant) {
        let Some((kind, projector)) = request else {
            return;
        };
        if let Err(e) = self.start_scan(kind, projector.as_deref(), now) {
            self.panels.error = Some(e);
        }
    }

    /// The scan in progress or just finished.
    pub(crate) fn scan_session(&self) -> Option<&ScanSession> {
        self.scan.as_ref()
    }

    /// Stops a running scan.
    pub(crate) fn cancel_scan(&mut self) {
        if let Some(s) = &mut self.scan {
            s.cancel(&mut self.editor);
        }
    }

    /// Closes the scan sheet and forgets the finished session.
    pub(crate) fn close_scan(&mut self) {
        if self.scan.as_ref().is_none_or(ScanSession::finished) {
            self.scan = None;
        }
        self.panels.scan_sheet = false;
    }

    /// Puts measured blend widths from the scans on every overlapping projector.
    pub(crate) fn use_measured_blends(&mut self) -> usize {
        let scans = self.editor.scans.ready();
        if scans.len() < 2 {
            return 0;
        }
        let maps: Vec<lumap_vision::correspondence::ProjectorMap> =
            scans.iter().map(|s| s.map.clone()).collect();
        let (cw, ch) = (scans[0].corr.cam_w, scans[0].corr.cam_h);
        let overlaps = lumap_vision::overlap::blend_widths(&maps, cw, ch);
        let edits: Vec<(String, lumap_vision::correspondence::Edge, f64)> = overlaps
            .iter()
            .filter_map(|o| {
                scans
                    .get(o.projector)
                    .map(|s| (s.projector.clone(), o.edge, o.width))
            })
            .collect();
        let n = edits.len();
        self.editor.edit(move |p| {
            for (id, edge, width) in &edits {
                if let Some(out) = p.output_mut(id) {
                    let b = &mut out.blend;
                    let w = width.clamp(0.0, 0.5);
                    match edge {
                        lumap_vision::correspondence::Edge::Left => b.left.width = w,
                        lumap_vision::correspondence::Edge::Right => b.right.width = w,
                        lumap_vision::correspondence::Edge::Top => b.top.width = w,
                        lumap_vision::correspondence::Edge::Bottom => b.bottom.width = w,
                    }
                }
            }
        });
        n
    }

    /// The step the scan sheet sends people to when they want to see what was found.
    pub(crate) fn review_found(&mut self) {
        self.panels.step = panels::shell::Step::Map;
        self.canvas.wall_photo = true;
        self.canvas.show_found = true;
        self.close_scan();
    }
}

/// The nearest quad corner of a surface on `projector` to `n` (normalised), within reach.
fn nearest_corner(editor: &super::Editor, projector: &str, n: Vec2) -> Option<(String, usize)> {
    let mut best: Option<(String, usize, f64)> = None;
    for s in editor.project.surfaces_for_output(projector) {
        if editor.is_locked(&s.id) {
            continue;
        }
        if let WarpKind::Quad(q) = &s.warp {
            for (i, c) in q.corners.iter().enumerate() {
                let d = c.distance(n);
                if d < GRAB && best.as_ref().is_none_or(|b| d < b.2) {
                    best = Some((s.id.clone(), i, d));
                }
            }
        }
    }
    best.map(|(s, i, _)| (s, i))
}

/// A low-resolution projector picture as the camera would see it (camera-space luma).
fn in_camera(corr: &lumap_vision::correspondence::Correspondence, projected: &Gray) -> Gray {
    let (sx, sy) = (
        f64::from(projected.width) / f64::from(corr.proj_w.max(1)),
        f64::from(projected.height) / f64::from(corr.proj_h.max(1)),
    );
    Gray::from_fn(corr.cam_w, corr.cam_h, |x, y| match corr.get(x, y) {
        Some(p) => projected.sample(p.x * sx, p.y * sy).round().clamp(0.0, 255.0) as u8,
        None => 0,
    })
}
