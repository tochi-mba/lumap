//! Mapping with the wall in view: the projector's-eye photo from the scan drawn over the Map
//! canvas, the surfaces the scan found shown as dashed outlines you click to use, and Pick an
//! object, where a click on the photo grows an outline that becomes a surface or a cut-out.

use super::canvas::{CanvasState, Tool};
use super::handles::Mapping;
use super::scan::ScanData;
use super::{Editor, Selection};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use lumap_core::geom::Vec2;
use lumap_core::mask::Mask;
use lumap_core::warp::{Quad, WarpKind};
use lumap_vision::find::{Suggestion, SuggestionKind};
use lumap_vision::wand::Outline;
use std::sync::Arc;

/// Draws the wall photo over the canvas, when it is on and the projector has a scan.
pub fn backdrop(ui: &egui::Ui, rect: Rect, scan: &Arc<ScanData>, state: &mut CanvasState) {
    if !state.wall_photo {
        return;
    }
    let key = Arc::as_ptr(scan) as usize;
    if state.wall_tex.as_ref().is_none_or(|(k, _)| *k != key) {
        let f = scan.photo_frame();
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [f.width as usize, f.height as usize],
            &f.rgba,
        );
        let tex = ui
            .ctx()
            .load_texture("wall-photo", image, egui::TextureOptions::LINEAR);
        state.wall_tex = Some((key, tex));
    }
    if let Some((_, tex)) = &state.wall_tex {
        let alpha = (state.wall_opacity.clamp(0.0, 1.0) * 255.0) as u8;
        ui.painter().image(
            tex.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::from_white_alpha(alpha),
        );
    }
}

/// Found surfaces and Pick an object. Returns true when the click was theirs, so the canvas
/// does not also treat it as a selection click.
pub fn tools(
    ui: &egui::Ui,
    editor: &mut Editor,
    state: &mut CanvasState,
    response: &egui::Response,
    map: &Mapping,
    output: &str,
    scan: Option<&Arc<ScanData>>,
) -> bool {
    let Some(scan) = scan else {
        state.tool = Tool::Edit;
        state.picked = None;
        return false;
    };
    let size = (f64::from(scan.photo.width), f64::from(scan.photo.height));
    let to_screen = |p: Vec2| map.to_screen(Vec2::new(p.x / size.0, p.y / size.1));
    let painter = ui.painter_at(map.rect.expand(2.0));
    if state.show_found {
        for (i, s) in scan.found.iter().enumerate() {
            let pts: Vec<Pos2> = s.points.iter().map(|p| to_screen(*p)).collect();
            let chosen = state.found_pick == Some(i);
            dashed(
                &painter,
                &pts,
                if chosen {
                    crate::theme::SIGNAL
                } else {
                    Color32::from_rgba_unmultiplied(215, 255, 63, 150)
                },
            );
            if let Some(first) = pts.first() {
                painter.text(
                    *first + egui::vec2(4.0, 4.0),
                    egui::Align2::LEFT_TOP,
                    &s.label,
                    egui::FontId::proportional(11.0),
                    crate::theme::SIGNAL,
                );
            }
        }
    }
    if let Some(o) = &state.picked {
        let pts: Vec<Pos2> = o.points.iter().map(|p| to_screen(*p)).collect();
        dashed(&painter, &pts, crate::theme::SIGNAL);
    }
    let mut consumed = false;
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let n = map.to_norm(pos);
            let px = Vec2::new(n.x * size.0, n.y * size.1);
            if state.tool == Tool::Pick {
                state.picked = lumap_vision::wand::pick(&scan.photo, px, 24.0);
                if state.picked.is_none() {
                    editor.status = "Nothing to pick there: click inside an object.".into();
                }
                consumed = true;
            } else if state.show_found {
                let hit = scan.found.iter().position(|s| contains(&s.points, px));
                if hit.is_some() {
                    state.found_pick = hit;
                    consumed = true;
                }
            }
        }
    }
    popups(ui, editor, state, map, output, scan, size);
    consumed
}

/// The little menus beside a chosen suggestion or a picked outline.
fn popups(
    ui: &egui::Ui,
    editor: &mut Editor,
    state: &mut CanvasState,
    map: &Mapping,
    output: &str,
    scan: &ScanData,
    size: (f64, f64),
) {
    let norm = |p: Vec2| Vec2::new(p.x / size.0, p.y / size.1);
    if let Some(i) = state.found_pick {
        let Some(s) = scan.found.get(i).cloned() else {
            state.found_pick = None;
            return;
        };
        let anchor = map.to_screen(norm(s.points[0])) + egui::vec2(0.0, -34.0);
        let mut close = false;
        egui::Area::new(egui::Id::unique("lumap-found-menu"))
            .fixed_pos(anchor)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                crate::theme::toast_frame(false).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if crate::theme::primary_button(ui, "Use this surface").clicked() {
                            add_suggestion(editor, output, &s, size);
                            close = true;
                        }
                        if ui.button(format!("Use all {}", scan.found.len())).clicked() {
                            for s in &scan.found {
                                add_suggestion(editor, output, s, size);
                            }
                            close = true;
                        }
                        if ui.button("Not this").clicked() {
                            close = true;
                        }
                    });
                });
            });
        if close {
            state.found_pick = None;
        }
    }
    if let Some(o) = state.picked.clone() {
        let anchor = map.to_screen(norm(o.points[0])) + egui::vec2(0.0, -34.0);
        let selected = match &editor.selection {
            Selection::Surface { id, .. } => Some(id.clone()),
            _ => None,
        };
        let mut close = false;
        egui::Area::new(egui::Id::unique("lumap-pick-menu"))
            .fixed_pos(anchor)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                crate::theme::toast_frame(false).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if crate::theme::primary_button(ui, "Make a surface").clicked() {
                            add_outline(editor, output, &o, size);
                            close = true;
                        }
                        if let Some(sid) = &selected {
                            if ui
                                .button(format!("Cut it out of {sid}"))
                                .on_hover_text("Light stays off this object")
                                .clicked()
                            {
                                cut_out(editor, sid, &o, size);
                                close = true;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                });
            });
        if close {
            state.picked = None;
        }
    }
}

/// Adds a surface shaped like a suggestion: a quad pinned to its corners, a polygon or circle as
/// a surface over its bounds with a window cut-out of its outline.
pub fn add_suggestion(editor: &mut Editor, output: &str, s: &Suggestion, size: (f64, f64)) {
    let pts: Vec<Vec2> = s
        .points
        .iter()
        .map(|p| Vec2::new(p.x / size.0, p.y / size.1))
        .collect();
    let name = s.label.to_lowercase().replace(' ', "-");
    match s.kind {
        SuggestionKind::Quad if pts.len() == 4 => {
            let q = Quad {
                corners: [pts[0], pts[1], pts[2], pts[3]],
            };
            editor.add_surface_shaped(output, &name, WarpKind::Quad(q), Vec::new());
        }
        _ => add_bounded(editor, output, &name, &pts),
    }
}

/// Adds a surface for a picked outline (a quad when it has four clear corners).
pub fn add_outline(editor: &mut Editor, output: &str, o: &Outline, size: (f64, f64)) {
    let n = |p: &Vec2| Vec2::new(p.x / size.0, p.y / size.1);
    match &o.quad {
        Some(q) => {
            let quad = Quad {
                corners: [n(&q[0]), n(&q[1]), n(&q[2]), n(&q[3])],
            };
            editor.add_surface_shaped(output, "object", WarpKind::Quad(quad), Vec::new());
        }
        None => {
            let pts: Vec<Vec2> = o.points.iter().map(n).collect();
            add_bounded(editor, output, "object", &pts);
        }
    }
}

/// A surface covering the outline's bounding box, with a window cut-out of the outline (in the
/// surface's own 0..1 space, which over an axis-aligned box is the box-relative position).
fn add_bounded(editor: &mut Editor, output: &str, name: &str, pts: &[Vec2]) {
    if pts.len() < 3 {
        return;
    }
    let (mut lo, mut hi) = (pts[0], pts[0]);
    for p in pts {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let (w, h) = ((hi.x - lo.x).max(1e-6), (hi.y - lo.y).max(1e-6));
    let quad = Quad {
        corners: [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)],
    };
    let local: Vec<Vec2> = pts
        .iter()
        .map(|p| Vec2::new((p.x - lo.x) / w, (p.y - lo.y) / h))
        .collect();
    editor.add_surface_shaped(
        output,
        name,
        WarpKind::Quad(quad),
        vec![Mask::window(local)],
    );
}

/// Cuts an outline out of a surface (a hole where the object is).
fn cut_out(editor: &mut Editor, surface: &str, o: &Outline, size: (f64, f64)) {
    let Some(s) = editor.project.surface(surface).cloned() else {
        return;
    };
    let WarpKind::Quad(q) = &s.warp else {
        editor.status = "Cut-outs from the wall photo need a four-corner surface.".into();
        return;
    };
    let Ok(h) = q.homography() else { return };
    let local: Vec<Vec2> = o
        .points
        .iter()
        .filter_map(|p| h.unmap(Vec2::new(p.x / size.0, p.y / size.1)))
        .collect();
    if local.len() < 3 {
        return;
    }
    let id = surface.to_string();
    editor.edit(move |p| {
        if let Some(s) = p.surface_mut(&id) {
            s.masks.push(Mask::hole(local));
        }
    });
}

/// Point in polygon (even-odd).
fn contains(poly: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

/// A dashed closed outline.
fn dashed(painter: &egui::Painter, pts: &[Pos2], color: Color32) {
    if pts.len() < 2 {
        return;
    }
    let mut closed = pts.to_vec();
    closed.push(pts[0]);
    painter.add(egui::Shape::dashed_line(
        &closed,
        Stroke::new(2.0, color),
        7.0,
        5.0,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_inside_polygons_are_found() {
        let square = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        assert!(contains(&square, Vec2::new(5.0, 5.0)));
        assert!(!contains(&square, Vec2::new(15.0, 5.0)));
    }
}
