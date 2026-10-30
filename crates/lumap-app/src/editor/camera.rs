//! The show's camera: which one, its live picture (one stream shared by the Setup preview, the
//! scanner, the trackers, the lab and any Camera content), and the cameras the system has.
//!
//! The camera only runs while something uses it: every user calls [`CameraHub::ensure`] each
//! frame it needs pictures, and [`CameraHub::tick`] closes the stream after a few idle seconds.

use lumap_sources::live_input::camera::{
    self, CameraBackend, CameraInfo, CameraStream, CaptureSettings,
};
use lumap_sources::Frame;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long the stream stays open after its last user.
const IDLE_CLOSE: Duration = Duration::from_secs(4);

/// What the camera is doing, for pills and the lab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraStatus {
    /// Not running.
    Off,
    /// Streaming.
    On {
        /// Device name.
        name: String,
        /// Picture size.
        size: (u32, u32),
    },
    /// It failed; the message says why.
    Failed(String),
}

/// The show's camera.
pub struct CameraHub {
    backend: Arc<dyn CameraBackend>,
    devices: Option<Vec<CameraInfo>>,
    stream: Option<(String, Arc<dyn CameraStream>)>,
    last_used: Option<Instant>,
    error: Option<String>,
}

impl CameraHub {
    /// A hub over `backend` (the platform's camera, or a fake one in tests).
    pub fn new(backend: Arc<dyn CameraBackend>) -> Self {
        Self {
            backend,
            devices: None,
            stream: None,
            last_used: None,
            error: None,
        }
    }

    /// The platform's cameras.
    pub fn platform() -> Self {
        Self::new(camera::backend())
    }

    /// The cameras the system has (listed once, then on [`CameraHub::refresh`]).
    pub fn devices(&mut self) -> &[CameraInfo] {
        if self.devices.is_none() {
            self.devices = Some(self.backend.list());
        }
        self.devices.as_deref().unwrap_or_default()
    }

    /// Lists the cameras again (a camera was plugged in).
    pub fn refresh(&mut self) {
        self.devices = None;
    }

    /// The camera a show means: its own choice, or the first camera when it has none.
    pub fn resolve(&mut self, chosen: &str) -> Option<String> {
        if !chosen.is_empty() {
            return Some(chosen.to_string());
        }
        self.devices().first().map(|d| d.name.clone())
    }

    /// The live stream of `device`, opened if needed. Call it every frame the camera is wanted.
    pub fn ensure(&mut self, device: &str, now: Instant) -> Option<Arc<dyn CameraStream>> {
        self.last_used = Some(now);
        if let Some((name, s)) = &self.stream {
            if name == device {
                return Some(Arc::clone(s));
            }
        }
        self.stream = None;
        let id = self
            .devices()
            .iter()
            .find(|d| d.name == device)
            .map_or_else(|| device.to_string(), |d| d.name.clone());
        match camera::shared(self.backend.as_ref(), &id, &CaptureSettings::default()) {
            Ok(s) => {
                self.error = None;
                self.stream = Some((device.to_string(), Arc::clone(&s)));
                Some(s)
            }
            Err(e) => {
                self.error = Some(e.to_string());
                None
            }
        }
    }

    /// The newest picture of `device`, opening it if needed.
    pub fn latest(&mut self, device: &str, now: Instant) -> Option<Arc<Frame>> {
        self.ensure(device, now).and_then(|s| s.latest())
    }

    /// Closes the stream when nothing has used it for a while.
    pub fn tick(&mut self, now: Instant) {
        if self
            .last_used
            .is_some_and(|t| now.duration_since(t) > IDLE_CLOSE)
        {
            self.stream = None;
            self.last_used = None;
        }
    }

    /// Stops the camera now.
    pub fn close(&mut self) {
        self.stream = None;
        self.last_used = None;
    }

    /// What the camera is doing.
    pub fn status(&self) -> CameraStatus {
        if let Some(e) = &self.error {
            return CameraStatus::Failed(e.clone());
        }
        match &self.stream {
            Some((name, s)) => match s.error() {
                Some(e) => CameraStatus::Failed(e),
                None => CameraStatus::On {
                    name: name.clone(),
                    size: s.size(),
                },
            },
            None => CameraStatus::Off,
        }
    }

    /// The open stream, if any, without keeping it alive.
    pub fn current(&self) -> Option<Arc<dyn CameraStream>> {
        self.stream.as_ref().map(|(_, s)| Arc::clone(s))
    }
}

/// Converts a camera frame into the vision crate's RGBA buffer (one copy of the bytes).
pub fn to_rgba(frame: &Frame) -> lumap_vision::Rgba {
    lumap_vision::Rgba {
        width: frame.width,
        height: frame.height,
        data: frame.rgba.clone(),
    }
}

/// Converts a vision RGBA buffer into a frame.
pub fn to_frame(image: lumap_vision::Rgba) -> Frame {
    Frame {
        width: image.width,
        height: image.height,
        rgba: image.data,
    }
}

/// The per-pixel median of a few frames: steadier than any one webcam frame.
pub fn median(frames: &[Arc<Frame>]) -> Option<lumap_vision::Rgba> {
    let first = frames.first()?;
    let (w, h) = (first.width, first.height);
    if frames.iter().any(|f| f.width != w || f.height != h) {
        return Some(to_rgba(first));
    }
    let n = frames.len();
    let mut out = vec![0u8; first.rgba.len()];
    let mut values = Vec::with_capacity(n);
    for (i, o) in out.iter_mut().enumerate() {
        values.clear();
        values.extend(frames.iter().map(|f| f.rgba[i]));
        values.sort_unstable();
        *o = values[n / 2];
    }
    Some(lumap_vision::Rgba {
        width: w,
        height: h,
        data: out,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use lumap_sources::live_input::camera::FakeCamera;

    fn hub() -> CameraHub {
        CameraHub::new(Arc::new(FakeCamera::solid(
            "Fake cam",
            8,
            4,
            [10, 20, 30, 255],
        )))
    }

    #[test]
    fn the_hub_lists_resolves_opens_and_closes_when_idle() {
        let mut h = hub();
        assert_eq!(h.devices().len(), 1);
        assert_eq!(h.resolve("").as_deref(), Some("Fake cam"));
        assert_eq!(h.resolve("Other").as_deref(), Some("Other"));
        assert_eq!(h.status(), CameraStatus::Off);
        let now = Instant::now();
        let s = h.ensure("Fake cam", now).unwrap();
        assert_eq!(s.size(), (8, 4));
        assert!(matches!(h.status(), CameraStatus::On { .. }));
        assert!(h.current().is_some());
        h.tick(now + Duration::from_secs(1));
        assert!(h.current().is_some(), "still in use");
        h.tick(now + Duration::from_secs(10));
        assert!(h.current().is_none(), "closed when idle");
        h.refresh();
        assert_eq!(h.devices().len(), 1);
        h.ensure("Fake cam", now);
        h.close();
        assert_eq!(h.status(), CameraStatus::Off);
    }

    #[test]
    fn the_median_of_frames_drops_a_flash() {
        let f = |v: u8| Arc::new(Frame::solid(2, 1, [v, v, v, 255]));
        let m = median(&[f(10), f(250), f(12)]).unwrap();
        assert_eq!(&m.data[..4], &[12, 12, 12, 255]);
        assert!(median(&[]).is_none());
        let odd = median(&[f(1), Arc::new(Frame::solid(1, 1, [9, 9, 9, 255]))]).unwrap();
        assert_eq!(
            odd.width, 2,
            "mismatched sizes fall back to the first frame"
        );
        let back = to_frame(to_rgba(&f(7)));
        assert_eq!(back.width, 2);
    }
}
