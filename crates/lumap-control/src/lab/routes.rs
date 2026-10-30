//! Routing: method, path, query and body to a [`LabRequest`], with every check that needs no
//! app state. Pure, so the whole table is unit-tested without a server.

use super::{Action, LabRequest, LogLevel, Pattern, ScanImage, UiEvent};
use lumap_core::params::ParamAddress;
use lumap_core::project::is_valid_id;
use serde::Deserialize;

/// A refused request: the HTTP status and a message for the client.
pub(crate) type Refusal = (u16, String);

/// Settle time of `GET /lab/camera.png` without `settle_ms`.
const DEFAULT_SETTLE_MS: u64 = 300;
/// Frames of `GET /lab/camera.png` without `frames`.
const DEFAULT_FRAMES: u64 = 3;
/// Longest settle time a camera photo may ask for.
const MAX_SETTLE_MS: u64 = 10_000;
/// Most frames a camera photo may take the median of.
const MAX_FRAMES: u64 = 60;
/// What the `wait` actions of one request may add up to, well inside the 30 s reply timeout.
const MAX_WAIT_MS: u64 = 20_000;
/// Longest recording.
const MAX_RECORD_SECONDS: f64 = 600.0;

/// Turns a request into what the app should do, or the status and message to refuse it with.
///
/// Unknown paths are `404` and a known path with the wrong method `405`. A bad query, id, value
/// or JSON body is `400` (bad JSON carries serde's message, with line and column), and so is any
/// path containing `..`. `GET /lab/ping` is not routed here: the server answers it itself, so it
/// works while the app is busy.
pub fn route(method: &str, path_and_query: &str, body: &[u8]) -> Result<LabRequest, Refusal> {
    let (path, query) = path_and_query
        .split_once('?')
        .unwrap_or((path_and_query, ""));
    if path.contains("..") {
        return Err((400, format!("paths may not contain `..`: {path}")));
    }
    let endpoint = Endpoint::parse(path)?;
    let want = endpoint.method();
    if method != want {
        return Err((405, format!("{path} answers {want}, not {method}")));
    }
    let query = pairs(query)?;
    let takes_query = matches!(endpoint, Endpoint::Camera | Endpoint::Logs);
    if let (Some((key, _)), false) = (query.first(), takes_query) {
        return Err((400, format!("{path} takes no query parameter `{key}`")));
    }
    endpoint.request(&query, body)
}

/// What the server does with an authorised request.
#[derive(Debug, PartialEq)]
pub(crate) enum Dispatch {
    /// `GET /lab/ping`: the server answers it without the app.
    Ping,
    /// Everything else goes to the app.
    App(LabRequest),
}

/// [`route`], plus `GET /lab/ping`.
pub(crate) fn dispatch(
    method: &str,
    path_and_query: &str,
    body: &[u8],
) -> Result<Dispatch, Refusal> {
    match (method, path_and_query.split('?').next().unwrap_or_default()) {
        ("GET", "/lab/ping") => Ok(Dispatch::Ping),
        (_, "/lab/ping") => Err((405, format!("/lab/ping answers GET, not {method}"))),
        _ => route(method, path_and_query, body).map(Dispatch::App),
    }
}

/// A known path, before its method, query and body are checked.
enum Endpoint {
    State,
    Act,
    Camera,
    CameraInfo,
    Projector(String),
    Ui,
    UiTree,
    UiInput,
    Record,
    Scan,
    ScanImage(String, ScanImage),
    Found(String),
    Realign,
    Vision(String),
    Logs,
}

impl Endpoint {
    fn parse(path: &str) -> Result<Self, Refusal> {
        let segments: Vec<&str> = path.split('/').collect();
        Ok(match segments.as_slice() {
            ["", "lab", "state"] => Self::State,
            ["", "lab", "act"] => Self::Act,
            ["", "lab", "camera.png"] => Self::Camera,
            ["", "lab", "camera.json"] => Self::CameraInfo,
            ["", "lab", "projector", file] => Self::Projector(id(png(file, path)?, "projector")?),
            ["", "lab", "ui.png"] => Self::Ui,
            ["", "lab", "ui", "tree"] => Self::UiTree,
            ["", "lab", "ui"] => Self::UiInput,
            ["", "lab", "record"] => Self::Record,
            ["", "lab", "scan"] => Self::Scan,
            ["", "lab", "scan", projector, file] => {
                let name = png(file, path)?;
                let which = ScanImage::ALL
                    .into_iter()
                    .find(|which| which.name() == name)
                    .ok_or_else(|| not_found(path))?;
                Self::ScanImage(id(projector, "projector")?, which)
            }
            ["", "lab", "found", projector] => Self::Found(id(projector, "projector")?),
            ["", "lab", "realign"] => Self::Realign,
            ["", "lab", "vision", file] => Self::Vision(id(png(file, path)?, "vision image")?),
            ["", "lab", "logs"] => Self::Logs,
            _ => return Err(not_found(path)),
        })
    }

    fn method(&self) -> &'static str {
        match self {
            Self::Act | Self::UiInput | Self::Record | Self::Scan | Self::Realign => "POST",
            _ => "GET",
        }
    }

    fn request(self, query: &[(&str, &str)], body: &[u8]) -> Result<LabRequest, Refusal> {
        Ok(match self {
            Self::State => LabRequest::State,
            Self::Act => LabRequest::Act(actions(body)?),
            Self::Camera => camera(query)?,
            Self::CameraInfo => LabRequest::CameraInfo,
            Self::Projector(id) => LabRequest::Projector { id },
            Self::Ui => LabRequest::Ui,
            Self::UiTree => LabRequest::UiTree,
            Self::UiInput => LabRequest::UiInput(
                serde_json::from_slice::<UiEvent>(body).map_err(|e| bad_json(&e))?,
            ),
            Self::Record => record(body)?,
            Self::Scan => scan(body)?,
            Self::ScanImage(projector, which) => LabRequest::ScanImage { projector, which },
            Self::Found(projector) => LabRequest::Found { projector },
            Self::Realign => LabRequest::Realign,
            Self::Vision(name) => LabRequest::Vision { name },
            Self::Logs => logs(query)?,
        })
    }
}

fn not_found(path: &str) -> Refusal {
    (404, format!("no lab endpoint {path}"))
}

fn bad_json(error: &serde_json::Error) -> Refusal {
    (400, format!("bad JSON: {error}"))
}

/// The name of a `<name>.png` path segment.
fn png<'a>(file: &'a str, path: &str) -> Result<&'a str, Refusal> {
    file.strip_suffix(".png").ok_or_else(|| not_found(path))
}

/// An id that is safe in parameter addresses and file names.
fn id(text: &str, what: &str) -> Result<String, Refusal> {
    if is_valid_id(text) {
        Ok(text.to_owned())
    } else {
        Err((
            400,
            format!("bad {what} id {text:?}: use letters, digits, `-` and `_`"),
        ))
    }
}

/// `a=1&b=2` as pairs.
fn pairs(query: &str) -> Result<Vec<(&str, &str)>, Refusal> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            pair.split_once('=')
                .ok_or_else(|| (400, format!("query parameter `{pair}` has no value")))
        })
        .collect()
}

fn camera(query: &[(&str, &str)]) -> Result<LabRequest, Refusal> {
    let (mut settle_ms, mut frames) = (DEFAULT_SETTLE_MS, DEFAULT_FRAMES);
    for &(key, value) in query {
        match key {
            "settle_ms" => settle_ms = number(key, value, 0, MAX_SETTLE_MS)?,
            "frames" => frames = number(key, value, 1, MAX_FRAMES)?,
            _ => {
                return Err((
                    400,
                    format!(
                        "unknown query parameter `{key}`; camera.png takes settle_ms and frames"
                    ),
                ))
            }
        }
    }
    Ok(LabRequest::Camera {
        settle_ms,
        frames: frames as u32,
    })
}

/// Most log lines one request returns.
pub const MAX_LOG_LINES: u64 = 5000;
/// Log lines returned when the request does not say.
pub const DEFAULT_LOG_LINES: u64 = 300;

fn logs(query: &[(&str, &str)]) -> Result<LabRequest, Refusal> {
    let (mut lines, mut level, mut grep) = (DEFAULT_LOG_LINES, LogLevel::Debug, None);
    for &(key, value) in query {
        match key {
            "lines" => lines = number(key, value, 1, MAX_LOG_LINES)?,
            "level" => {
                level = match value.to_ascii_lowercase().as_str() {
                    "error" => LogLevel::Error,
                    "warn" => LogLevel::Warn,
                    "info" => LogLevel::Info,
                    "debug" => LogLevel::Debug,
                    "trace" => LogLevel::Trace,
                    _ => {
                        return Err((
                            400,
                            format!(
                                "level must be error, warn, info, debug or trace, not {value:?}"
                            ),
                        ))
                    }
                }
            }
            "grep" => grep = Some(unescape(value)),
            _ => {
                return Err((
                    400,
                    format!("unknown query parameter `{key}`; logs takes lines, level and grep"),
                ))
            }
        }
    }
    Ok(LabRequest::Logs {
        lines: lines as u32,
        level,
        grep,
    })
}

/// A query value with `+` and `%XX` decoded.
fn unescape(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A whole number from `min` to `max`.
fn number(key: &str, value: &str, min: u64, max: u64) -> Result<u64, Refusal> {
    value
        .parse::<u64>()
        .ok()
        .filter(|n| (min..=max).contains(n))
        .ok_or_else(|| {
            (
                400,
                format!("{key} must be a whole number from {min} to {max}, not {value:?}"),
            )
        })
}

/// A JSON array of actions, or `{"actions": [...]}`.
fn actions(body: &[u8]) -> Result<Vec<Action>, Refusal> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wrapped {
        actions: Vec<Action>,
    }

    let wrapped = body.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{');
    let actions = if wrapped {
        serde_json::from_slice::<Wrapped>(body)
            .map_err(|e| bad_json(&e))?
            .actions
    } else {
        serde_json::from_slice::<Vec<Action>>(body).map_err(|e| bad_json(&e))?
    };
    check(&actions)?;
    Ok(actions)
}

/// Checks the values serde cannot: ranges, parameter addresses and the total wait.
fn check(actions: &[Action]) -> Result<(), Refusal> {
    if actions.is_empty() {
        return Err((400, "no actions to run".into()));
    }
    let mut waited: u64 = 0;
    for (i, action) in actions.iter().enumerate() {
        let problem = match action {
            Action::Step { step } if !(1..=4).contains(step) => {
                Some(format!("step must be 1 to 4, not {step}"))
            }
            Action::Param { address, .. } => {
                ParamAddress::parse(address).err().map(|e| e.to_string())
            }
            Action::StoreCue { fade, .. } if *fade < 0.0 => {
                Some(format!("fade must not be negative, not {fade}"))
            }
            Action::ShowPattern {
                pattern: Pattern::Checker { size: 0 },
                ..
            } => Some("checker squares must be at least 1 pixel".into()),
            Action::Wait { ms } => {
                waited = waited.saturating_add(*ms);
                None
            }
            _ => None,
        };
        if let Some(problem) = problem {
            return Err((400, format!("actions[{i}]: {problem}")));
        }
    }
    if waited > MAX_WAIT_MS {
        return Err((
            400,
            format!("the waits add up to {waited} ms; keep one request to {MAX_WAIT_MS} ms and send the rest in another"),
        ));
    }
    Ok(())
}

fn record(body: &[u8]) -> Result<LabRequest, Refusal> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        seconds: f64,
    }

    let Body { seconds } = serde_json::from_slice(body).map_err(|e| bad_json(&e))?;
    if seconds <= 0.0 || seconds > MAX_RECORD_SECONDS {
        return Err((
            400,
            format!("seconds must be more than 0 and at most {MAX_RECORD_SECONDS}, not {seconds}"),
        ));
    }
    Ok(LabRequest::Record { seconds })
}

/// An empty body scans every projector; `{"projector": "<id>"}` scans one.
fn scan(body: &[u8]) -> Result<LabRequest, Refusal> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        #[serde(default)]
        projector: Option<String>,
    }

    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(LabRequest::Scan { projector: None });
    }
    let Body { projector } = serde_json::from_slice(body).map_err(|e| bad_json(&e))?;
    Ok(LabRequest::Scan {
        projector: projector.map(|p| id(&p, "projector")).transpose()?,
    })
}
