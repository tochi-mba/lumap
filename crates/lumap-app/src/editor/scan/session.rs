//! A scan as it runs. For each projector: show white, black and every Gray-code stripe pattern
//! (each with its inverse) full-screen while the other projectors stay black; for each pattern
//! wait until the camera has had time to see it, then take the median of a few fresh frames and
//! hand it to the decoder. A worker thread decodes, analyses and saves each projector's scan
//! while the next projector is photographed.
//!
//! Three kinds share the machinery: a full scan, a quick drift check (only the coarse stripes,
//! about two seconds a projector) and a re-align (a full scan whose result moves every surface
//! on the projector from where the old scan saw it to where the new one does).

use super::data::ScanData;
use crate::editor::camera::{self, CameraHub};
use crate::editor::Editor;
use lumap_core::geom::Vec2;
use lumap_core::warp::WarpKind;
use lumap_sources::Frame;
use lumap_vision::graycode::{DecodeSettings, Decoder, GrayCode, PatternFrame};
use lumap_vision::{realign, Gray, Rgba};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The least time a pattern stays up before frames count.
const SETTLE: Duration = Duration::from_millis(150);
/// The longest wait for the camera to show a new pattern before the lag is known. Phone cameras
/// streamed as webcams can lag by most of a second.
const FIRST_WAIT: Duration = Duration::from_millis(2500);
/// Once the lag is known, a pattern the camera shows no change for (stripes finer than the
/// camera can see) is photographed after this much more than the lag.
const LAG_MARGIN: Duration = Duration::from_millis(250);
/// The longest wait for the picture to hold still after it changed (auto-exposure settling: a
/// phone's camera takes a couple of seconds after a big change in light).
const STEADY_WAIT: Duration = Duration::from_millis(3000);
/// After a change shows up, the least time before the picture can count as still: phones smooth
/// their video over time, so a new pattern fades in over several frames that each look nearly
/// like the last.
const AFTER_CHANGE: Duration = Duration::from_millis(450);
/// Frames in a row that must agree before the picture counts as still.
const STEADY_FRAMES: u32 = 2;
/// How long the camera must have been sending pictures before the first pattern: a phone used
/// as a webcam shows a loading spinner, then black, while its stream starts.
const WARM_UP: Duration = Duration::from_millis(2500);
/// How different in shape (1 - correlation) the camera's picture must become to count as the
/// new pattern arriving. Auto-exposure makes the whole picture brighter or darker, which keeps
/// the correlation near 1; a new stripe pattern moves where the light is.
const CHANGED: f64 = 0.15;
/// Change in shape (1 - correlation) between frames below which the picture is holding still.
const STEADY_SHAPE: f64 = 0.02;
/// Mean brightness change between frames below which the picture is holding still.
const STEADY: f64 = 2.5;
/// Fresh frames whose median makes one photo.
const FRAMES_PER_PATTERN: usize = 3;
/// No new camera frame for this long ends the scan.
const STALL: Duration = Duration::from_secs(4);
/// Stripes a drift check uses per axis.
const DRIFT_BITS: u32 = 4;
/// A camera that moved more than this (see `realign::camera_moved`) needs a new scan.
pub const CAMERA_MOVED: f64 = 0.25;

/// What a scan is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A full scan: the projector's-eye photo, surfaces found, blends measured.
    Full,
    /// A quick check that projectors are still where they were scanned.
    Drift,
    /// A full scan that then moves every surface to follow its projector.
    Realign,
}

/// Where a scan has got to.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// Photographing patterns.
    Showing,
    /// Every photo taken; workers decoding.
    Decoding,
    /// Finished; results ready to adopt.
    Done,
    /// Stopped by a problem, explained.
    Failed(String),
    /// Stopped by the user.
    Cancelled,
    /// Paused before the stripes: the camera's view needs fixing first (the advice, worst
    /// first). The projector stays white so the camera can be aimed.
    Adjust(Vec<String>),
}

/// Progress for the scan sheet and the lab.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    /// Projector being photographed.
    pub projector: String,
    /// Its position, from 0.
    pub index: usize,
    /// How many projectors.
    pub of: usize,
    /// Patterns photographed for this projector.
    pub frame: usize,
    /// Patterns in all for this projector.
    pub frames: usize,
    /// Where the scan is.
    pub phase: Phase,
}

/// How far a projector drifted since its scan.
#[derive(Debug, Clone, PartialEq)]
pub struct DriftReport {
    /// The projector.
    pub projector: String,
    /// Mean movement in projector pixels.
    pub mean: f64,
    /// Largest movement in projector pixels.
    pub max: f64,
    /// 0 = the camera sees the same room, 1 = a different view.
    pub camera_moved: f64,
}

/// What to do with a fresh camera frame while a pattern is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ready {
    /// Not yet: the camera does not show the pattern, or is still settling.
    Wait,
    /// Photograph it.
    Collect,
    /// The camera does not see this projector at all.
    Blind,
}

type WorkerResult = Result<Finished, String>;

/// A worker's answer.
enum Finished {
    Scan(Box<ScanData>),
    Drift(DriftReport),
}

/// One scan in progress.
pub struct ScanSession {
    /// What it is for.
    pub kind: Kind,
    camera: String,
    projectors: Vec<(String, u32, u32)>,
    current: usize,
    code: Option<GrayCode>,
    decoder: Option<Decoder>,
    frame: usize,
    shown: bool,
    shown_at: Instant,
    seen_at_show: u64,
    collected: Vec<Arc<Frame>>,
    collected_seen: u64,
    last_seen: u64,
    last_new_frame: Instant,
    white_color: Option<Rgba>,
    /// What the camera saw when the pattern went up (a small grey thumbnail).
    reference: Option<Vec<u8>>,
    /// The previous fresh frame's thumbnail, for the holding-still test.
    previous: Option<Vec<u8>>,
    /// When the camera started showing the new pattern.
    changed_at: Option<Instant>,
    /// The camera's lag behind the projector: the median of every clear change measured.
    lag: Option<Duration>,
    /// Each lag measured so far.
    lags: Vec<Duration>,
    /// Frames in a row the picture has held still.
    still: u32,
    /// When the camera was first seen sending pictures in this scan.
    live_since: Option<Instant>,
    /// The camera's photos of the projector white and black.
    white_gray: Option<Gray>,
    black_gray: Option<Gray>,
    /// The framing check of those photos.
    framing: Option<lumap_vision::framing::Framing>,
    /// Scan even though the framing check found problems.
    accept_framing: bool,
    /// The photo of the last stripe pattern, waiting for its inverse.
    last_pattern: Option<Gray>,
    /// The picture the scan sheet fills in.
    preview: Option<super::preview::Preview>,
    workers: Vec<Receiver<WorkerResult>>,
    scans: Vec<ScanData>,
    drifts: Vec<DriftReport>,
    /// Projectors that could not be scanned, and why.
    problems: Vec<String>,
    restore_open: Vec<String>,
    phase: Phase,
    root: PathBuf,
}

impl ScanSession {
    /// Starts scanning `projectors` (ids) with `camera`, keeping the results under `root`.
    pub fn start(
        kind: Kind,
        editor: &Editor,
        projectors: &[String],
        camera: &str,
        root: PathBuf,
        now: Instant,
    ) -> Self {
        let projectors = projectors
            .iter()
            .filter_map(|id| {
                editor
                    .project
                    .output(id)
                    .map(|o| (o.id.clone(), o.width, o.height))
            })
            .collect::<Vec<_>>();
        let phase = if projectors.is_empty() {
            Phase::Failed("There is no projector to scan.".into())
        } else {
            Phase::Showing
        };
        Self {
            kind,
            camera: camera.to_string(),
            projectors,
            current: 0,
            code: None,
            decoder: None,
            frame: 0,
            shown: false,
            shown_at: now,
            seen_at_show: 0,
            collected: Vec::new(),
            collected_seen: 0,
            last_seen: 0,
            last_new_frame: now,
            white_color: None,
            reference: None,
            previous: None,
            changed_at: None,
            lag: None,
            lags: Vec::new(),
            still: 0,
            live_since: None,
            white_gray: None,
            black_gray: None,
            framing: None,
            accept_framing: false,
            last_pattern: None,
            preview: None,
            workers: Vec::new(),
            scans: Vec::new(),
            drifts: Vec::new(),
            problems: Vec::new(),
            restore_open: editor.open_outputs.clone(),
            phase,
            root,
        }
    }

    /// Where it is.
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// True once it stopped, for whatever reason.
    pub fn finished(&self) -> bool {
        matches!(
            self.phase,
            Phase::Done | Phase::Failed(_) | Phase::Cancelled
        )
    }

    /// Progress for the sheet.
    pub fn progress(&self) -> Progress {
        let (projector, frames) = self
            .projectors
            .get(self.current.min(self.projectors.len().saturating_sub(1)))
            .map_or((String::new(), 0), |(id, w, h)| {
                (id.clone(), self.code_for(*w, *h).frames().len())
            });
        Progress {
            projector,
            index: self.current.min(self.projectors.len()),
            of: self.projectors.len(),
            frame: self.frame,
            frames,
            phase: self.phase.clone(),
        }
    }

    /// Drift reports of a finished drift check.
    pub fn drifts(&self) -> &[DriftReport] {
        &self.drifts
    }

    /// What the framing check found for the projector being scanned.
    pub fn framing(&self) -> Option<&lumap_vision::framing::Framing> {
        self.framing.as_ref()
    }

    /// The picture filling in as stripes are read.
    pub fn preview(&self) -> Option<&super::preview::Preview> {
        self.preview.as_ref()
    }

    /// After fixing the camera: photographs white and black again and checks again.
    pub fn retry(&mut self) {
        if matches!(self.phase, Phase::Adjust(_)) {
            self.phase = Phase::Showing;
            self.code = None;
            self.decoder = None;
            self.shown = false;
            self.framing = None;
            self.preview = None;
        }
    }

    /// Scans with the camera as it is, problems and all.
    pub fn accept(&mut self) {
        if matches!(self.phase, Phase::Adjust(_)) {
            self.accept_framing = true;
            self.phase = Phase::Showing;
            self.shown = false;
        }
    }

    /// Projectors that could not be scanned, and why (the others still were).
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// The camera's measured lag behind the projectors.
    pub fn lag(&self) -> Option<Duration> {
        self.lag
    }

    /// Stops it, putting the projectors back the way they were.
    pub fn cancel(&mut self, editor: &mut Editor) {
        if !self.finished() {
            self.phase = Phase::Cancelled;
        }
        self.restore(editor);
    }

    fn code_for(&self, w: u32, h: u32) -> GrayCode {
        match self.kind {
            Kind::Drift => GrayCode::coarse(w, h, DRIFT_BITS),
            Kind::Full | Kind::Realign => GrayCode::new(w, h),
        }
    }

    fn fail(&mut self, editor: &mut Editor, why: &str) {
        self.phase = Phase::Failed(why.to_string());
        self.restore(editor);
    }

    fn restore(&mut self, editor: &mut Editor) {
        editor.scan_pattern = None;
        editor.open_outputs.clone_from(&self.restore_open);
    }

    /// Advances the scan; call once per frame.
    pub fn tick(&mut self, editor: &mut Editor, hub: &mut CameraHub, now: Instant) {
        match self.phase {
            Phase::Showing => self.show(editor, hub, now),
            Phase::Decoding => self.collect_workers(editor),
            Phase::Adjust(_) => {
                // Keep the camera running so the sheet can show what it sees while aiming.
                let _ = hub.ensure(&self.camera, now);
            }
            Phase::Done | Phase::Failed(_) | Phase::Cancelled => {}
        }
    }

    fn show(&mut self, editor: &mut Editor, hub: &mut CameraHub, now: Instant) {
        if editor.show_mode {
            self.fail(editor, "Show mode started, so the scan stopped.");
            return;
        }
        let Some(stream) = hub.ensure(&self.camera, now) else {
            self.fail(editor, "The camera is not available.");
            return;
        };
        if let Some(e) = stream.error() {
            self.fail(editor, &format!("The camera stopped: {e}"));
            return;
        }
        let Some((id, w, h)) = self.projectors.get(self.current).cloned() else {
            self.phase = Phase::Decoding;
            editor.scan_pattern = None;
            return;
        };
        if !editor.open_outputs.contains(&id) {
            editor.open_outputs.push(id.clone());
        }
        if self.code.is_none() {
            let code = self.code_for(w, h);
            let (cw, ch) = stream.size();
            self.decoder = Some(Decoder::new(&code, cw, ch));
            self.code = Some(code);
            self.frame = 0;
            self.shown = false;
            self.white_color = None;
        }
        let seen = stream.frames_seen();
        if seen == self.last_seen {
            if now.duration_since(self.last_new_frame) > STALL {
                self.fail(editor, "The camera stopped sending pictures.");
                return;
            }
        } else {
            self.last_seen = seen;
            self.last_new_frame = now;
        }
        if seen > 0 {
            self.live_since.get_or_insert(now);
        }
        if self
            .live_since
            .is_none_or(|t| now.duration_since(t) < WARM_UP)
        {
            return;
        }
        let Some(code) = &self.code else { return };
        let frames = code.frames().to_vec();
        if !self.shown {
            let gray = code.render(&frames[self.frame]);
            editor.scan_pattern = Some((id, Arc::new(gray_frame(&gray))));
            self.shown = true;
            self.shown_at = now;
            self.seen_at_show = seen;
            self.collected.clear();
            self.collected_seen = seen;
            self.reference = stream.latest().map(|f| thumbnail(&f));
            self.previous = None;
            self.changed_at = None;
            self.still = 0;
            return;
        }
        if now.duration_since(self.shown_at) < SETTLE || seen <= self.collected_seen {
            return;
        }
        self.collected_seen = seen;
        let Some(fresh) = stream.latest() else { return };
        match self.ready(&fresh, frames[self.frame], now) {
            Ready::Wait => return,
            Ready::Collect => {}
            Ready::Blind => {
                self.problems.push(format!(
                    "The camera cannot see {id}'s light: aim it at the wall where {id} shines, \
                     or turn the room lights down."
                ));
                self.next_projector(editor);
                return;
            }
        }
        self.collected.push(fresh);
        if self.collected.len() < FRAMES_PER_PATTERN {
            return;
        }
        let Some(photo) = camera::median(&self.collected) else {
            return;
        };
        if frames[self.frame] == PatternFrame::White {
            self.white_color = Some(photo.clone());
        }
        if std::env::var_os("LUMAP_SCAN_DEBUG").is_some() {
            // Every photographed pattern, for looking at what the camera really saw.
            let dir = self.root.join(&id).join("captures");
            let path = dir.join(format!("{:02}.png", self.frame));
            let (w, h, data) = (photo.width, photo.height, photo.data.clone());
            std::thread::spawn(move || {
                let _ = std::fs::create_dir_all(&dir);
                if let Some(img) = image::RgbaImage::from_raw(w, h, data) {
                    let _ = img.save(path);
                }
            });
        }
        let gray = Gray::from_rgba(&photo);
        if let Some(d) = &mut self.decoder {
            d.add(self.frame, &gray);
        }
        let pattern = frames[self.frame];
        self.frame += 1;
        self.shown = false;
        if self.read(editor, &id, pattern, gray) {
            return;
        }
        if self.frame >= frames.len() {
            self.dispatch(editor, &id);
            self.current += 1;
            self.code = None;
            self.decoder = None;
            if self.current >= self.projectors.len() {
                self.phase = Phase::Decoding;
                editor.scan_pattern = None;
            }
        }
    }

    /// Uses each photo beyond the decoder: white and black give the framing check (and may
    /// pause the scan for the camera to be fixed); each stripe and its inverse sharpen the
    /// preview. Returns true when the scan paused.
    fn read(&mut self, editor: &mut Editor, id: &str, pattern: PatternFrame, gray: Gray) -> bool {
        match pattern {
            PatternFrame::White => self.white_gray = Some(gray),
            PatternFrame::Black => {
                self.black_gray = Some(gray);
                if let (Some(w), Some(b)) = (&self.white_gray, &self.black_gray) {
                    let framing = lumap_vision::framing::check(w, b);
                    self.preview = Some(super::preview::Preview::new(&framing.footprint));
                    let advice: Vec<String> =
                        framing.problems.iter().map(|p| p.advice()).collect();
                    let blind = framing.problems.iter().any(|p| p.is_blocking());
                    self.framing = Some(framing);
                    if !advice.is_empty() && (!self.accept_framing || blind) {
                        // Light the wall so the camera can be aimed by eye, and wait.
                        let (w, h) = self
                            .projectors
                            .get(self.current)
                            .map_or((1, 1), |(_, w, h)| (*w, *h));
                        editor.scan_pattern =
                            Some((id.to_string(), Arc::new(Frame::solid(w, h, [255; 4]))));
                        self.accept_framing = self.accept_framing && !blind;
                        self.frame = 0;
                        self.phase = Phase::Adjust(advice);
                        return true;
                    }
                }
            }
            PatternFrame::Bit { inverted: false, .. } => self.last_pattern = Some(gray),
            PatternFrame::Bit {
                axis,
                inverted: true,
                ..
            } => {
                if let (Some(p), Some(a), Some(w), Some(b)) = (
                    &mut self.preview,
                    &self.last_pattern,
                    &self.white_gray,
                    &self.black_gray,
                ) {
                    p.add(axis, a, &gray, w, b);
                }
            }
        }
        false
    }

    /// Decides, frame by frame, when the camera shows the pattern that is up and holds it still.
    /// First the picture must change from what the camera showed when the pattern went up (or,
    /// once the camera's lag is known, the lag must pass: stripes finer than the camera can see
    /// change nothing); then two frames in a row must agree (auto-exposure settling). A camera
    /// that does not react when its projector goes from white to black cannot see it.
    fn ready(&mut self, fresh: &Arc<Frame>, pattern: PatternFrame, now: Instant) -> Ready {
        let small = thumbnail(fresh);
        let waited = now.duration_since(self.shown_at);
        if self.changed_at.is_none() {
            // No picture yet when the pattern went up: the first one is what to compare with.
            let Some(reference) = &self.reference else {
                self.reference = Some(small);
                return Ready::Wait;
            };
            // Before half the known lag has passed, a change is the camera adjusting, not the
            // pattern arriving.
            let too_soon = self.lag.is_some_and(|lag| waited < lag / 2);
            let moved = !too_soon && reshaped(reference, &small) > CHANGED;
            let limit = self.lag.map_or(FIRST_WAIT, |lag| lag + LAG_MARGIN);
            self.previous = Some(small);
            if moved {
                self.changed_at = Some(now);
                // The lag is the median of the clear changes so far: one slow frame from a
                // phone streamed over Wi-Fi must not stretch every later wait.
                self.lags.push(waited);
                let mut sorted = self.lags.clone();
                sorted.sort_unstable();
                self.lag = Some(sorted[sorted.len() / 2]);
                return Ready::Wait;
            }
            if waited < limit {
                return Ready::Wait;
            }
            if pattern == PatternFrame::Black {
                return Ready::Blind;
            }
            self.changed_at = Some(now);
            return Ready::Collect;
        }
        // Still: the picture keeps its shape (a phone's exposure drifting on is fine; the
        // decoder evens exposure out) and its brightness hardly moves.
        let steady_now = self.previous.as_ref().is_some_and(|p| {
            reshaped(p, &small) < STEADY_SHAPE && difference(p, &small) < STEADY * 3.0
        });
        self.still = if steady_now { self.still + 1 } else { 0 };
        let faded_in = self
            .changed_at
            .is_some_and(|t| now.duration_since(t) >= AFTER_CHANGE);
        let steady = faded_in && self.still >= STEADY_FRAMES;
        self.previous = Some(small);
        let settled = self
            .changed_at
            .is_some_and(|t| now.duration_since(t) >= STEADY_WAIT);
        if steady || settled || !self.collected.is_empty() {
            Ready::Collect
        } else {
            Ready::Wait
        }
    }

    /// Gives up on the projector being photographed and moves to the next one.
    fn next_projector(&mut self, editor: &mut Editor) {
        self.current += 1;
        self.code = None;
        self.decoder = None;
        self.shown = false;
        if self.current >= self.projectors.len() {
            self.phase = Phase::Decoding;
            editor.scan_pattern = None;
        }
    }

    /// Sends the finished projector's photos to a worker.
    fn dispatch(&mut self, editor: &Editor, id: &str) {
        let (Some(decoder), Some(color)) = (self.decoder.take(), self.white_color.take()) else {
            return;
        };
        let (tx, rx) = channel();
        self.workers.push(rx);
        let kind = self.kind;
        let camera = self.camera.clone();
        let folder = self.root.join(id);
        let id = id.to_string();
        let old = editor.scans.get(&id);
        std::thread::spawn(move || {
            let scan = decoder.finish(&DecodeSettings::default());
            let result = match (kind, old) {
                (Kind::Drift, Some(old)) => {
                    let drift = realign::drift(&old.map, &scan.correspondence);
                    Ok(Finished::Drift(DriftReport {
                        projector: id,
                        mean: drift.mean,
                        max: drift.max,
                        camera_moved: realign::camera_moved(&old.white, &scan.white),
                    }))
                }
                (Kind::Drift, None) => Err(format!("{id} has no scan to compare with.")),
                (Kind::Full | Kind::Realign, _) => {
                    if scan.stats.valid_fraction < 0.02 {
                        Err(format!(
                            "The camera could not see {id}'s stripes. Point it at the wall and \
                             make the room darker."
                        ))
                    } else {
                        let data = ScanData::build(
                            &id,
                            folder,
                            &camera,
                            scan.correspondence,
                            scan.white,
                            scan.black,
                            &color,
                            &scan.stats,
                        );
                        data.save(&color).map(|()| Finished::Scan(Box::new(data)))
                    }
                }
            };
            let _ = tx.send(result);
        });
    }

    fn collect_workers(&mut self, editor: &mut Editor) {
        let mut pending = Vec::new();
        for rx in std::mem::take(&mut self.workers) {
            match rx.try_recv() {
                Ok(Ok(Finished::Scan(d))) => self.scans.push(*d),
                Ok(Ok(Finished::Drift(r))) => self.drifts.push(r),
                Ok(Err(e)) => self.problems.push(e),
                Err(std::sync::mpsc::TryRecvError::Empty) => pending.push(rx),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.problems.push("Decoding a scan failed.".into());
                }
            }
        }
        self.workers = pending;
        if self.workers.is_empty() {
            // One projector the camera cannot see does not cost the others their scans.
            if self.scans.is_empty() && self.drifts.is_empty() && !self.problems.is_empty() {
                let why = self.problems.join(" ");
                self.fail(editor, &why);
            } else {
                self.phase = Phase::Done;
                self.restore(editor);
            }
        }
    }

    /// Adopts finished scans into the show: re-aligns surfaces when asked, points each
    /// projector at its scan folder (one undo step) and hands the scans to the store.
    pub fn adopt(&mut self, editor: &mut Editor) -> usize {
        if self.phase != Phase::Done {
            return 0;
        }
        let scans = std::mem::take(&mut self.scans);
        if scans.is_empty() {
            return 0;
        }
        let base = editor.base_dir();
        let kind = self.kind;
        let moves: Vec<(String, Vec<Moved>)> = if kind == Kind::Realign {
            scans
                .iter()
                .filter_map(|new| {
                    let old = editor.scans.get(&new.projector)?;
                    Some((new.projector.clone(), realigned(editor, &old, new)))
                })
                .collect()
        } else {
            Vec::new()
        };
        let folders: Vec<(String, String)> = scans
            .iter()
            .map(|d| {
                let rel = d
                    .folder
                    .strip_prefix(&base)
                    .map_or_else(|_| d.folder.clone(), std::path::Path::to_path_buf);
                (
                    d.projector.clone(),
                    rel.to_string_lossy().replace('\\', "/"),
                )
            })
            .collect();
        editor.edit(move |p| {
            for (id, folder) in &folders {
                if let Some(o) = p.output_mut(id) {
                    o.scan = Some(folder.clone());
                }
            }
            for (_, surfaces) in &moves {
                for (sid, warp, masks) in surfaces {
                    if let Some(s) = p.surface_mut(sid) {
                        s.warp = warp.clone();
                        for (m, pts) in s.masks.iter_mut().zip(masks) {
                            m.points.clone_from(pts);
                        }
                    }
                }
            }
        });
        let n = scans.len();
        for d in scans {
            editor.scans.put(d);
        }
        n
    }
}

/// A surface's id, its moved warp and its moved cut-out points.
type Moved = (String, WarpKind, Vec<Vec<Vec2>>);

/// Every surface on the new scan's projector, with warp and cut-out points moved from where the
/// old scan saw them to where the new one does (normalised projector coordinates).
fn realigned(editor: &Editor, old: &ScanData, new: &ScanData) -> Vec<Moved> {
    let (w, h) = (f64::from(new.corr.proj_w), f64::from(new.corr.proj_h));
    let follow = |p: Vec2| {
        realign::realign_point(&old.map, &new.corr, Vec2::new(p.x * w, p.y * h))
            .map_or(p, |q| Vec2::new(q.x / w, q.y / h))
    };
    editor
        .project
        .surfaces_for_output(&new.projector)
        .iter()
        .map(|s| {
            let warp = s.warp.map_points(follow);
            let masks = s
                .masks
                .iter()
                .map(|m| m.points.iter().copied().map(follow).collect())
                .collect();
            (s.id.clone(), warp, masks)
        })
        .collect()
}

/// Thumbnail width and height: enough to see a pattern change, cheap to compare every frame.
const THUMB: (u32, u32) = (80, 45);

/// A small grey version of a camera frame.
fn thumbnail(f: &Frame) -> Vec<u8> {
    let (tw, th) = THUMB;
    let mut out = Vec::with_capacity((tw * th) as usize);
    for ty in 0..th {
        for tx in 0..tw {
            let x = (u64::from(tx) * u64::from(f.width) / u64::from(tw)) as usize;
            let y = (u64::from(ty) * u64::from(f.height) / u64::from(th)) as usize;
            let i = (y * f.width as usize + x) * 4;
            let p = f.rgba.get(i..i + 3).unwrap_or(&[0, 0, 0]);
            let l = (u32::from(p[0]) * 54 + u32::from(p[1]) * 183 + u32::from(p[2]) * 19) >> 8;
            out.push(l as u8);
        }
    }
    out
}

/// How differently two thumbnails are lit, ignoring overall brightness: 1 minus their
/// correlation (0 = the same picture, however bright; 1 = unrelated; up to 2 = inverted). Two
/// flat pictures have no shape to compare, so their mean brightness difference decides.
fn reshaped(a: &[u8], b: &[u8]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 1.0;
    }
    let n = a.len() as f64;
    let mean = |v: &[u8]| v.iter().map(|&x| f64::from(x)).sum::<f64>() / n;
    let (ma, mb) = (mean(a), mean(b));
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (&x, &y) in a.iter().zip(b) {
        let (dx, dy) = (f64::from(x) - ma, f64::from(y) - mb);
        ab += dx * dy;
        aa += dx * dx;
        bb += dy * dy;
    }
    // Below about two brightness levels of spread, a picture is flat (a black frame).
    let flat = 4.0 * n;
    if aa < flat || bb < flat {
        return ((ma - mb).abs() / 32.0).min(1.0);
    }
    1.0 - ab / (aa * bb).sqrt()
}

/// Mean absolute difference of two thumbnails, in brightness levels.
fn difference(a: &[u8], b: &[u8]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return f64::MAX;
    }
    let sum: u64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    sum as f64 / a.len() as f64
}

/// A grey pattern as an opaque frame for a projector window.
fn gray_frame(g: &Gray) -> Frame {
    Frame {
        width: g.width,
        height: g.height,
        rgba: g.data.iter().flat_map(|&v| [v, v, v, 255]).collect(),
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
