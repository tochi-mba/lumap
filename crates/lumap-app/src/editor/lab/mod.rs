//! The lab, app side: `lumap --lab` serves a local automation API (see `lumap_control::lab` and
//! docs/lab.md) and this module answers it. Each frame it takes the requests that arrived and
//! either answers at once (state, actions, scan images) or keeps them waiting for something to
//! happen: a settled camera photo, a screenshot, a fresh widget tree, input to finish, a scan.

mod act;
mod images;
pub mod plugin;

use super::LumapApp;
use eframe::egui;
use lumap_control::lab::{LabRequest, LabServer, Pending, Reply};
use lumap_sources::Frame;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long a screenshot may take to arrive.
const SHOT_TIMEOUT: Duration = Duration::from_secs(5);

/// Something a request is waiting for.
enum Wait {
    Camera {
        pending: Pending,
        until: Instant,
        need: usize,
        got: Vec<Arc<Frame>>,
        seen: u64,
    },
    Shot {
        pending: Pending,
        rx: Receiver<Arc<egui::ColorImage>>,
        deadline: Instant,
    },
    Tree {
        pending: Pending,
        after: u64,
    },
    Input {
        pending: Pending,
    },
    Scan {
        pending: Pending,
    },
    Act {
        pending: Pending,
        rest: Vec<lumap_control::lab::Action>,
        resume: Instant,
        done: Vec<serde_json::Value>,
    },
    Record {
        pending: Pending,
        until: Instant,
        dir: PathBuf,
        seen: u64,
        count: u32,
    },
}

/// The running lab.
pub struct LabRuntime {
    server: LabServer,
    plugin: Arc<Mutex<plugin::Shared>>,
    waits: Vec<Wait>,
    dir: PathBuf,
}

/// The lab's folder: `%LOCALAPPDATA%\lumap\lab` (or the platform's data folder).
pub fn lab_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("lumap")
        .join("lab")
}

impl LabRuntime {
    /// Starts the server on `port` (0 = any free port), registers the input plugin and writes
    /// the session file a client reads.
    pub fn start(ctx: &egui::Context, port: u16) -> Result<Self, String> {
        let server = LabServer::bind(port)?;
        let dir = lab_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let session = server.write_session(&dir).map_err(|e| e.to_string())?;
        log::info!(
            "lab: http://{} token {} (session {})",
            server.addr(),
            server.token(),
            session.display()
        );
        let plugin = Arc::new(Mutex::new(plugin::Shared::default()));
        ctx.add_plugin(plugin::LabPlugin::new(Arc::clone(&plugin)));
        ctx.enable_accesskit();
        Ok(Self {
            server,
            plugin,
            waits: Vec::new(),
            dir,
        })
    }

    /// Where the lab listens.
    pub fn url(&self) -> String {
        format!("http://{}", self.server.addr())
    }

    /// Answers new requests and finishes waiting ones; call once per frame.
    pub fn tick(&mut self, app: &mut LumapApp, ctx: &egui::Context, now: Instant) {
        for pending in self.server.drain() {
            self.handle(app, ctx, pending, now);
        }
        let waits = std::mem::take(&mut self.waits);
        for w in waits {
            if let Some(w) = self.advance(app, ctx, w, now) {
                self.waits.push(w);
            }
        }
        if !self.waits.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn handle(&mut self, app: &mut LumapApp, ctx: &egui::Context, pending: Pending, now: Instant) {
        match pending.request.clone() {
            LabRequest::State => pending.reply(Reply::Json(images::state(app))),
            LabRequest::Act(actions) => self.waits.push(Wait::Act {
                pending,
                rest: actions,
                resume: now,
                done: Vec::new(),
            }),
            LabRequest::Camera { settle_ms, frames } => {
                let Some(device) = app.camera_device() else {
                    pending.reply(Reply::Error(409, "There is no camera.".into()));
                    return;
                };
                let seen = app
                    .camera
                    .ensure(&device, now)
                    .map_or(0, |s| s.frames_seen());
                self.waits.push(Wait::Camera {
                    pending,
                    until: now + Duration::from_millis(settle_ms),
                    need: frames.max(1) as usize,
                    got: Vec::new(),
                    seen,
                });
            }
            LabRequest::CameraInfo => pending.reply(Reply::Json(images::camera_info(app))),
            LabRequest::Projector { id } => {
                if !app.editor.open_outputs.contains(&id) {
                    pending.reply(Reply::Error(
                        409,
                        format!("Projector {id}'s window is not open."),
                    ));
                    return;
                }
                let viewport = egui::ViewportId::from_hash_of(("lumap-output", &id));
                let (tx, rx) = std::sync::mpsc::channel();
                ctx.send_viewport_cmd_to(
                    viewport,
                    egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                        let _ = tx.send(img);
                    })),
                );
                self.waits.push(Wait::Shot {
                    pending,
                    rx,
                    deadline: now + SHOT_TIMEOUT,
                });
            }
            LabRequest::Ui => {
                let (tx, rx) = std::sync::mpsc::channel();
                ctx.request_screenshot(move |img| {
                    let _ = tx.send(img);
                });
                self.waits.push(Wait::Shot {
                    pending,
                    rx,
                    deadline: now + SHOT_TIMEOUT,
                });
            }
            LabRequest::UiTree => {
                let after = self.plugin_state(|s| s.tree_frames);
                self.waits.push(Wait::Tree { pending, after });
            }
            LabRequest::UiInput(event) => match self.queue_input(&event) {
                Ok(()) => self.waits.push(Wait::Input { pending }),
                Err(e) => pending.reply(Reply::Error(404, e)),
            },
            LabRequest::Record { seconds } => {
                let Some(device) = app.camera_device() else {
                    pending.reply(Reply::Error(409, "There is no camera.".into()));
                    return;
                };
                let stamp = images::stamp();
                let dir = self.dir.join("recordings").join(stamp);
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    pending.reply(Reply::Error(500, e.to_string()));
                    return;
                }
                let seen = app
                    .camera
                    .ensure(&device, now)
                    .map_or(0, |s| s.frames_seen());
                self.waits.push(Wait::Record {
                    pending,
                    until: now + Duration::from_secs_f64(seconds.clamp(0.1, 120.0)),
                    dir,
                    seen,
                    count: 0,
                });
            }
            LabRequest::Scan { projector } => {
                let kind = super::scan::Kind::Full;
                match app.start_scan(kind, projector.as_deref(), now) {
                    Ok(()) => self.waits.push(Wait::Scan { pending }),
                    Err(e) => pending.reply(Reply::Error(409, e)),
                }
            }
            LabRequest::Realign => match app.start_scan(super::scan::Kind::Realign, None, now) {
                Ok(()) => self.waits.push(Wait::Scan { pending }),
                Err(e) => pending.reply(Reply::Error(409, e)),
            },
            LabRequest::ScanImage { projector, which } => {
                pending.reply(images::scan_image(app, &projector, which));
            }
            LabRequest::Found { projector } => pending.reply(images::found(app, &projector)),
            LabRequest::Vision { name } => pending.reply(images::vision(app, &name, now)),
            LabRequest::Logs { lines, level, grep } => {
                let lines =
                    crate::logging::recent(lines as usize, level.to_log(), grep.as_deref());
                pending.reply(Reply::Json(serde_json::json!({
                    "file": crate::logging::current_file().map(|p| p.display().to_string()),
                    "lines": lines,
                })));
            }
        }
    }

    fn plugin_state<R>(&self, f: impl FnOnce(&mut plugin::Shared) -> R) -> R {
        f(&mut self.plugin.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Turns a UI event into frames of raw input for the plugin.
    fn queue_input(&self, event: &lumap_control::lab::UiEvent) -> Result<(), String> {
        use lumap_control::lab::UiEvent;
        let frames = self.plugin_state(|s| -> Result<Vec<Vec<egui::Event>>, String> {
            let at = |label: &str, nth: Option<usize>| {
                plugin::find(&s.tree, label, nth.unwrap_or(0))
                    .map(plugin::Widget::centre)
                    .ok_or_else(|| format!("No widget labelled {label:?} on screen."))
            };
            Ok(match event {
                UiEvent::Click { label, nth } => plugin::click(at(label, *nth)?),
                UiEvent::ClickAt { x, y } => plugin::click(egui::pos2(*x, *y)),
                UiEvent::Type { label, text } => {
                    let mut frames = match label {
                        Some(l) => plugin::click(at(l, None)?),
                        None => Vec::new(),
                    };
                    frames.push(vec![egui::Event::Text(text.clone())]);
                    frames
                }
                UiEvent::Key { key, modifiers } => {
                    let k = egui::Key::from_name(key)
                        .ok_or_else(|| format!("No key called {key:?}."))?;
                    plugin::key(k, plugin::modifiers(modifiers))
                }
                UiEvent::Drag { from, to } => {
                    plugin::drag(egui::pos2(from[0], from[1]), egui::pos2(to[0], to[1]))
                }
                UiEvent::Scroll { x, y, dy } => plugin::scroll(egui::pos2(*x, *y), *dy),
            })
        })?;
        self.plugin_state(|s| s.queue.extend(frames));
        Ok(())
    }

    /// Moves a waiting request on; gives it back while it still waits.
    #[allow(clippy::too_many_lines)]
    fn advance(
        &mut self,
        app: &mut LumapApp,
        ctx: &egui::Context,
        wait: Wait,
        now: Instant,
    ) -> Option<Wait> {
        match wait {
            Wait::Act {
                pending,
                mut rest,
                resume,
                mut done,
            } => {
                if now < resume {
                    return Some(Wait::Act {
                        pending,
                        rest,
                        resume,
                        done,
                    });
                }
                while !rest.is_empty() {
                    let action = rest.remove(0);
                    if let lumap_control::lab::Action::Wait { ms } = action {
                        done.push(serde_json::json!({ "do": "wait", "ok": true }));
                        return Some(Wait::Act {
                            pending,
                            rest,
                            resume: now + Duration::from_millis(ms),
                            done,
                        });
                    }
                    done.push(act::run(app, ctx, &action, now));
                }
                pending.reply(Reply::Json(serde_json::json!({ "results": done })));
                None
            }
            Wait::Camera {
                pending,
                until,
                need,
                mut got,
                mut seen,
            } => {
                let Some(device) = app.camera_device() else {
                    pending.reply(Reply::Error(409, "There is no camera.".into()));
                    return None;
                };
                let Some(stream) = app.camera.ensure(&device, now) else {
                    pending.reply(Reply::Error(503, "The camera is not available.".into()));
                    return None;
                };
                if now >= until && stream.frames_seen() > seen {
                    seen = stream.frames_seen();
                    if let Some(f) = stream.latest() {
                        got.push(f);
                    }
                }
                if got.len() < need {
                    return Some(Wait::Camera {
                        pending,
                        until,
                        need,
                        got,
                        seen,
                    });
                }
                let reply = super::camera::median(&got).map_or_else(
                    || Reply::Error(500, "No camera frame.".into()),
                    |img| images::png_reply(img.width, img.height, img.data),
                );
                pending.reply(reply);
                None
            }
            Wait::Shot {
                pending,
                rx,
                deadline,
            } => match rx.try_recv() {
                Ok(img) => {
                    pending.reply(images::color_image_png(&img));
                    None
                }
                Err(_) if now > deadline => {
                    pending.reply(Reply::Error(504, "The screenshot never came.".into()));
                    None
                }
                Err(_) => Some(Wait::Shot {
                    pending,
                    rx,
                    deadline,
                }),
            },
            Wait::Tree { pending, after } => {
                let (frames, tree) = self.plugin_state(|s| (s.tree_frames, s.tree.clone()));
                if frames <= after + 1 {
                    ctx.request_repaint();
                    return Some(Wait::Tree { pending, after });
                }
                pending.reply(Reply::Json(serde_json::json!({ "widgets": tree })));
                None
            }
            Wait::Input { pending } => {
                if self.plugin_state(|s| s.queue.is_empty()) {
                    pending.reply(Reply::Json(serde_json::json!({ "ok": true })));
                    None
                } else {
                    ctx.request_repaint();
                    Some(Wait::Input { pending })
                }
            }
            Wait::Scan { pending } => match app.scan_outcome() {
                None => Some(Wait::Scan { pending }),
                Some(Ok(json)) => {
                    pending.reply(Reply::Json(json));
                    None
                }
                Some(Err(e)) => {
                    pending.reply(Reply::Error(500, e));
                    None
                }
            },
            Wait::Record {
                pending,
                until,
                dir,
                mut seen,
                mut count,
            } => {
                let stream = app.camera_device().and_then(|d| app.camera.ensure(&d, now));
                if let Some(s) = stream {
                    if s.frames_seen() > seen {
                        seen = s.frames_seen();
                        if let Some(f) = s.latest() {
                            count += 1;
                            let path = dir.join(format!("frame-{count:05}.png"));
                            std::thread::spawn(move || {
                                if let Some(img) =
                                    image::RgbaImage::from_raw(f.width, f.height, f.rgba.clone())
                                {
                                    let _ = img.save(path);
                                }
                            });
                        }
                    }
                }
                if now < until {
                    return Some(Wait::Record {
                        pending,
                        until,
                        dir,
                        seen,
                        count,
                    });
                }
                pending.reply(Reply::Json(serde_json::json!({
                    "dir": dir.display().to_string(),
                    "frames": count,
                })));
                None
            }
        }
    }
}
