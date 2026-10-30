//! A stand-in camera for tests and the app's UI tests: frames come from a function of the frame
//! number, either on a timer or only when the test asks.

use super::{camera_error, lock, CameraBackend, CameraInfo, CameraStream, CaptureSettings, Slot};
use crate::{Frame, SourceError};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

/// Draws frame number `n` (counting from 0) of a fake camera.
pub type FrameFn = Arc<dyn Fn(u64) -> Frame + Send + Sync>;

/// Frames per second of [`FakeCamera::solid`].
const SOLID_FPS: u32 = 30;

/// A [`CameraBackend`] with made-up cameras.
///
/// Timed fakes ([`FakeCamera::new`], [`FakeCamera::solid`]) run a thread per open stream that
/// draws a frame `fps` times a second until the stream is dropped. Stepped fakes
/// ([`FakeCamera::stepped`]) only deliver a frame when the test calls
/// [`FakeCamera::advance`], so tests are deterministic. The `settings` passed to `open` are
/// ignored: frames come in whatever size the frame function draws.
pub struct FakeCamera {
    cameras: Vec<CameraInfo>,
    frames: FrameFn,
    fps: Option<u32>,
    streams: Mutex<Vec<Weak<FakeStream>>>,
    opens: AtomicUsize,
}

impl FakeCamera {
    /// Cameras whose frames a background thread draws `fps` times a second (at least once).
    pub fn new(cameras: Vec<CameraInfo>, fps: u32, frames: FrameFn) -> Self {
        Self::build(cameras, Some(fps.max(1)), frames)
    }

    /// Cameras whose frames move on only when [`FakeCamera::advance`] is called.
    pub fn stepped(cameras: Vec<CameraInfo>, frames: FrameFn) -> Self {
        Self::build(cameras, None, frames)
    }

    /// One camera called `name` showing a single colour, 30 frames a second.
    pub fn solid(name: &str, width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let frame = Frame::solid(width, height, rgba);
        Self::new(
            vec![CameraInfo::new(name, name)],
            SOLID_FPS,
            Arc::new(move |_| frame.clone()),
        )
    }

    fn build(cameras: Vec<CameraInfo>, fps: Option<u32>, frames: FrameFn) -> Self {
        Self {
            cameras,
            frames,
            fps,
            streams: Mutex::new(Vec::new()),
            opens: AtomicUsize::new(0),
        }
    }

    fn live_streams(&self) -> Vec<Arc<FakeStream>> {
        let mut streams = lock(&self.streams);
        streams.retain(|s| s.strong_count() > 0);
        streams.iter().filter_map(Weak::upgrade).collect()
    }

    /// Delivers the next frame to every stream this backend has open; returns how many got one.
    pub fn advance(&self) -> usize {
        let streams = self.live_streams();
        for s in &streams {
            s.step();
        }
        streams.len()
    }

    /// Stops every open stream with `reason`, as if the camera were unplugged.
    pub fn fail(&self, reason: &str) {
        for s in self.live_streams() {
            s.slot.fail(reason.to_string());
        }
    }

    /// How many streams `open` has started, so tests can check a camera is shared.
    pub fn opens(&self) -> usize {
        self.opens.load(Ordering::Acquire)
    }
}

impl CameraBackend for FakeCamera {
    fn list(&self) -> Vec<CameraInfo> {
        self.cameras.clone()
    }

    fn open(
        &self,
        device: &str,
        _settings: &CaptureSettings,
    ) -> Result<Arc<dyn CameraStream>, SourceError> {
        let camera = self
            .cameras
            .iter()
            .find(|c| device.is_empty() || c.name == device || c.id == device)
            .ok_or_else(|| camera_error(device, "no such camera is connected"))?;
        let first = (self.frames)(0);
        let stream = Arc::new(FakeStream {
            name: camera.name.clone(),
            size: (first.width, first.height),
            frames: Arc::clone(&self.frames),
            slot: Arc::new(Slot::default()),
        });
        if let Some(fps) = self.fps {
            stream.slot.publish(Arc::new(first));
            let period = Duration::from_secs(1) / fps;
            let weak = Arc::downgrade(&stream);
            let _ = std::thread::Builder::new()
                .name("lumap-fake-camera".into())
                .spawn(move || loop {
                    std::thread::sleep(period);
                    match weak.upgrade() {
                        Some(stream) => stream.step(),
                        None => break,
                    }
                });
        }
        lock(&self.streams).push(Arc::downgrade(&stream));
        self.opens.fetch_add(1, Ordering::AcqRel);
        Ok(stream)
    }
}

/// One open fake camera.
struct FakeStream {
    name: String,
    size: (u32, u32),
    frames: FrameFn,
    slot: Arc<Slot>,
}

impl FakeStream {
    /// Publishes the next frame in the sequence.
    fn step(&self) {
        let n = self.slot.seen();
        self.slot.publish(Arc::new((self.frames)(n)));
    }
}

impl CameraStream for FakeStream {
    fn latest(&self) -> Option<Arc<Frame>> {
        self.slot.latest()
    }
    fn frames_seen(&self) -> u64 {
        self.slot.seen()
    }
    fn size(&self) -> (u32, u32) {
        self.size
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn error(&self) -> Option<String> {
        self.slot.error()
    }
}
