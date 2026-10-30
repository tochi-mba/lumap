//! The camera in Setup: pick it, look through it to aim it, and scan the room. The scan sheet
//! shows each projector being photographed, then what the scan found, with one click to use
//! the measured blends or to see the found surfaces in Map.

use super::super::super::camera::{CameraHub, CameraStatus};
use super::super::super::scan::{Kind, Phase, ScanSession};
use super::super::super::Editor;
use super::super::{Actions, PanelState};
use eframe::egui::{self, RichText, Stroke};
use std::time::Instant;

/// How often the preview texture refreshes.
const PREVIEW_EVERY: f64 = 0.1;

/// The Camera card at the bottom of Setup's projector list.
pub fn card(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    hub: &mut CameraHub,
    state: &mut PanelState,
    actions: &mut Actions,
) {
    crate::theme::section_header(ui, "Camera");
    let names: Vec<String> = hub.devices().iter().map(|d| d.name.clone()).collect();
    if names.is_empty() {
        ui.label(
            RichText::new(
                "No camera found. A webcam that sees the wall lets lumap scan the room, find \
                 surfaces and react to people.",
            )
            .weak()
            .small(),
        );
        if ui.button("Look for cameras again").clicked() {
            hub.refresh();
        }
        return;
    }
    let chosen = editor.project.camera.device.clone();
    let shown = if chosen.is_empty() {
        format!("{} (first camera)", names[0])
    } else {
        chosen.clone()
    };
    ui.horizontal(|ui| {
        egui::ComboBox::from_label("Camera")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for n in &names {
                    if ui.selectable_label(*n == chosen, n).clicked() {
                        let n = n.clone();
                        editor.edit(move |p| p.camera.device = n);
                    }
                }
            });
        if ui
            .small_button("Refresh")
            .on_hover_text("Look for cameras again")
            .clicked()
        {
            hub.refresh();
        }
    });
    let device = if chosen.is_empty() {
        names[0].clone()
    } else {
        chosen
    };
    ui.horizontal(|ui| {
        ui.toggle_value(&mut state.camera_preview, "Look through it")
            .on_hover_text("Shows the camera's picture here, to aim it at the wall");
        match hub.status() {
            CameraStatus::On { .. } => {
                crate::theme::pill(ui, "on", false);
            }
            CameraStatus::Failed(e) => {
                crate::theme::pill(ui, "problem", true).on_hover_text(e);
            }
            CameraStatus::Off => {}
        }
    });
    if state.camera_preview {
        preview(ui, editor, hub, state, &device);
    }
    let scanned = editor
        .project
        .outputs
        .iter()
        .filter(|o| editor.scans.get(&o.id).is_some())
        .count();
    ui.label(
        RichText::new(if scanned == 0 {
            "Scan the room: each projector shows stripes for about 20 seconds while the camera \
             watches. Then lumap knows exactly where every projector lands."
                .to_string()
        } else {
            format!(
                "{scanned} of {} projector(s) scanned. Scan again after moving anything.",
                editor.project.outputs.len()
            )
        })
        .weak()
        .small(),
    );
    ui.horizontal_wrapped(|ui| {
        if crate::theme::primary_button(ui, "Scan the room")
            .on_hover_text("Keep the room dark and still while it runs")
            .clicked()
        {
            actions.scan = Some((Kind::Full, None));
        }
        if scanned > 0
            && ui
                .button("Check alignment")
                .on_hover_text("A two-second check per projector")
                .clicked()
        {
            actions.scan = Some((Kind::Drift, None));
        }
    });
}

/// The camera's picture, with each scanned projector's footprint outlined in its colour.
fn preview(
    ui: &mut egui::Ui,
    editor: &Editor,
    hub: &mut CameraHub,
    state: &mut PanelState,
    device: &str,
) {
    let now = Instant::now();
    let Some(stream) = hub.ensure(device, now) else {
        ui.label(RichText::new("The camera did not start.").color(crate::theme::LIVE));
        return;
    };
    let t = ui.input(|i| i.time);
    let stale = state
        .camera_tex
        .as_ref()
        .is_none_or(|(at, _)| t - *at >= PREVIEW_EVERY);
    if stale {
        if let Some(f) = stream.latest() {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [f.width as usize, f.height as usize],
                &f.rgba,
            );
            let tex = ui
                .ctx()
                .load_texture("camera-preview", image, egui::TextureOptions::LINEAR);
            state.camera_tex = Some((t, tex));
        }
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs_f64(PREVIEW_EVERY));
    let Some((_, tex)) = &state.camera_tex else {
        ui.label(RichText::new("Waiting for the camera…").weak());
        return;
    };
    let size = tex.size_vec2();
    let w = ui.available_width().min(320.0);
    let h = w * size.y / size.x.max(1.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter().image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Camera picture"));
    for (index, o) in editor.project.outputs.iter().enumerate() {
        let Some(scan) = editor.scans.get(&o.id) else {
            continue;
        };
        let (cw, ch) = (scan.corr.cam_w as f32, scan.corr.cam_h as f32);
        let points: Vec<egui::Pos2> = scan
            .map
            .footprint()
            .iter()
            .map(|p| {
                egui::pos2(
                    rect.min.x + p.x as f32 / cw * rect.width(),
                    rect.min.y + p.y as f32 / ch * rect.height(),
                )
            })
            .collect();
        if points.len() > 2 {
            ui.painter().add(egui::Shape::closed_line(
                points,
                Stroke::new(2.0, super::super::super::handles::output_color(index)),
            ));
        }
    }
    ui.label(
        RichText::new("Aim it so every projector's picture is inside the frame.")
            .weak()
            .small(),
    );
}

/// The scan sheet: progress while a scan runs, then what it found.
pub fn sheet(
    ctx: &egui::Context,
    editor: &Editor,
    session: Option<&ScanSession>,
    skipped: &[String],
    state: &mut PanelState,
    actions: &mut Actions,
) {
    let Some(session) = session else { return };
    let progress = session.progress();
    if matches!(progress.phase, Phase::Showing | Phase::Adjust(_)) {
        state.want_camera = true;
    }
    let modal = egui::Modal::new(egui::Id::unique("lumap-scan"))
        .frame(
            egui::Frame::new()
                .fill(crate::theme::PANEL)
                .stroke(Stroke::new(1.0, crate::theme::LINE))
                .corner_radius(egui::CornerRadius::same(12))
                .inner_margin(egui::Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_width(620.0);
            let title = match session.kind {
                Kind::Full => "Scanning the room",
                Kind::Drift => "Checking alignment",
                Kind::Realign => "Re-aligning",
            };
            ui.heading(title);
            match &progress.phase {
                Phase::Showing => {
                    ui.label(format!(
                        "Projector {} of {}: {}",
                        progress.index + 1,
                        progress.of,
                        progress.projector
                    ));
                    live_view(ui, session, state);
                    let f = progress.frame as f32 / progress.frames.max(1) as f32;
                    ui.add(
                        egui::ProgressBar::new(f)
                            .fill(crate::theme::SIGNAL)
                            .text(format!(
                                "{} of {} patterns",
                                progress.frame, progress.frames
                            )),
                    );
                    ui.label(
                        RichText::new(
                            "Keep the room dark and still. Nobody in front of the projectors.",
                        )
                        .weak()
                        .small(),
                    );
                    if ui.button("Stop scanning").clicked() {
                        actions.scan_cancel = true;
                    }
                }
                Phase::Decoding => {
                    live_view(ui, session, state);
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().color(crate::theme::SIGNAL));
                        ui.label("Working out where every pixel lands…");
                    });
                }
                Phase::Adjust(advice) => {
                    ui.label(
                        RichText::new("Fix the camera first")
                            .family(crate::theme::strong_family())
                            .color(crate::theme::LIVE),
                    );
                    for a in advice {
                        ui.label(a);
                    }
                    live_view(ui, session, state);
                    ui.label(
                        RichText::new(
                            "The projector is lit white so you can see what the camera sees.",
                        )
                        .weak()
                        .small(),
                    );
                    ui.horizontal(|ui| {
                        if crate::theme::primary_button(ui, "Check again").clicked() {
                            actions.scan_retry = true;
                        }
                        if ui.button("Scan anyway").clicked() {
                            actions.scan_accept = true;
                        }
                        if ui.button("Stop scanning").clicked() {
                            actions.scan_cancel = true;
                        }
                    });
                }
                Phase::Done => done(ui, editor, session, skipped, actions),
                Phase::Failed(why) => {
                    ui.label(RichText::new(why).color(crate::theme::LIVE));
                    ui.horizontal(|ui| {
                        if ui.button("Try again").clicked() {
                            actions.scan = Some((session.kind, None));
                        }
                        if ui.button("Close").clicked() {
                            actions.scan_close = true;
                        }
                    });
                }
                Phase::Cancelled => {
                    ui.label("Stopped.");
                    if ui.button("Close").clicked() {
                        actions.scan_close = true;
                    }
                }
            }
        });
    if modal.should_close() && session.finished() {
        actions.scan_close = true;
    }
}

/// The camera's live picture, with the projection's outline from the framing check and a scan
/// line sweeping across it, beside the map filling in as stripes are read.
fn live_view(ui: &mut egui::Ui, session: &ScanSession, state: &mut PanelState) {
    let t = ui.input(|i| i.time);
    ui.ctx().request_repaint();
    if let Some(frame) = state.camera_frame.clone() {
        if state.camera_tex.as_ref().is_none_or(|(at, _)| t - *at >= 0.08) {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [frame.width as usize, frame.height as usize],
                &frame.rgba,
            );
            let tex = ui
                .ctx()
                .load_texture("camera-preview", image, egui::TextureOptions::LINEAR);
            state.camera_tex = Some((t, tex));
        }
    }
    let progress = session.progress();
    let key = (progress.index * 1000 + progress.frame) as u64;
    if let Some(p) = session.preview() {
        if state.scan_tex.as_ref().is_none_or(|(k, _)| *k != key) {
            let img = p.image();
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [img.width as usize, img.height as usize],
                &img.data,
            );
            let tex = ui
                .ctx()
                .load_texture("scan-preview", image, egui::TextureOptions::NEAREST);
            state.scan_tex = Some((key, tex));
        }
    }
    let w = (ui.available_width() - 8.0) / 2.0;
    let h = w * 9.0 / 16.0;
    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        ui.painter().rect_filled(rect, 6.0, crate::theme::INK);
        if let Some((_, tex)) = &state.camera_tex {
            ui.painter().image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Image, true, "What the camera sees")
        });
        if let Some(f) = session.framing() {
            if let Some([x0, y0, x1, y1]) = f.bounds {
                let (cw, ch) = (f.footprint.width as f32, f.footprint.height as f32);
                let to = |x: u32, y: u32| {
                    egui::pos2(
                        rect.min.x + x as f32 / cw * rect.width(),
                        rect.min.y + y as f32 / ch * rect.height(),
                    )
                };
                let colour = if f.problems.is_empty() {
                    crate::theme::SIGNAL
                } else {
                    crate::theme::LIVE
                };
                ui.painter().rect_stroke(
                    egui::Rect::from_min_max(to(x0, y0), to(x1, y1)),
                    2.0,
                    Stroke::new(2.0, colour),
                    egui::StrokeKind::Outside,
                );
            }
        }
        if matches!(session.phase(), Phase::Showing) {
            // A soft line sweeping down the picture while the stripes are photographed.
            let y = rect.min.y + ((t * 0.45).fract() as f32) * rect.height();
            for (k, a) in [(0.0, 200u8), (3.0, 90), (7.0, 35)] {
                ui.painter().hline(
                    rect.x_range(),
                    y - k,
                    Stroke::new(2.0, crate::theme::SIGNAL.gamma_multiply(f32::from(a) / 255.0)),
                );
            }
        }
        let (rect, response) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        ui.painter().rect_filled(rect, 6.0, crate::theme::INK);
        if let Some((_, tex)) = &state.scan_tex {
            ui.painter().image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Image, true, "Where each pixel lands so far")
        });
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("What the camera sees").weak().small());
        ui.add_space(w - 110.0);
        ui.label(
            RichText::new("Where each pixel lands: colour across, brightness down")
                .weak()
                .small(),
        );
    });
}

/// What a finished scan found.
fn done(
    ui: &mut egui::Ui,
    editor: &Editor,
    session: &ScanSession,
    skipped: &[String],
    actions: &mut Actions,
) {
    if session.kind == Kind::Drift {
        let reports = session.drifts();
        for d in reports {
            let text = if d.camera_moved > super::super::super::scan::CAMERA_MOVED {
                format!(
                    "{}: the camera moved since the scan. Scan again.",
                    d.projector
                )
            } else if d.mean < 2.0 {
                format!("{}: where it was scanned.", d.projector)
            } else {
                format!("{}: moved {:.0} px since its scan.", d.projector, d.mean)
            };
            ui.label(text);
        }
        ui.horizontal(|ui| {
            if reports.iter().any(|d| d.mean >= 2.0)
                && crate::theme::primary_button(ui, "Re-align").clicked()
            {
                actions.scan = Some((Kind::Realign, None));
            }
            if ui.button("Close").clicked() {
                actions.scan_close = true;
            }
        });
        return;
    }
    egui::Grid::new("lumap-scan-results")
        .num_columns(3)
        .spacing(egui::vec2(12.0, 6.0))
        .show(ui, |ui| {
            for o in &editor.project.outputs {
                let Some(scan) = editor.scans.get(&o.id) else {
                    continue;
                };
                ui.label(RichText::new(&o.id).family(crate::theme::strong_family()));
                ui.label(format!("{:.0}% seen", scan.stats.coverage * 100.0));
                let cut = scan.map.cut_off_edges(scan.corr.cam_w, scan.corr.cam_h);
                if cut.is_empty() {
                    ui.label(format!("{} surface(s) found", scan.found.len()));
                } else {
                    ui.label(
                        RichText::new(
                            format!("cut off at the {:?}: aim the camera wider", cut[0])
                                .to_lowercase(),
                        )
                        .color(crate::theme::LIVE),
                    );
                }
                ui.end_row();
            }
        });
    for problem in session.problems() {
        ui.label(RichText::new(problem).color(crate::theme::LIVE));
    }
    if !skipped.is_empty() {
        ui.label(
            RichText::new(format!(
                "Left out, because they are not on a display: {}.",
                skipped.join(", ")
            ))
            .weak()
            .small(),
        );
    }
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        if crate::theme::primary_button(ui, "See them in Map").clicked() {
            actions.review_found = true;
        }
        if editor.scans.ready().len() > 1 && ui.button("Use measured blends").clicked() {
            actions.measured_blends = true;
        }
        if ui.button("Close").clicked() {
            actions.scan_close = true;
        }
    });
}
