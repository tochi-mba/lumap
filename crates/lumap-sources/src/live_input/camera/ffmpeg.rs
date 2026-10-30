//! Cameras through an `ffmpeg` sibling process.
//!
//! Listing runs ffmpeg's device list (DirectShow, AVFoundation) or reads `/dev` (Video4Linux).
//! Capturing runs `ffmpeg -f <api> -i <camera> -pix_fmt rgba -f rawvideo -`: a thread reads the
//! stderr banner for the frame size and keeps ffmpeg's last words, another slices stdout into
//! frames and publishes each to the stream's slot. The process is killed when the stream drops.

use super::parse::{parse_avfoundation, parse_dshow, parse_v4l2, Banner};
use super::PREFERRED_SIZE;
use super::{camera_error, lock, CameraBackend, CameraInfo, CameraStream, CaptureSettings, Slot};
use crate::video::Tools;
use crate::{Frame, SourceError};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Stdio};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// How long a camera may take to print its stream before it counts as not starting.
const START_TIMEOUT: Duration = Duration::from_secs(10);
/// Lines of ffmpeg's stderr kept for error messages.
const LOG_LINES: usize = 12;
/// DirectShow's real-time buffer: room for a few raw 720p frames without letting a stalled
/// reader build up seconds of lag.
const DSHOW_BUFFER: &str = "16M";
/// AVFoundation's default rate (29.97) is refused by many Mac cameras; ask for 30 instead.
const AVFOUNDATION_FPS: u32 = 30;

/// How ffmpeg reaches cameras on one operating system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Api {
    /// DirectShow (Windows): `-f dshow -i video=<name>`.
    Dshow,
    /// AVFoundation (macOS): `-f avfoundation -i <index>:none`.
    AvFoundation,
    /// Video4Linux2 (Linux): `-f v4l2 -i /dev/videoN`, with the nodes found in this folder.
    V4l2(PathBuf),
}

impl Api {
    /// The capture API of the platform lumap was built for.
    #[cfg(windows)]
    pub(super) fn native() -> Option<Self> {
        Some(Self::Dshow)
    }

    /// The capture API of the platform lumap was built for.
    #[cfg(target_os = "macos")]
    pub(super) fn native() -> Option<Self> {
        Some(Self::AvFoundation)
    }

    /// The capture API of the platform lumap was built for.
    #[cfg(target_os = "linux")]
    pub(super) fn native() -> Option<Self> {
        Some(Self::V4l2(PathBuf::from("/dev")))
    }

    /// No capture API is known on this platform.
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    pub(super) fn native() -> Option<Self> {
        None
    }

    /// True when `device` is already something ffmpeg opens (an index, a device path), so a
    /// camera missing from the list can still be tried.
    fn is_id(&self, device: &str) -> bool {
        match self {
            Self::Dshow => true,
            Self::AvFoundation => device.bytes().all(|b| b.is_ascii_digit()),
            Self::V4l2(_) => device.contains('/'),
        }
    }
}

/// Capture settings to try in order: what was asked for (or [`PREFERRED_SIZE`] when the size
/// was left open), then the camera's own size, then the camera's own everything.
pub(super) fn attempts(settings: &CaptureSettings) -> Vec<CaptureSettings> {
    let mut list = vec![
        CaptureSettings {
            size: Some(settings.size.unwrap_or(PREFERRED_SIZE)),
            fps: settings.fps,
        },
        CaptureSettings {
            size: None,
            fps: settings.fps,
        },
        CaptureSettings::default(),
    ];
    list.dedup();
    list
}

/// ffmpeg's arguments for capturing camera `id` as raw RGBA on stdout. `fps_mode` passes each
/// camera frame through untouched (no duplicates); ffmpeg before 5.1 lacks the option.
pub(super) fn capture_args(
    api: &Api,
    id: &str,
    settings: &CaptureSettings,
    fps_mode: bool,
) -> Vec<String> {
    let mut args: Vec<String> = ["-hide_banner", "-nostdin", "-nostats", "-loglevel", "info"]
        .map(String::from)
        .to_vec();
    let format = match api {
        Api::Dshow => "dshow",
        Api::AvFoundation => "avfoundation",
        Api::V4l2(_) => "v4l2",
    };
    args.extend(["-f".into(), format.into()]);
    if *api == Api::Dshow {
        args.extend(["-rtbufsize".into(), DSHOW_BUFFER.into()]);
    }
    if let Some((w, h)) = settings.size {
        args.extend(["-video_size".into(), format!("{w}x{h}")]);
    }
    let fps = match api {
        Api::AvFoundation => Some(settings.fps.unwrap_or(AVFOUNDATION_FPS)),
        _ => settings.fps,
    };
    if let Some(fps) = fps {
        args.extend(["-framerate".into(), fps.to_string()]);
    }
    let input = match api {
        Api::Dshow => format!("video={id}"),
        Api::AvFoundation => format!("{id}:none"),
        Api::V4l2(_) => id.to_string(),
    };
    args.extend(["-i".into(), input, "-an".into(), "-sn".into()]);
    if fps_mode {
        args.extend(["-fps_mode".into(), "passthrough".into()]);
    }
    args.extend(["-pix_fmt", "rgba", "-f", "rawvideo", "-"].map(String::from));
    args
}

/// Every entry of a folder, as path strings (empty when it cannot be read).
fn entries(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The last lines ffmpeg printed, joined for a message.
fn last_words(log: &Mutex<VecDeque<String>>) -> String {
    let log = lock(log);
    if log.is_empty() {
        return "ffmpeg printed nothing".into();
    }
    let skip = log.len().saturating_sub(3);
    log.iter()
        .skip(skip)
        .cloned()
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Cameras through ffmpeg: the default [`CameraBackend`].
pub struct FfmpegCamera {
    tools: Tools,
    api: Option<Api>,
}

impl Default for FfmpegCamera {
    /// ffmpeg found by [`Tools::discover`]: the setting, the bundled copy, then the PATH.
    fn default() -> Self {
        Self::new(Tools::discover().0)
    }
}

impl FfmpegCamera {
    /// Cameras through the given ffmpeg, using this platform's capture API.
    pub fn new(tools: Tools) -> Self {
        Self::with_api(tools, Api::native())
    }

    /// Cameras through the given ffmpeg and capture API (tests run every API everywhere).
    pub(super) fn with_api(tools: Tools, api: Option<Api>) -> Self {
        Self { tools, api }
    }

    /// The stderr of an ffmpeg device listing (empty when ffmpeg cannot run).
    fn run_list(&self, args: &[&str]) -> String {
        crate::live_input::background(&self.tools.ffmpeg)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map(|out| String::from_utf8_lossy(&out.stderr).into_owned())
            .unwrap_or_default()
    }

    /// The camera `device` names: the first one when empty, else a listed name or id, else the
    /// device itself when it already looks like something ffmpeg opens.
    fn resolve(&self, api: &Api, device: &str) -> Result<CameraInfo, SourceError> {
        if device.is_empty() {
            return self
                .list()
                .into_iter()
                .next()
                .ok_or_else(|| camera_error(device, "no camera is connected"));
        }
        if *api == Api::Dshow {
            return Ok(CameraInfo::new(device, device));
        }
        match self
            .list()
            .into_iter()
            .find(|c| c.name == device || c.id == device)
        {
            Some(camera) => Ok(camera),
            None if api.is_id(device) => Ok(CameraInfo::new(device, device)),
            None => Err(camera_error(device, "no camera by that name is connected")),
        }
    }
}

impl CameraBackend for FfmpegCamera {
    fn list(&self) -> Vec<CameraInfo> {
        match &self.api {
            None => Vec::new(),
            Some(Api::Dshow) => parse_dshow(&self.run_list(&[
                "-hide_banner",
                "-list_devices",
                "true",
                "-f",
                "dshow",
                "-i",
                "dummy",
            ])),
            Some(Api::AvFoundation) => parse_avfoundation(&self.run_list(&[
                "-hide_banner",
                "-f",
                "avfoundation",
                "-list_devices",
                "true",
                "-i",
                "",
            ])),
            Some(Api::V4l2(dev)) => parse_v4l2(&entries(dev)),
        }
    }

    fn open(
        &self,
        device: &str,
        settings: &CaptureSettings,
    ) -> Result<Arc<dyn CameraStream>, SourceError> {
        let Some(api) = &self.api else {
            return Err(SourceError::Unavailable(device.to_string(), "camera"));
        };
        let camera = self.resolve(api, device)?;
        let attempts = attempts(settings);
        let (mut i, mut fps_mode, mut reason) = (0, true, String::new());
        while let Some(attempt) = attempts.get(i) {
            match FfmpegStream::start(&self.tools, api, &camera, attempt, fps_mode) {
                Ok(stream) => return Ok(Arc::new(stream)),
                Err(why) => {
                    if fps_mode && why.contains("fps_mode") {
                        fps_mode = false;
                    } else {
                        i += 1;
                    }
                    reason = why;
                }
            }
        }
        Err(camera_error(&camera.name, reason))
    }
}

/// A camera captured by one ffmpeg process.
pub(super) struct FfmpegStream {
    name: String,
    size: (u32, u32),
    slot: Arc<Slot>,
    child: Child,
}

impl FfmpegStream {
    /// Starts ffmpeg and waits for the banner to give the frame size. On failure, returns
    /// ffmpeg's last words.
    pub(super) fn start(
        tools: &Tools,
        api: &Api,
        camera: &CameraInfo,
        settings: &CaptureSettings,
        fps_mode: bool,
    ) -> Result<Self, String> {
        let mut child = crate::live_input::background(&tools.ffmpeg)
            .args(capture_args(api, &camera.id, settings, fps_mode))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run {}: {e}", tools.ffmpeg.display()))?;
        let log = Arc::new(Mutex::new(VecDeque::new()));
        let (size_tx, size_rx) = channel();
        let drain = child.stderr.take().and_then(|stderr| {
            let log = Arc::clone(&log);
            std::thread::Builder::new()
                .name("lumap-camera-log".into())
                .spawn(move || {
                    let mut banner = Banner::default();
                    let mut size_tx = Some(size_tx);
                    let mut reader = BufReader::new(stderr);
                    let mut buf = Vec::new();
                    while reader.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
                        let text = String::from_utf8_lossy(&buf).into_owned();
                        buf.clear();
                        for line in text.split(['\r', '\n']).map(str::trim) {
                            if line.is_empty() {
                                continue;
                            }
                            let mut log = lock(&log);
                            if log.len() == LOG_LINES {
                                log.pop_front();
                            }
                            log.push_back(line.to_string());
                            if let Some(size) = banner.feed(line) {
                                if let Some(tx) = size_tx.take() {
                                    let _ = tx.send(size);
                                }
                            }
                        }
                    }
                })
                .ok()
        });
        let stdout = child.stdout.take();
        match (size_rx.recv_timeout(START_TIMEOUT), stdout) {
            (Ok(size), Some(stdout)) => {
                let slot = Arc::new(Slot::default());
                let reader_slot = Arc::clone(&slot);
                let _ = std::thread::Builder::new()
                    .name("lumap-camera".into())
                    .spawn(move || read_frames(stdout, size, &reader_slot, &log, drain));
                Ok(Self {
                    name: camera.name.clone(),
                    size,
                    slot,
                    child,
                })
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                if let Some(drain) = drain {
                    let _ = drain.join();
                }
                Err(last_words(&log))
            }
        }
    }
}

/// Slices raw RGBA from ffmpeg's stdout into frames until it ends, then records why. The frame
/// a new one replaces is reused for the next read when no one else still holds it, so a steady
/// stream allocates nothing.
fn read_frames(
    mut out: ChildStdout,
    (width, height): (u32, u32),
    slot: &Slot,
    log: &Mutex<VecDeque<String>>,
    drain: Option<JoinHandle<()>>,
) {
    let bytes = width as usize * height as usize * 4;
    let mut spare: Option<Arc<Frame>> = None;
    loop {
        let mut rgba = spare
            .take()
            .and_then(|f| Arc::try_unwrap(f).ok())
            .map_or_else(|| vec![0; bytes], |f| f.rgba);
        if out.read_exact(&mut rgba).is_err() {
            break;
        }
        spare = slot.publish(Arc::new(Frame {
            width,
            height,
            rgba,
        }));
    }
    if let Some(drain) = drain {
        let _ = drain.join();
    }
    slot.fail(format!("the camera stopped: {}", last_words(log)));
}

impl CameraStream for FfmpegStream {
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

impl Drop for FfmpegStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
