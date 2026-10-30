//! Webcams: list the cameras the system has and stream one as frames.
#![allow(dead_code, clippy::pedantic)]
//!
//! The default backend, [`FfmpegCamera`], runs the bundled `ffmpeg` as a sibling process, the
//! same way videos are decoded: DirectShow on Windows, AVFoundation on macOS, Video4Linux on
//! Linux. Which of those runs is the only platform choice, and it lives here in `live_input`;
//! the parsers and argument lists for all three are plain functions tested everywhere.
//!
//! A camera is opened once per device however many things watch it: the content source, a
//! scan and a tracker all get the same [`CameraStream`] from [`shared`]. A stream holds only
//! the newest frame (a single slot, never a queue), so a slow reader skips frames instead of
//! falling behind, and [`CameraStream::frames_seen`] tells a reader when a fresh one arrived.
//! [`FakeCamera`] stands in for real hardware in tests.

mod fake;
mod ffmpeg;
mod parse;
#[cfg(test)]
mod tests;

pub use fake::{FakeCamera, FrameFn};
pub use ffmpeg::FfmpegCamera;
pub use parse::{parse_avfoundation, parse_dshow, parse_v4l2};

use crate::{Frame, FrameSource, SourceError};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};

/// The size asked for when the settings leave it open; cameras without it use their default.
pub const PREFERRED_SIZE: (u32, u32) = (1280, 720);

/// A camera the system offers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CameraInfo {
    /// What the user picks and the project stores ("Integrated Camera").
    pub name: String,
    /// What ffmpeg opens: the DirectShow name, the AVFoundation index, or `/dev/videoN`.
    pub id: String,
}

impl CameraInfo {
    /// A camera called `name` that ffmpeg opens as `id`.
    pub fn new(name: &str, id: &str) -> Self {
        Self {
            name: name.to_string(),
            id: id.to_string(),
        }
    }
}

/// How to capture. `None` leaves a value to the camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CaptureSettings {
    /// Frame size; `None` asks for [`PREFERRED_SIZE`] and falls back to the camera's default.
    pub size: Option<(u32, u32)>,
    /// Frames per second; `None` is the camera's default.
    pub fps: Option<u32>,
}

/// A running camera. Cloned handles share one capture; it stops when the last one drops.
pub trait CameraStream: Send + Sync {
    /// The newest frame, or `None` before the first one arrives. Never a queue: a reader that
    /// polls slowly simply sees fewer frames.
    fn latest(&self) -> Option<Arc<Frame>>;
    /// How many frames have arrived so far; wait for it to grow to get a fresh frame.
    fn frames_seen(&self) -> u64;
    /// Frame size in pixels.
    fn size(&self) -> (u32, u32);
    /// The camera's name.
    fn name(&self) -> &str;
    /// Why the camera stopped, once it has (unplugged, taken by another app, ffmpeg ended).
    fn error(&self) -> Option<String>;
}

/// A way of reaching cameras: the real one ([`FfmpegCamera`]) or a stand-in ([`FakeCamera`]).
pub trait CameraBackend: Send + Sync {
    /// The cameras connected now, in the order the system lists them.
    fn list(&self) -> Vec<CameraInfo>;
    /// Starts a camera by name (or id); an empty `device` is the first camera.
    fn open(
        &self,
        device: &str,
        settings: &CaptureSettings,
    ) -> Result<Arc<dyn CameraStream>, SourceError>;
}

/// The default backend for this platform: ffmpeg found the way videos find it.
pub fn backend() -> Arc<dyn CameraBackend> {
    Arc::new(FfmpegCamera::default())
}

/// The error for a camera that cannot be opened. Cameras reuse the video error: a camera is a
/// live video, and the message reads "cannot play video <camera>: <reason>".
pub(crate) fn camera_error(device: &str, reason: impl Into<String>) -> SourceError {
    let label = if device.is_empty() { "camera" } else { device };
    SourceError::Video(PathBuf::from(label), reason.into())
}

/// Locks a mutex, carrying on with the data if a panicking thread poisoned it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The newest frame of a running camera, shared by the thread that fills it and everyone who
/// reads it.
#[derive(Default)]
struct Slot {
    latest: Mutex<Option<Arc<Frame>>>,
    seen: AtomicU64,
    error: Mutex<Option<String>>,
}

impl Slot {
    /// Makes `frame` the newest and returns the one it replaces.
    fn publish(&self, frame: Arc<Frame>) -> Option<Arc<Frame>> {
        let old = lock(&self.latest).replace(frame);
        self.seen.fetch_add(1, Ordering::AcqRel);
        old
    }

    fn latest(&self) -> Option<Arc<Frame>> {
        lock(&self.latest).clone()
    }

    fn seen(&self) -> u64 {
        self.seen.load(Ordering::Acquire)
    }

    /// Records why the camera stopped; the first reason wins.
    fn fail(&self, reason: String) {
        lock(&self.error).get_or_insert(reason);
    }

    fn error(&self) -> Option<String> {
        lock(&self.error).clone()
    }
}

type Registry = Mutex<HashMap<String, Weak<dyn CameraStream>>>;

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::default)
}

/// The running stream for `device`, starting it through `backend` when nothing has it open.
///
/// One device runs one capture however many callers share it; the settings apply only when
/// this call starts the camera. A stream that has stopped with an error is replaced by a fresh
/// one. An empty `device` means the first camera `backend` lists. Streams are keyed by the
/// device string, so callers should pass the camera's name as the list gives it.
pub fn shared(
    backend: &dyn CameraBackend,
    device: &str,
    settings: &CaptureSettings,
) -> Result<Arc<dyn CameraStream>, SourceError> {
    let first;
    let device = if device.is_empty() {
        first = backend
            .list()
            .into_iter()
            .next()
            .ok_or_else(|| camera_error(device, "no camera is connected"))?;
        first.name.as_str()
    } else {
        device
    };
    let mut streams = lock(registry());
    let live = streams
        .get(device)
        .and_then(Weak::upgrade)
        .filter(|s| s.error().is_none());
    if let Some(stream) = live {
        return Ok(stream);
    }
    let stream = backend.open(device, settings)?;
    streams.retain(|_, s| s.strong_count() > 0);
    streams.insert(device.to_string(), Arc::downgrade(&stream));
    Ok(stream)
}

/// A camera as a content source: always its newest frame, shared rather than copied.
struct CameraSource {
    stream: Arc<dyn CameraStream>,
}

impl FrameSource for CameraSource {
    fn frame(&mut self, _t: f64) -> Option<Arc<Frame>> {
        self.stream.latest()
    }
    fn is_live(&self) -> bool {
        true
    }
}

/// Opens a camera as a live content source through the default backend. An empty `device` is
/// the first camera.
pub fn open_source(id: &str, device: &str) -> Result<Box<dyn FrameSource>, SourceError> {
    open_source_with(backend().as_ref(), id, device)
}

/// [`open_source`] through a given backend, so tests and previews can use a [`FakeCamera`].
/// `id` is the project source the camera feeds; it names the source when no capture API exists
/// on this platform.
pub fn open_source_with(
    backend: &dyn CameraBackend,
    id: &str,
    device: &str,
) -> Result<Box<dyn FrameSource>, SourceError> {
    match shared(backend, device, &CaptureSettings::default()) {
        Ok(stream) => Ok(Box::new(CameraSource { stream })),
        Err(SourceError::Unavailable(_, what)) => {
            Err(SourceError::Unavailable(id.to_string(), what))
        }
        Err(e) => Err(e),
    }
}
