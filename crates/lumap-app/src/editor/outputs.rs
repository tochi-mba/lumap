//! Output windows: one borderless, always-on-top viewport per open output.

use super::handles::{self, HandleState, Mapping};
use super::{monitor_at, physical_to_points, scene_for, Editor, MonitorInfo};
use eframe::egui::{self, Sense};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor};
use lumap_render::gpu::GpuRenderer;
use lumap_render::Scene;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Paints a scene into the current egui render pass through the shared [`GpuRenderer`].
pub struct ScenePaint {
    /// Composite key (`output:<id>` or `preview:<id>`).
    pub key: String,
    /// What to draw.
    pub scene: Arc<Scene>,
    /// Target size in physical pixels.
    pub size: [u32; 2],
    /// Surface format of the window being drawn into.
    pub format: wgpu::TextureFormat,
}

impl CallbackTrait for ScenePaint {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = resources.get_mut::<GpuRenderer>() {
            renderer.prepare(
                &self.key,
                &self.scene,
                self.size[0],
                self.size[1],
                self.format,
                encoder,
            );
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        if let Some(renderer) = resources.get::<GpuRenderer>() {
            renderer.paint(&self.key, self.format, pass);
        }
    }
}

/// Adds a scene paint callback covering `rect`.
pub fn paint_scene(
    ui: &egui::Ui,
    rect: egui::Rect,
    key: String,
    scene: Arc<Scene>,
    format: wgpu::TextureFormat,
) {
    let ppp = ui.ctx().pixels_per_point();
    let size = [
        (rect.width() * ppp).round().max(1.0) as u32,
        (rect.height() * ppp).round().max(1.0) as u32,
    ];
    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
        rect,
        ScenePaint {
            key,
            scene,
            size,
            format,
        },
    ));
}

/// Where and how big an output window should be, in egui points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowSpec {
    /// Outer position.
    pub pos: Option<[f32; 2]>,
    /// Inner size.
    pub size: [f32; 2],
    /// Borderless, always-on-top projector window versus a movable preview.
    pub locked: bool,
    /// Borderless fullscreen on the display the window opens on: placed on a connected display,
    /// the window covers it exactly, whatever the displays' scaling (a 125% laptop screen next
    /// to a 100% projector put sized-by-hand windows short of the projector's edges).
    pub fullscreen: bool,
}

/// Decides the window for an output: placed outputs open borderless at full size on their
/// monitor; unplaced ones open as a small decorated preview the user can drag to a projector.
pub fn window_spec(output: &lumap_core::project::Output, monitors: &[MonitorInfo]) -> WindowSpec {
    if let Some(m) = output
        .placement
        .as_ref()
        .filter(|p| !p.monitor.is_empty())
        .and_then(|p| monitors.iter().find(|m| m.name == p.monitor))
    {
        // Open well inside the display, so fullscreen takes the right one.
        let inset = |origin: i32, extent: u32| origin + (extent / 4) as i32;
        return WindowSpec {
            pos: Some([
                physical_to_points(inset(m.x, m.width), m.scale),
                physical_to_points(inset(m.y, m.height), m.scale),
            ]),
            size: [
                physical_to_points(m.width as i32, m.scale),
                physical_to_points(m.height as i32, m.scale),
            ],
            locked: true,
            fullscreen: true,
        };
    }
    if let Some(p) = &output.placement {
        let scale = monitor_at(monitors, p.x, p.y).map_or(1.0, |m| m.scale);
        WindowSpec {
            pos: Some([
                physical_to_points(p.x, scale),
                physical_to_points(p.y, scale),
            ]),
            size: [
                physical_to_points(output.width as i32, scale),
                physical_to_points(output.height as i32, scale),
            ],
            locked: true,
            fullscreen: false,
        }
    } else {
        let aspect = output.width.max(1) as f32 / output.height.max(1) as f32;
        WindowSpec {
            pos: None,
            size: [480.0, (480.0 / aspect).max(120.0)],
            locked: false,
            fullscreen: false,
        }
    }
}

/// Idle time after which the pointer disappears over an output window.
pub const CURSOR_HIDE_AFTER: Duration = Duration::from_secs(2);

/// Per-window interaction state.
pub struct OutputUiState {
    /// Handle interaction on the wall.
    pub handles: HandleState,
    /// Last pointer movement seen on this window.
    pub last_move: Instant,
    /// Last pointer position seen.
    pub last_pos: Option<egui::Pos2>,
    /// The scan pattern on screen, as a texture, keyed by the frame it came from.
    pub pattern: Option<(usize, egui::TextureHandle)>,
    /// Frames this window has been shown.
    pub frames: u32,
    /// Fullscreen has been asked for.
    pub fullscreen: bool,
}

impl Default for OutputUiState {
    fn default() -> Self {
        Self {
            handles: HandleState::default(),
            last_move: Instant::now(),
            last_pos: None,
            pattern: None,
            frames: 0,
            fullscreen: false,
        }
    }
}

/// True when the pointer has not moved for [`CURSOR_HIDE_AFTER`].
pub fn cursor_idle(state: &mut OutputUiState, pos: Option<egui::Pos2>, now: Instant) -> bool {
    if pos != state.last_pos {
        state.last_pos = pos;
        state.last_move = now;
    }
    now.duration_since(state.last_move) >= CURSOR_HIDE_AFTER
}

/// Draws a scan pattern pixel for pixel over the whole window.
fn paint_pattern(
    ui: &egui::Ui,
    rect: egui::Rect,
    id: &str,
    frame: &Arc<lumap_sources::Frame>,
    cache: &mut Option<(usize, egui::TextureHandle)>,
) {
    let key = Arc::as_ptr(frame) as usize;
    if cache.as_ref().is_none_or(|(k, _)| *k != key) {
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [frame.width as usize, frame.height as usize],
            &frame.rgba,
        );
        let tex = ui.ctx().load_texture(
            format!("scan-pattern:{id}"),
            image,
            egui::TextureOptions::NEAREST,
        );
        *cache = Some((key, tex));
    }
    if let Some((_, tex)) = cache {
        ui.painter().image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}

/// A ring where a tracked hand points, in normalised projector coordinates, with a filled
/// centre while it pinches.
fn draw_hand_cursor(
    painter: &egui::Painter,
    rect: egui::Rect,
    cursor: (lumap_core::geom::Vec2, bool),
) {
    let (p, pinching) = cursor;
    let at = egui::pos2(
        rect.min.x + p.x as f32 * rect.width(),
        rect.min.y + p.y as f32 * rect.height(),
    );
    let r = rect.height() * 0.018;
    painter.circle_stroke(at, r, egui::Stroke::new(3.0, crate::theme::SIGNAL));
    if pinching {
        painter.circle_filled(at, r * 0.45, crate::theme::SIGNAL);
    }
}

/// Shows every open output window.
///
/// Outside show mode the windows carry the same handles as the canvas when
/// `editor.edit_on_wall` is set, so a surface can be calibrated while looking at the wall.
#[allow(clippy::too_many_lines)]
pub fn show_output_windows(
    ctx: &egui::Context,
    editor: &mut Editor,
    monitors: &[MonitorInfo],
    format: wgpu::TextureFormat,
    ui_state: &mut HashMap<String, OutputUiState>,
    snap: bool,
    grid: u32,
) {
    let ids = editor.open_outputs.clone();
    ui_state.retain(|k, _| ids.contains(k));
    let now = Instant::now();
    let identify = editor.identify_active(now);
    for id in ids {
        let Some(output) = editor.project.output(&id).cloned() else {
            editor.open_outputs.retain(|o| *o != id);
            continue;
        };
        let spec = window_spec(&output, monitors);
        let mut builder = egui::ViewportBuilder::default()
            .with_title(format!("lumap projector: {id}"))
            .with_inner_size(spec.size)
            .with_decorations(!spec.locked)
            .with_resizable(!spec.locked)
            .with_taskbar(!spec.locked);
        if let Some(pos) = spec.pos {
            builder = builder.with_position(pos);
        }
        if spec.locked {
            builder = builder.with_window_level(egui::WindowLevel::AlwaysOnTop);
        }
        // Fullscreen comes after the window has drawn a few frames: asked for while the window
        // is created, DirectX 12 refuses the swapchain and the app stops.
        let go_fullscreen = {
            let state = ui_state.entry(id.clone()).or_default();
            state.frames = state.frames.saturating_add(1);
            let due = spec.fullscreen && !state.fullscreen && state.frames > 2;
            if due {
                state.fullscreen = true;
            }
            due
        };
        // While a scan runs, its projector shows the pattern and every other one stays black.
        let scan = editor
            .scan_pattern
            .as_ref()
            .map(|(pid, f)| (*pid == id, Arc::clone(f)));
        let scene = if scan.is_some() {
            None
        } else {
            scene_for(editor, &id).map(Arc::new)
        };
        let hand = editor.hand_cursor.get(&id).copied();
        let viewport_id = egui::ViewportId::from_hash_of(("lumap-output", &id));
        let mut close = false;
        let mut outer = None;
        let index = editor
            .project
            .outputs
            .iter()
            .position(|o| o.id == id)
            .unwrap_or(0);
        let state = ui_state.entry(id.clone()).or_default();
        state.handles.snap = snap;
        state.handles.grid = 0;
        let editing = !editor.show_mode && editor.edit_on_wall && scan.is_none();
        ctx.show_viewport_immediate(viewport_id, builder, |ui, class| {
            let rect = ui.max_rect();
            // The whole window, not just the content area, so no light-grey strip shows on the
            // wall at an edge.
            ui.ctx()
                .layer_painter(egui::LayerId::background())
                .rect_filled(
                    ui.ctx().viewport_rect().expand(4.0),
                    0.0,
                    egui::Color32::BLACK,
                );
            ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
            if let Some(scene) = &scene {
                paint_scene(ui, rect, format!("output:{id}"), Arc::clone(scene), format);
            }
            if let Some((true, frame)) = &scan {
                paint_pattern(ui, rect, &id, frame, &mut state.pattern);
                // Tell the scan this pattern is on the wall, once the window fills its display
                // (a window still opening or going full screen shows it somewhere else).
                let filled =
                    !spec.fullscreen || ui.ctx().input(|i| i.viewport().fullscreen) == Some(true);
                if filled {
                    let generation = editor.scan_generation;
                    match &mut editor.scan_shown {
                        Some(s) if s.projector == id && s.generation == generation => {
                            s.frames = s.frames.saturating_add(1);
                        }
                        shown => {
                            log::trace!("projector {id} drew scan pattern {generation}");
                            *shown = Some(crate::editor::state::ScanShown {
                                projector: id.clone(),
                                generation,
                                frames: 1,
                            });
                        }
                    }
                    // Keep drawing so the count moves even when nothing else changes.
                    ui.ctx().request_repaint();
                }
            }
            if let Some(p) = hand {
                draw_hand_cursor(ui.painter(), rect, p);
            }
            if editing {
                let response = ui.allocate_rect(rect, Sense::click_and_drag());
                let map = Mapping { rect };
                state.handles.grid = grid;
                handles::interact(ui, editor, &mut state.handles, &response, &map, &id);
            }
            if identify && scan.is_none() {
                handles::draw_identify(ui.painter(), rect, &id, output.width, output.height, index);
            }
            let pos = ui.input(|i| i.pointer.hover_pos());
            if cursor_idle(state, pos, now) || editor.show_mode {
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            }
            let info = ui.input(|i| i.viewport().clone());
            close = info.close_requested();
            // A backend without native viewports embeds the output as an egui window; its
            // rect inside the editor is still a useful "position" for the placement button.
            outer = if class == egui::ViewportClass::Immediate {
                info.outer_rect
            } else {
                Some(ui.max_rect())
            };
        });
        if scan.is_none() {
            state.pattern = None;
        }
        if go_fullscreen {
            ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Fullscreen(true));
        }
        if let Some(r) = outer {
            editor
                .window_positions
                .insert(id.clone(), (r.min.x, r.min.y));
        }
        if close && !editor.show_mode {
            editor.open_outputs.retain(|o| *o != id);
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use lumap_core::project::{Output, Placement};

    fn output(placement: Option<Placement>) -> Output {
        Output {
            id: "o".into(),
            width: 1920,
            height: 1080,
            enabled: true,
            blend: lumap_core::blend::EdgeBlend::none(),
            color: lumap_core::project::ColorAdjust::default(),
            placement,
            overlay: None,
            scan: None,
        }
    }

    #[test]
    fn cursor_hides_after_idle_and_returns_on_movement() {
        let mut s = OutputUiState::default();
        let t = Instant::now();
        assert!(!cursor_idle(&mut s, Some(egui::Pos2::new(1.0, 1.0)), t));
        assert!(cursor_idle(
            &mut s,
            Some(egui::Pos2::new(1.0, 1.0)),
            t + Duration::from_secs(3)
        ));
        assert!(!cursor_idle(
            &mut s,
            Some(egui::Pos2::new(2.0, 1.0)),
            t + Duration::from_secs(3)
        ));
        assert!(
            !cursor_idle(&mut s, None, t + Duration::from_secs(4)),
            "leaving the window counts as movement"
        );
    }

    #[test]
    fn placed_outputs_are_locked_full_size_windows() {
        let monitors = vec![MonitorInfo {
            name: "m".into(),
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 2.0,
        }];
        let spec = window_spec(
            &output(Some(Placement {
                monitor: "m".into(),
                x: 1920,
                y: 0,
            })),
            &monitors,
        );
        // On a connected display: fullscreen there, opened a quarter of the way in.
        assert_eq!(
            spec,
            WindowSpec {
                pos: Some([1200.0, 135.0]),
                size: [960.0, 540.0],
                locked: true,
                fullscreen: true,
            }
        );
        // A display that is not connected: the window keeps the saved place and size.
        let spec = window_spec(
            &output(Some(Placement {
                monitor: "gone".into(),
                x: 1920,
                y: 0,
            })),
            &monitors,
        );
        assert!(spec.locked && !spec.fullscreen);
        assert_eq!(spec.pos, Some([960.0, 0.0]));
        let spec = window_spec(
            &output(Some(Placement {
                monitor: String::new(),
                x: 10,
                y: 10,
            })),
            &[],
        );
        assert_eq!(spec.pos, Some([10.0, 10.0]));
        assert!(spec.locked);
    }

    #[test]
    fn unplaced_outputs_are_small_previews() {
        let spec = window_spec(&output(None), &[]);
        assert_eq!(
            spec,
            WindowSpec {
                pos: None,
                size: [480.0, 270.0],
                locked: false,
                fullscreen: false,
            }
        );
        let mut tall = output(None);
        tall.width = 100;
        tall.height = 1000;
        assert_eq!(window_spec(&tall, &[]).size[1], 4800.0);
    }
}
