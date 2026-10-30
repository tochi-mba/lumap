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

/// How long a pattern stays up before frames count: projector, window and camera latency.
const SETTLE: Duration = Duration::from_millis(350);
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
    workers: Vec<Receiver<WorkerResult>>,
    scans: Vec<ScanData>,
    drifts: Vec<DriftReport>,
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
            workers: Vec::new(),
            scans: Vec::new(),
            drifts: Vec::new(),
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
            return;
        }
        if now.duration_since(self.shown_at) < SETTLE || seen < self.seen_at_show + 2 {
            return;
        }
        if seen > self.collected_seen {
            if let Some(f) = stream.latest() {
                self.collected.push(f);
            }
            self.collected_seen = seen;
        }
        if self.collected.len() < FRAMES_PER_PATTERN {
            return;
        }
        let Some(photo) = camera::median(&self.collected) else {
            return;
        };
        if frames[self.frame] == PatternFrame::White {
            self.white_color = Some(photo.clone());
        }
        if let Some(d) = &mut self.decoder {
            d.add(self.frame, &Gray::from_rgba(&photo));
        }
        self.frame += 1;
        self.shown = false;
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
                Ok(Err(e)) => {
                    self.fail(editor, &e);
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => pending.push(rx),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.fail(editor, "Decoding a scan failed.");
                    return;
                }
            }
        }
        self.workers = pending;
        if self.workers.is_empty() {
            self.phase = Phase::Done;
            self.restore(editor);
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

/// A grey pattern as an opaque frame for a projector window.
fn gray_frame(g: &Gray) -> Frame {
    Frame {
        width: g.width,
        height: g.height,
        rgba: g.data.iter().flat_map(|&v| [v, v, v, 255]).collect(),
    }
}
