//! The lab's actions: each one is what a person would do through the interface, done directly
//! on the editor, and answered with what happened.

use super::super::panels::shell::Step;
use super::super::{LumapApp, Pending, Selection};
use eframe::egui;
use lumap_control::lab::{Action, Pattern};
use lumap_core::geom::Vec2;
use lumap_core::params::ParamValue;
use lumap_core::project::{Overlay, Project};
use lumap_core::warp::{Quad, WarpKind};
use lumap_sources::Frame;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// Runs one action and reports `{ "do": ..., "ok": ..., "error"?: ... }`.
pub fn run(app: &mut LumapApp, ctx: &egui::Context, action: &Action, now: Instant) -> Value {
    let name = serde_json::to_value(action)
        .ok()
        .and_then(|v| v.get("do").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default();
    match apply(app, ctx, action, now) {
        Ok(extra) => {
            let mut v = json!({ "do": name, "ok": true });
            if let (Some(obj), Some(extra)) = (v.as_object_mut(), extra) {
                obj.insert("result".into(), extra);
            }
            v
        }
        Err(e) => json!({ "do": name, "ok": false, "error": e }),
    }
}

#[allow(clippy::too_many_lines)]
fn apply(
    app: &mut LumapApp,
    ctx: &egui::Context,
    action: &Action,
    now: Instant,
) -> Result<Option<Value>, String> {
    let e = &mut app.editor;
    match action {
        Action::Open { path } => {
            app.perform(Pending::Open(PathBuf::from(path)));
            app.panels.home = false;
        }
        Action::Demo => {
            app.perform(Pending::New);
            app.panels.home = false;
        }
        Action::New => {
            app.editor = super::super::Editor::new(Project::default(), None);
            app.panels.home = false;
        }
        Action::Save { path } => match path {
            Some(p) => app.editor.save_as(std::path::Path::new(p))?,
            None => match app.editor.path.clone() {
                Some(p) => app.editor.save_as(&p)?,
                None => return Err("The show has no file yet: give a path.".into()),
            },
        },
        Action::Step { step } => {
            let s = Step::ALL
                .get(usize::from(*step).saturating_sub(1))
                .copied()
                .ok_or("Steps are 1 to 4.")?;
            app.panels.home = false;
            app.panels.step = s;
        }
        Action::Select { what, id } => {
            let id = id.clone().unwrap_or_default();
            e.selection = match what.as_str() {
                "projector" | "output" => Selection::Output(id),
                "surface" => Selection::Surface { id, mask: None },
                "content" | "source" => Selection::Source(id),
                _ => Selection::None,
            };
            e.extra_selected.clear();
        }
        Action::AddSurface { projector } => {
            if e.project.output(projector).is_none() {
                return Err(format!("No projector {projector}."));
            }
            e.add_surface(projector);
            let id = match &e.selection {
                Selection::Surface { id, .. } => id.clone(),
                _ => String::new(),
            };
            return Ok(Some(json!({ "surface": id })));
        }
        Action::SetCorners { surface, corners } => {
            let q = Quad {
                corners: corners.map(|[x, y]| Vec2::new(x, y)),
            };
            if !e.set_warp(surface, WarpKind::Quad(q)) {
                return Err(format!("No surface {surface}."));
            }
            e.commit();
        }
        Action::ApplyLook { look, surface } => {
            let l = lumap_core::looks::find(look).ok_or_else(|| format!("No look {look}."))?;
            match surface {
                Some(s) => {
                    let s = s.clone();
                    e.edit(move |p| {
                        l.apply(p, &s);
                    });
                    e.reload_sources();
                }
                None => super::super::panels::looks::apply_look_everywhere(e, l),
            }
        }
        Action::Param { address, value } => {
            let v = match value {
                Value::Bool(b) => ParamValue::Bool(*b),
                Value::Number(n) => ParamValue::Number(n.as_f64().unwrap_or(0.0)),
                _ => return Err("Values are numbers or true/false.".into()),
            };
            if !e.set_param(address, v) {
                return Err(format!("{address} cannot be set."));
            }
            e.commit();
        }
        Action::StoreCue { name, fade } => {
            let id = e.store_cue(name, *fade);
            return Ok(Some(json!({ "cue": id })));
        }
        Action::Go { cue } => {
            let t = e.sources.time();
            let ok = match cue {
                Some(id) => e.cue_go_id(id, t),
                None => e.cue_go_next(t).is_some(),
            };
            if !ok {
                return Err("No cue to go to.".into());
            }
        }
        Action::Identify => e.identify(now),
        Action::Overlay { projector, overlay } => {
            let o = match overlay.as_deref() {
                None | Some("off" | "") => None,
                Some(name) => Some(
                    Overlay::ALL
                        .into_iter()
                        .find(|o| o.label().eq_ignore_ascii_case(name) || format!("{o:?}").eq_ignore_ascii_case(name))
                        .ok_or_else(|| format!("No overlay {name}."))?,
                ),
            };
            if !e.set_overlay(projector, o) {
                return Err(format!("No projector {projector}."));
            }
        }
        Action::ShowPattern { projector, pattern } => {
            let (w, h) = e
                .project
                .output(projector)
                .map(|o| (o.width, o.height))
                .ok_or_else(|| format!("No projector {projector}."))?;
            if !e.open_outputs.contains(projector) {
                e.open_outputs.push(projector.clone());
            }
            e.scan_pattern = match pattern {
                Pattern::Clear => None,
                p => Some((projector.clone(), Arc::new(pattern_frame(p, w, h)?))),
            };
        }
        Action::OpenWindow { projector } => {
            if e.project.output(projector).is_none() {
                return Err(format!("No projector {projector}."));
            }
            if !e.open_outputs.contains(projector) {
                e.open_outputs.push(projector.clone());
            }
        }
        Action::CloseWindow { projector } => e.open_outputs.retain(|o| o != projector),
        Action::GoLive => e.enter_show_mode(),
        Action::LeaveLive => e.exit_show_mode(),
        Action::Undo => {
            e.undo();
        }
        Action::Redo => {
            e.redo();
        }
        Action::Wait { .. } => {}
        Action::Camera { device } => {
            let d = device.clone();
            e.edit(move |p| p.camera.device = d);
            app.camera.refresh();
        }
    }
    ctx.request_repaint();
    Ok(None)
}

/// A full-screen test image for a projector.
fn pattern_frame(p: &Pattern, w: u32, h: u32) -> Result<Frame, String> {
    Ok(match p {
        Pattern::Solid { rgb } => Frame::solid(w, h, [rgb[0], rgb[1], rgb[2], 255]),
        Pattern::Grid => {
            let step = (w.min(h) / 12).max(8);
            Frame::from_fn(w, h, |x, y| {
                if x % step < 2 || y % step < 2 {
                    [255, 255, 255, 255]
                } else {
                    [0, 0, 0, 255]
                }
            })
        }
        Pattern::Checker { size } => {
            let s = (*size).max(1);
            Frame::from_fn(w, h, |x, y| {
                if (x / s + y / s) % 2 == 0 {
                    [255, 255, 255, 255]
                } else {
                    [0, 0, 0, 255]
                }
            })
        }
        Pattern::Png { path } => {
            let img = image::open(path).map_err(|e| e.to_string())?.into_rgba8();
            let img = image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle);
            Frame {
                width: w,
                height: h,
                rgba: img.into_raw(),
            }
        }
        Pattern::Clear => Frame::solid(w, h, [0, 0, 0, 255]),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_patterns_have_the_projector_size() {
        let g = pattern_frame(&Pattern::Grid, 64, 32).unwrap();
        assert_eq!((g.width, g.height), (64, 32));
        assert_eq!(&g.rgba[..4], &[255, 255, 255, 255]);
        let c = pattern_frame(&Pattern::Checker { size: 4 }, 8, 8).unwrap();
        assert_eq!(&c.rgba[16..20], &[0, 0, 0, 255]);
        let s = pattern_frame(&Pattern::Solid { rgb: [1, 2, 3] }, 2, 2).unwrap();
        assert_eq!(&s.rgba[..4], &[1, 2, 3, 255]);
        assert!(pattern_frame(&Pattern::Png { path: "missing.png".into() }, 2, 2).is_err());
        assert_eq!(pattern_frame(&Pattern::Clear, 2, 2).unwrap().rgba[0], 0);
    }
}
