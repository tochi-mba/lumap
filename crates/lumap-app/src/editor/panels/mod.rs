//! The editor's panels: the shell (top bar, steps, coach line, toasts), the start page, the
//! settings window, the status bar, each step's panels, the inspector, the check before going
//! live, the dialogs and the show-mode screen.

use super::canvas::CanvasState;
use super::thumbs::Thumbnails;
use super::Editor;
use eframe::egui::{self, Color32, RichText};
use lumap_core::params::ParamValue;
use lumap_core::project::SourceKind;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Native file dialogs, behind a trait so tests can script them.
pub trait FileDialogs {
    /// Choose a project to open.
    fn pick_project(&mut self) -> Option<PathBuf>;
    /// Choose where to save a project.
    fn save_project(&mut self, suggested: Option<&Path>) -> Option<PathBuf>;
    /// Choose media files to add as sources.
    fn pick_media(&mut self) -> Vec<PathBuf>;
    /// Choose a folder of image frames.
    fn pick_folder(&mut self) -> Option<PathBuf>;
}

/// The real dialogs.
#[derive(Debug, Default)]
pub struct NativeDialogs;

impl FileDialogs for NativeDialogs {
    fn pick_project(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("lumap project", &["toml"])
            .pick_file()
    }
    fn save_project(&mut self, suggested: Option<&Path>) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new().add_filter("lumap project", &["toml"]);
        if let Some(p) = suggested {
            if let Some(dir) = p.parent() {
                d = d.set_directory(dir);
            }
            if let Some(name) = p.file_name() {
                d = d.set_file_name(name.to_string_lossy());
            }
        } else {
            d = d.set_file_name("show.lumap.toml");
        }
        d.save_file()
    }
    fn pick_media(&mut self) -> Vec<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("media", super::state::MEDIA_EXTENSIONS)
            .pick_files()
            .unwrap_or_default()
    }
    fn pick_folder(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }
}

/// Scripted dialogs for tests.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct FakeDialogs {
    /// Returned by the next `pick_project`.
    pub project: Option<PathBuf>,
    /// Returned by the next `save_project`.
    pub save: Option<PathBuf>,
    /// Returned by the next `pick_media`.
    pub media: Vec<PathBuf>,
    /// Returned by the next `pick_folder`.
    pub folder: Option<PathBuf>,
}

#[cfg(test)]
impl FileDialogs for FakeDialogs {
    fn pick_project(&mut self) -> Option<PathBuf> {
        self.project.take()
    }
    fn save_project(&mut self, _suggested: Option<&Path>) -> Option<PathBuf> {
        self.save.take()
    }
    fn pick_media(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.media)
    }
    fn pick_folder(&mut self) -> Option<PathBuf> {
        self.folder.take()
    }
}

/// Things the panels ask the app to do; the app runs them after the frame.
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Actions {
    /// Save (or Save As when untitled).
    pub save: bool,
    /// Save As.
    pub save_as: bool,
    /// Open through the dialog.
    pub open: bool,
    /// Open this path.
    pub open_path: Option<PathBuf>,
    /// New project.
    pub new: bool,
    /// Add media through the dialog.
    pub add_media: bool,
    /// Add an image-sequence folder through the dialog.
    pub add_folder: bool,
    /// Create outputs for the detected monitors.
    pub create_outputs: bool,
    /// The unsaved-changes prompt was answered: proceed or cancel.
    pub resolve_pending: Option<bool>,
    /// Show (true) or skip (false) the start page when lumap opens.
    pub start_page: Option<bool>,
    /// Leave the start page for the editor.
    pub leave_home: bool,
    /// New ffmpeg path to remember and use.
    pub ffmpeg_path: Option<String>,
    /// MIDI input port to open (empty closes it).
    pub midi_port: Option<String>,
    /// OSC feedback target `host:port` (empty disables).
    pub feedback: Option<String>,
    /// Phone remote port (0 stops it).
    pub remote_port: Option<u16>,
    /// OSC listening port (0 stops it).
    pub osc_port: Option<u16>,
    /// A click on the update banner or the Check now button.
    pub update: Option<crate::update::Request>,
    /// A new update mode from the show inspector.
    pub update_mode: Option<crate::update::flow::Mode>,
    /// A look picked on the start page, to put on every surface.
    pub look: Option<String>,
}

/// Transient UI state for the panels.
#[allow(clippy::struct_excessive_bools)]
pub struct PanelState {
    /// File dialogs (native, or scripted in tests).
    pub dialogs: Box<dyn FileDialogs>,
    /// The start page is up instead of the editor.
    pub home: bool,
    /// The step the editor is on.
    pub step: shell::Step,
    /// The Settings window is open.
    pub settings_open: bool,
    /// Messages in the corner.
    pub toasts: Vec<shell::Toast>,
    /// The editor status last turned into a toast.
    pub last_status: String,
    /// The first status has been seen (it is not shown: it is the one lumap starts with).
    pub status_seen: bool,
    /// The keyboard cheat sheet is up.
    pub help: bool,
    /// Text for a new Spout sender name.
    pub spout_sender: String,
    /// A projector size being typed or dragged: projector id, width, height.
    pub size_edit: Option<(String, u32, u32)>,
    /// Mesh size for "convert to Bezier".
    pub mesh_cols: u32,
    /// Mesh size for "convert to Bezier".
    pub mesh_rows: u32,
    /// Error from the last file operation.
    pub error: Option<String>,
    /// Id being edited in the rename box.
    pub rename: Option<(String, String)>,
    /// Text being typed into a [`draft_field`]: whose it is, and the text.
    pub draft: Option<(String, String)>,
    /// The check before going live is open.
    pub golive: bool,
    /// OSC port field.
    pub osc_port: u16,
    /// Thumbnail textures.
    pub thumbs: Thumbnails,
    /// Whether ffmpeg was found at startup.
    pub ffmpeg_ok: bool,
    /// Where the ffmpeg lumap uses came from.
    pub ffmpeg_origin: lumap_sources::video::Origin,
    /// ffmpeg path field.
    pub ffmpeg_path: String,
    /// Name for the next stored cue.
    pub cue_name: String,
    /// Fade time for the next stored cue.
    pub cue_fade: f64,
    /// MIDI port field.
    pub midi_port: String,
    /// MIDI ports found last time the list was refreshed.
    pub midi_ports: Vec<String>,
    /// Address typed into the MIDI learn box.
    pub midi_address: String,
    /// OSC feedback field.
    pub feedback_target: String,
    /// Phone remote port field.
    pub remote_port: u16,
    /// Update mode shown in the show inspector.
    pub update_mode: crate::update::flow::Mode,
    /// The look chosen in the surface inspector (index into `LOOKS`).
    pub look: usize,
    /// The Content step's library.
    pub library: steps::content::LibraryState,
}

impl Default for PanelState {
    fn default() -> Self {
        let (ffmpeg_tools, ffmpeg_origin) = lumap_sources::video::Tools::discover();
        Self {
            dialogs: Box::new(NativeDialogs),
            home: false,
            step: shell::Step::default(),
            settings_open: false,
            toasts: Vec::new(),
            last_status: String::new(),
            status_seen: false,
            help: false,
            spout_sender: String::new(),
            size_edit: None,
            mesh_cols: 2,
            mesh_rows: 2,
            error: None,
            rename: None,
            draft: None,
            golive: false,
            osc_port: 9000,
            thumbs: Thumbnails::default(),
            ffmpeg_ok: ffmpeg_tools.available(),
            ffmpeg_origin,
            ffmpeg_path: std::env::var("LUMAP_FFMPEG").unwrap_or_default(),
            cue_name: String::new(),
            cue_fade: 2.0,
            midi_port: String::new(),
            midi_ports: Vec::new(),
            midi_address: "/show/master".into(),
            feedback_target: String::new(),
            remote_port: 8080,
            update_mode: crate::update::flow::Mode::default(),
            look: 0,
            library: steps::content::LibraryState::default(),
        }
    }
}

impl std::fmt::Debug for PanelState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanelState")
            .field("home", &self.home)
            .field("step", &self.step)
            .field("help", &self.help)
            .finish_non_exhaustive()
    }
}

fn num(editor: &mut Editor, address: &str) -> f64 {
    editor
        .get_param(address)
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

/// A slider bound to a parameter address, routed through undo.
fn param_slider(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    label: &str,
    address: &str,
    range: std::ops::RangeInclusive<f64>,
) {
    let mut v = num(editor, address);
    let r = ui.add(egui::Slider::new(&mut v, range).text(label));
    if r.changed() {
        editor.set_param(address, ParamValue::Number(v));
    }
    if r.drag_stopped() || r.lost_focus() {
        editor.commit();
    }
}

fn param_toggle(ui: &mut egui::Ui, editor: &mut Editor, label: &str, address: &str) {
    let mut v = editor
        .get_param(address)
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if ui.checkbox(&mut v, label).changed() {
        editor.set_param(address, ParamValue::Bool(v));
        editor.commit();
    }
}

/// Bottom status strip.
pub fn status_bar(
    ui: &mut egui::Ui,
    editor: &Editor,
    canvas: &CanvasState,
    fps: f32,
    now: Instant,
    banner: &crate::update::flow::Banner,
    actions: &mut Actions,
) {
    egui::Panel::bottom("lumap-status").show(ui, |ui| {
        ui.horizontal(|ui| {
            update_banner(ui, banner, actions);
            let selected = editor.selected_surfaces();
            if selected.len() > 1 {
                ui.label(RichText::new(format!("{} surfaces selected", selected.len())).weak());
            }
            ui.separator();
            crate::theme::pill(
                ui,
                &match &editor.osc {
                    Some(s) => format!("OSC {}", s.local_addr().port()),
                    None => "OSC off".into(),
                },
                false,
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{fps:.0} fps")).weak());
                ui.separator();
                ui.label(RichText::new(format!("{:.0}%", canvas.zoom * 100.0)).weak())
                    .on_hover_text("Scroll to zoom, middle-drag to pan, 0 to fit");
                ui.separator();
                crate::theme::pill(
                    ui,
                    if canvas.handles.snap {
                        "snap on"
                    } else {
                        "snap off"
                    },
                    false,
                );
                ui.separator();
                if editor.dirty {
                    let secs = editor.autosave_due_in(now).as_secs();
                    ui.label(
                        RichText::new(if editor.path.is_some() {
                            format!("unsaved · autosave in {secs}s")
                        } else {
                            "unsaved".into()
                        })
                        .color(crate::theme::SIGNAL),
                    );
                } else {
                    ui.label(RichText::new("saved").weak());
                }
            });
        });
    });
}

/// The update offer in the status bar: a line and the choices it allows. Quiet when there is
/// nothing to say.
fn update_banner(ui: &mut egui::Ui, banner: &crate::update::flow::Banner, actions: &mut Actions) {
    use crate::update::flow::Banner;
    use crate::update::Request;
    let (text, choices): (&str, &[(&str, Request)]) = match banner {
        Banner::None => return,
        Banner::Available(t) => (
            t,
            &[
                ("Download", Request::Download),
                ("Later", Request::Later),
                ("Skip", Request::Skip),
            ],
        ),
        Banner::Downloading(t) => (t, &[]),
        Banner::Ready(t) => (
            t,
            &[
                ("Install and restart", Request::Install),
                ("Later", Request::Later),
                ("Skip", Request::Skip),
            ],
        ),
    };
    ui.label(RichText::new(text).color(crate::theme::ACCENT));
    for (label, request) in choices {
        if ui.small_button(*label).clicked() {
            actions.update = Some(*request);
        }
    }
    ui.separator();
}

fn describe(kind: &SourceKind) -> String {
    match kind {
        SourceKind::TestPattern { pattern } => format!("{pattern:?}").to_lowercase(),
        SourceKind::Solid { color } => {
            format!("solid {:.2} {:.2} {:.2}", color[0], color[1], color[2])
        }
        SourceKind::Image { path } => format!("image {path}"),
        SourceKind::Spout { sender } => format!(
            "spout {}",
            if sender.is_empty() {
                "(active)"
            } else {
                sender
            }
        ),
        SourceKind::Video { path, playback } => format!(
            "video {path}{}",
            if playback.paused { " (paused)" } else { "" }
        ),
        SourceKind::Sequence { path, .. } => format!("sequence {path}"),
        SourceKind::Generator { pattern, .. } => format!("generator {pattern:?}").to_lowercase(),
    }
}

/// A one-line field that edits a copy of `current` and hands back the new text once, when the
/// field loses focus (Enter, Tab or a click elsewhere) with something changed. Until something
/// is typed the field shows `current`, so undo and changes made elsewhere show up. `key` says
/// whose copy `draft` holds; one field is typed in at a time, so the panels share one draft.
fn draft_field(
    ui: &mut egui::Ui,
    draft: &mut Option<(String, String)>,
    key: &str,
    current: &str,
    width: f32,
    hint: &str,
) -> (egui::Response, Option<String>) {
    let mine = draft.as_ref().is_some_and(|(k, _)| k == key);
    let mut text = match draft {
        Some((_, t)) if mine => t.clone(),
        _ => current.to_string(),
    };
    let r = ui.add(
        egui::TextEdit::singleline(&mut text)
            .desired_width(width)
            .hint_text(hint),
    );
    if r.changed() {
        *draft = Some((key.to_string(), text.clone()));
    }
    let mut done = None;
    if r.lost_focus() && mine {
        if text != current {
            done = Some(text);
        }
        *draft = None;
    }
    (r, done)
}

/// Inline id editor shared by the inspectors. Returns true when a rename happened.
fn rename_box(ui: &mut egui::Ui, editor: &mut Editor, state: &mut PanelState, id: &str) -> bool {
    let mut renamed = false;
    ui.horizontal(|ui| {
        let label = ui.label("Name");
        let editing = state.rename.as_ref().is_some_and(|(orig, _)| orig == id);
        let mut text = if editing {
            state
                .rename
                .as_ref()
                .map(|(_, t)| t.clone())
                .unwrap_or_default()
        } else {
            id.to_string()
        };
        let r = ui.text_edit_singleline(&mut text).labelled_by(label.id);
        if r.changed() {
            state.rename = Some((id.to_string(), text.clone()));
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (enter || (r.lost_focus() && editing)) && text != id {
            match editor.rename(id, &text) {
                Ok(()) => {
                    state.error = None;
                    renamed = true;
                }
                Err(e) => state.error = Some(e),
            }
            state.rename = None;
        }
    });
    renamed
}

/// The cheat sheet.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("1 / 2 / 3 / 4", "Setup / Map / Content / Show"),
    ("Drag handle", "Move a corner, control point or mask point"),
    ("Shift+drag", "Move the whole surface or mask"),
    (
        "Arrows",
        "Nudge the active handle by 1 px (Shift ×10, Alt ×0.1)",
    ),
    ("Click surface", "Select it (Shift adds to the selection)"),
    ("Scroll / middle-drag", "Zoom and pan the canvas, 0 fits"),
    ("G / S", "Toggle grid / snapping"),
    ("L", "Lock or unlock the selected surface"),
    ("Ctrl+D", "Duplicate the selected surface"),
    ("Delete", "Delete the selection"),
    ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
    ("Ctrl+N / Ctrl+O / Ctrl+S", "New / open / save"),
    ("I", "Identify: flash each projector's name on its wall"),
    ("B", "Blackout"),
    ("T", "Stage view: every placed projector on the desktop"),
    (
        "[ / ]",
        "Blend gamma of the shown projector down / up (gamma match)",
    ),
    ("Space", "GO: fade to the next cue"),
    ("F10", "Go live without the check; Escape twice leaves"),
    ("?", "This list"),
];

/// Overlay while files hover over the window.
pub fn drop_overlay(ctx: &egui::Context) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("drop-overlay"),
    ));
    let rect = ctx.content_rect();
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha(140));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Drop to add as sources",
        egui::FontId::new(28.0, crate::theme::strong_family()),
        crate::theme::TEXT,
    );
}

pub fn show_mode_screen(ui: &mut egui::Ui, editor: &Editor, fps: f32) {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(crate::theme::INK))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.3);
                ui.label(
                    RichText::new("REX TECHNOLOGIES")
                        .size(11.0)
                        .family(crate::theme::strong_family())
                        .color(crate::theme::SIGNAL),
                );
                ui.label(
                    RichText::new("SHOW MODE")
                        .size(32.0)
                        .family(crate::theme::strong_family())
                        .color(crate::theme::TEXT),
                );
                ui.label(
                    RichText::new(&editor.project.name)
                        .size(18.0)
                        .color(crate::theme::MUTED),
                );
                ui.add_space(12.0);
                ui.label(
                    RichText::new(format!(
                        "{} output window(s) · {fps:.0} fps",
                        editor.open_outputs.len()
                    ))
                    .color(crate::theme::MUTED),
                );
                ui.label(
                    RichText::new(match &editor.osc {
                        Some(s) => format!("OSC on {}", s.local_addr()),
                        None => "OSC off".into(),
                    })
                    .color(crate::theme::MUTED),
                );
                if editor.project.blackout {
                    crate::theme::pill(ui, "BLACKOUT", true);
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Locked · press Escape twice to leave")
                        .small()
                        .color(crate::theme::MUTED),
                );
                ui.add_space(12.0);
                ui.label(RichText::new(&editor.status).color(crate::theme::MUTED));
            });
        });
}

mod dialogs;
pub mod golive;
mod inspectors;
pub mod looks;
mod settings;
pub mod shell;
mod start;
pub mod steps;

pub use dialogs::dialogs;
pub use inspectors::inspector;
pub use settings::settings_window;
pub use start::start_page;
