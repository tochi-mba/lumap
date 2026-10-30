//! The lab: a local automation API (`lumap --lab`) that lets a script drive lumap, look
//! through the camera and see what every projector shows, for testing on real hardware.
//!
//! [`LabServer::bind`] listens on `127.0.0.1` only. Every request needs the session token in an
//! `X-Lumap-Lab` header, a `Host` of `127.0.0.1:<port>` or `localhost:<port>` and no `Origin`
//! header, so a web page open in a browser can never reach it (CSRF and DNS rebinding).
//! [`route`] turns an authorised request into a [`LabRequest`]; the app takes those with
//! [`LabServer::drain`] once per frame and answers each [`Pending`] with a [`Reply`], in that
//! frame or a later one. A server thread waits for the answer (30 s, or 15 minutes for scans,
//! recordings and re-aligns) and writes it back as JSON or PNG. `GET /lab/ping` is answered by
//! the server itself, so it works while the app is busy or frozen.
//!
//! `docs/lab.md` documents every endpoint; `scripts/lab/lab.py` is the client.
//!
//! ```no_run
//! use lumap_control::lab::{session_dir, LabRequest, LabServer, Reply, DEFAULT_PORT};
//!
//! let lab = LabServer::bind(DEFAULT_PORT)?;
//! if let Some(dir) = session_dir() {
//!     lab.write_session(&dir)?;
//! }
//! // Once per frame:
//! for pending in lab.drain() {
//!     let reply = match &pending.request {
//!         LabRequest::State => Reply::Json(serde_json::json!({ "step": 1 })),
//!         _ => Reply::Error(501, "not built yet".into()),
//!     };
//!     pending.reply(reply);
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod routes;
mod server;

#[cfg(test)]
mod route_tests;
#[cfg(test)]
mod tests;

pub use routes::route;
pub use server::{session_dir, LabServer, DEFAULT_PORT};

use serde::{Deserialize, Serialize};
use std::sync::mpsc::{channel, Receiver, Sender};

/// Something a lab client asked the app for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabRequest {
    /// `GET /lab/state`: the editor's state as JSON.
    State,
    /// `POST /lab/act`: run these actions in order.
    Act(Vec<Action>),
    /// `GET /lab/camera.png`: a camera photo, the median of `frames` frames taken after waiting
    /// `settle_ms` for the projector and the camera's exposure to settle.
    Camera {
        /// Milliseconds to wait before the first frame.
        settle_ms: u64,
        /// Frames to take the per-pixel median of.
        frames: u32,
    },
    /// `GET /lab/camera.json`: the camera's size, frame rate and brightness.
    CameraInfo,
    /// `GET /lab/projector/<id>.png`: the exact frame a projector shows.
    Projector {
        /// Projector (output) id.
        id: String,
    },
    /// `GET /lab/ui.png`: a screenshot of the editor window.
    Ui,
    /// `GET /lab/ui/tree`: the editor's widgets (role, label, value, rectangle) as JSON.
    UiTree,
    /// `POST /lab/ui`: one input event, injected as real input at the start of a frame.
    UiInput(UiEvent),
    /// `POST /lab/record`: camera frames saved as a PNG sequence, for test fixtures.
    Record {
        /// How long to record, more than 0 and at most 600.
        seconds: f64,
    },
    /// `POST /lab/scan`: a structured-light scan of one projector, or of every projector.
    Scan {
        /// Projector id, or `None` for all of them.
        projector: Option<String>,
    },
    /// `GET /lab/scan/<id>/<which>.png`: an image from a projector's last scan.
    ScanImage {
        /// Projector id.
        projector: String,
        /// Which image.
        which: ScanImage,
    },
    /// `GET /lab/found/<id>`: the surfaces found on a projector's part of the wall, as JSON.
    Found {
        /// Projector id.
        projector: String,
    },
    /// `POST /lab/realign`: check the rig for drift and re-align the projectors.
    Realign,
    /// `GET /lab/vision/<name>.png`: a debug image from the vision code (edges, facets, ...).
    Vision {
        /// Debug image name.
        name: String,
    },
}

/// One step of `POST /lab/act`, mirroring an editor command. On the wire it is an object named
/// by `do` in snake case, for example `{"do": "step", "step": 2}`; unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Open a show file.
    Open {
        /// Path of the show file.
        path: String,
    },
    /// Open the demo show.
    Demo,
    /// Start a new, empty show.
    New,
    /// Save the show.
    Save {
        /// Where to save; the show's own file when `None`.
        path: Option<String>,
    },
    /// Go to a step: 1 Setup, 2 Map, 3 Content, 4 Show.
    Step {
        /// The step, 1 to 4.
        step: u8,
    },
    /// Select something, or clear the selection.
    Select {
        /// `projector`, `surface`, `source`, `cue` or `none`.
        what: String,
        /// Its id.
        id: Option<String>,
    },
    /// Add a surface to a projector.
    AddSurface {
        /// Projector id.
        projector: String,
    },
    /// Move a surface's corner pins.
    SetCorners {
        /// Surface id.
        surface: String,
        /// Top-left, top-right, bottom-right, bottom-left, in normalised output coordinates
        /// (0..1, origin top-left) like lumap's quad corners.
        corners: [[f64; 2]; 4],
    },
    /// Apply a look from the library.
    ApplyLook {
        /// Look name.
        look: String,
        /// Surface id; the selected surface when `None`.
        surface: Option<String>,
    },
    /// Set a parameter, the way OSC does.
    Param {
        /// Parameter address, for example `/show/master`.
        address: String,
        /// Number, bool or text.
        value: serde_json::Value,
    },
    /// Store the current look of the show as a cue.
    StoreCue {
        /// Cue name.
        name: String,
        /// Fade time in seconds, not negative.
        fade: f64,
    },
    /// GO a cue.
    Go {
        /// Cue id; the next cue when `None`.
        cue: Option<String>,
    },
    /// Show every projector's name on its output (Identify).
    Identify,
    /// Set or clear a projector's calibration overlay.
    Overlay {
        /// Projector id.
        projector: String,
        /// `grid`, `crosshair`, `bars`, `grey_steps` or `blend_check`; `None` clears it.
        overlay: Option<String>,
    },
    /// Show a test pattern full-screen on a projector, over the show.
    ShowPattern {
        /// Projector id.
        projector: String,
        /// What to show.
        pattern: Pattern,
    },
    /// Open a projector's output window on its display (nothing when it is open already).
    OpenWindow {
        /// Projector id.
        projector: String,
    },
    /// Close a projector's output window.
    CloseWindow {
        /// Projector id.
        projector: String,
    },
    /// Go live (show mode).
    GoLive,
    /// Leave show mode.
    LeaveLive,
    /// Undo the last edit.
    Undo,
    /// Redo the last undone edit.
    Redo,
    /// Pause between actions; the waits of one request may add up to 20 s at most.
    Wait {
        /// Milliseconds.
        ms: u64,
    },
    /// Use a camera.
    Camera {
        /// Device name, as `GET /lab/camera.json` lists them.
        device: String,
    },
}

/// A test pattern for [`Action::ShowPattern`], named by `kind`: `{"kind": "checker", "size": 64}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Pattern {
    /// One colour over the whole output.
    Solid {
        /// Red, green and blue, 0 to 255.
        rgb: [u8; 3],
    },
    /// lumap's alignment grid.
    Grid,
    /// A black and white checkerboard.
    Checker {
        /// Square size in output pixels, at least 1.
        size: u32,
    },
    /// A PNG file, stretched over the output.
    Png {
        /// Path of the PNG file.
        path: String,
    },
    /// Take the pattern away and show the show again.
    Clear,
}

/// One input event for `POST /lab/ui`, named by `kind`. Widgets are found by their accessible
/// label, the names the kittest suite clicks; positions are editor points from the window's
/// top-left corner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum UiEvent {
    /// Click a widget.
    Click {
        /// Its label.
        label: String,
        /// Which one when several share the label, from 0; the only one when `None`.
        nth: Option<usize>,
    },
    /// Click a position.
    ClickAt {
        /// Points from the left.
        x: f32,
        /// Points from the top.
        y: f32,
    },
    /// Type text.
    Type {
        /// The field to type into; the focused one when `None`.
        label: Option<String>,
        /// What to type.
        text: String,
    },
    /// Press a key.
    Key {
        /// egui key name, such as `Enter`, `Escape`, `F10` or `A`.
        key: String,
        /// Held modifiers: `ctrl`, `shift`, `alt`, `command`.
        #[serde(default)]
        modifiers: Vec<String>,
    },
    /// Drag with the primary button.
    Drag {
        /// Start position in points.
        from: [f32; 2],
        /// End position in points.
        to: [f32; 2],
    },
    /// Turn the mouse wheel over a position.
    Scroll {
        /// Points from the left.
        x: f32,
        /// Points from the top.
        y: f32,
        /// Points to scroll, positive towards the top, like egui's wheel delta.
        dy: f32,
    },
}

/// An image from a projector's last scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanImage {
    /// The wall as the projector sees it: the camera photo warped into projector pixels.
    Photo,
    /// The decoded camera-to-projector map, as a colour image.
    Map,
    /// Where the projector's light lands in the camera's view.
    Footprint,
}

impl ScanImage {
    /// Every scan image, in the order the docs list them.
    pub const ALL: [Self; 3] = [Self::Photo, Self::Map, Self::Footprint];

    /// The name in URLs: `photo`, `map` or `footprint`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Photo => "photo",
            Self::Map => "map",
            Self::Footprint => "footprint",
        }
    }
}

/// The app's answer to a [`Pending`] request.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// `200` with a JSON body.
    Json(serde_json::Value),
    /// `200` with an encoded PNG.
    Png(Vec<u8>),
    /// An error status and a message, sent as `{"error": message}`.
    Error(u16, String),
}

impl Reply {
    /// The HTTP status this reply is sent with: `200` for JSON and PNG, an error's own status
    /// when it is `400` to `599`, else `500`.
    pub fn status(&self) -> u16 {
        match self {
            Self::Json(_) | Self::Png(_) => 200,
            Self::Error(status, _) if (400..=599).contains(status) => *status,
            Self::Error(..) => 500,
        }
    }
}

/// A request waiting for the app. Answer it with [`Pending::reply`], in this frame or a later
/// one; dropping it unanswered sends the client a `500`.
#[derive(Debug)]
pub struct Pending {
    /// What the client asked for.
    pub request: LabRequest,
    reply: Sender<Reply>,
}

impl Pending {
    /// A pending request and the receiver its reply arrives on. The server makes these; tests
    /// of request handlers use it to read what a handler answered.
    pub fn new(request: LabRequest) -> (Self, Receiver<Reply>) {
        let (reply, answer) = channel();
        (Self { request, reply }, answer)
    }

    /// Answers the request. Nothing happens when the client already gave up waiting.
    pub fn reply(self, reply: Reply) {
        let _ = self.reply.send(reply);
    }
}
