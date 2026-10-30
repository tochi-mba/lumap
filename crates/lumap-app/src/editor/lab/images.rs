//! What the lab reports: the show as JSON, PNGs of camera photos, scans and what the camera's
//! vision sees, drawn so a person (or an AI looking at the picture) can judge them at a glance.

use super::super::LumapApp;
use eframe::egui;
use lumap_control::lab::{Reply, ScanImage};
use lumap_core::geom::Vec2;
use lumap_vision::{Gray, Rgba};
use serde_json::{json, Value};
use std::time::Instant;

/// A sortable timestamp for folder names.
pub fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("{secs}")
}

/// An RGBA buffer as a PNG reply.
pub fn png_reply(width: u32, height: u32, rgba: Vec<u8>) -> Reply {
    let Some(img) = image::RgbaImage::from_raw(width, height, rgba) else {
        return Reply::Error(500, "image has the wrong size".into());
    };
    let mut bytes = Vec::new();
    match img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    ) {
        Ok(()) => Reply::Png(bytes),
        Err(e) => Reply::Error(500, e.to_string()),
    }
}

/// A screenshot as a PNG reply.
pub fn color_image_png(img: &egui::ColorImage) -> Reply {
    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
    png_reply(img.size[0] as u32, img.size[1] as u32, rgba)
}

/// The show, the editor and the camera, as the lab's `/lab/state`.
pub fn state(app: &mut LumapApp) -> Value {
    let e = &app.editor;
    let r = e.readiness();
    let checks: Vec<Value> = e
        .preflight(&app.monitors, app.panels.ffmpeg_ok)
        .into_iter()
        .map(|c| json!({ "ok": c.ok, "text": c.text, "fix": c.fix.map(|f| f.label()) }))
        .collect();
    let projectors: Vec<Value> = e
        .project
        .outputs
        .iter()
        .map(|o| {
            let scan = e.scans.get(&o.id).map(|s| {
                json!({
                    "coverage": s.stats.coverage,
                    "valid_fraction": s.stats.valid_fraction,
                    "found": s.found.len(),
                    "taken": s.stats.taken,
                })
            });
            json!({
                "id": o.id,
                "size": [o.width, o.height],
                "enabled": o.enabled,
                "placement": o.placement.as_ref().map(|p| json!({ "display": p.monitor, "x": p.x, "y": p.y })),
                "window_open": e.open_outputs.contains(&o.id),
                "overlay": o.overlay.map(|v| v.label()),
                "scan": scan,
            })
        })
        .collect();
    let surfaces: Vec<Value> = e
        .project
        .surfaces
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "projector": s.output,
                "content": s.source,
                "visible": s.visible,
                "opacity": s.opacity,
                "locked": s.locked,
                "points": s.warp.points().iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
                "cutouts": s.masks.len(),
            })
        })
        .collect();
    let content: Vec<Value> = e
        .project
        .sources
        .iter()
        .map(|s| json!({ "id": s.id, "kind": s.kind.label(), "live": e.sources.frame(&s.id).is_some() }))
        .collect();
    let cues: Vec<Value> = e
        .project
        .cues
        .iter()
        .enumerate()
        .map(|(i, c)| json!({ "id": c.id, "name": c.name, "fade": c.fade_seconds, "running": e.cues.current == Some(i) }))
        .collect();
    let step = app.panels.step;
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "home": app.panels.home,
        "step": { "number": step.number(), "label": step.label() },
        "show": { "name": e.project.name, "path": e.path.as_ref().map(|p| p.display().to_string()), "dirty": e.dirty },
        "live": e.show_mode,
        "blackout": e.project.blackout,
        "master": e.project.master,
        "selection": format!("{:?}", e.selection),
        "status": e.status,
        "readiness": { "projectors": r.projectors, "projectors_open": r.projectors_open, "surfaces": r.surfaces, "surfaces_with_content": r.surfaces_with_content, "cues": r.cues },
        "preflight": checks,
        "projectors": projectors,
        "surfaces": surfaces,
        "content": content,
        "cues": cues,
        "camera": camera_info(app),
        "scan": app.scan.as_ref().map(|s| format!("{:?}", s.progress())),
        "fps": app.fps(),
    })
}

/// The cameras and the one in use.
pub fn camera_info(app: &mut LumapApp) -> Value {
    let devices: Vec<Value> = app
        .camera
        .devices()
        .iter()
        .map(|d| json!({ "name": d.name, "id": d.id }))
        .collect();
    let status = match app.camera.status() {
        super::super::camera::CameraStatus::Off => json!({ "state": "off" }),
        super::super::camera::CameraStatus::On { name, size } => {
            json!({ "state": "on", "name": name, "size": [size.0, size.1] })
        }
        super::super::camera::CameraStatus::Failed(e) => json!({ "state": "failed", "error": e }),
    };
    json!({
        "chosen": app.editor.project.camera.device,
        "devices": devices,
        "status": status,
        "vision_fps": app.vision.as_ref().map(|v| v.seen().fps),
    })
}

/// One of a projector's scan pictures.
pub fn scan_image(app: &LumapApp, projector: &str, which: ScanImage) -> Reply {
    let Some(scan) = app.editor.scans.get(projector) else {
        return Reply::Error(404, format!("{projector} has no scan."));
    };
    match which {
        ScanImage::Photo => {
            let mut photo = scan.photo.clone();
            outline_found(&mut photo, &scan.found);
            png_reply(photo.width, photo.height, photo.data)
        }
        ScanImage::Map => {
            let c = &scan.corr;
            let img = Rgba::from_fn(c.cam_w, c.cam_h, |x, y| match c.get(x, y) {
                Some(p) => [
                    (p.x / f64::from(c.proj_w) * 255.0) as u8,
                    (p.y / f64::from(c.proj_h) * 255.0) as u8,
                    96,
                    255,
                ],
                None => [0, 0, 0, 255],
            });
            png_reply(img.width, img.height, img.data)
        }
        ScanImage::Footprint => {
            let mut img = Rgba::from_gray(&scan.white);
            let hull = scan.map.footprint();
            polyline(&mut img, &hull, true, [215, 255, 63, 255]);
            png_reply(img.width, img.height, img.data)
        }
    }
}

/// The surfaces a scan found, as JSON.
pub fn found(app: &LumapApp, projector: &str) -> Reply {
    let Some(scan) = app.editor.scans.get(projector) else {
        return Reply::Error(404, format!("{projector} has no scan."));
    };
    let (w, h) = (f64::from(scan.photo.width), f64::from(scan.photo.height));
    let list: Vec<Value> = scan
        .found
        .iter()
        .map(|s| {
            json!({
                "kind": format!("{:?}", s.kind),
                "label": s.label,
                "score": s.score,
                "points_px": s.points.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
                "points": s.points.iter().map(|p| [p.x / w, p.y / h]).collect::<Vec<_>>(),
            })
        })
        .collect();
    Reply::Json(json!({ "projector": projector, "found": list }))
}

/// Debug pictures of what the camera's vision sees: `camera`, `edges-<projector>`,
/// `shadow-<projector>`, `spot`.
pub fn vision(app: &mut LumapApp, name: &str, now: Instant) -> Reply {
    if let Some(p) = name.strip_prefix("edges-") {
        return match app.editor.scans.get(p) {
            Some(s) => gray_png(&s.edges),
            None => Reply::Error(404, format!("{p} has no scan.")),
        };
    }
    let seen = app.vision.as_ref().map(super::super::vision::Vision::seen);
    if let Some(p) = name.strip_prefix("shadow-") {
        return match seen.and_then(|s| s.shadows.get(p).cloned()) {
            Some(mask) => gray_png(&mask),
            None => Reply::Error(
                404,
                format!("No shadow picture for {p} (no shadow effect running)."),
            ),
        };
    }
    let Some(device) = app.camera_device() else {
        return Reply::Error(409, "There is no camera.".into());
    };
    let Some(frame) = app.camera.latest(&device, now) else {
        return Reply::Error(503, "The camera has no picture yet.".into());
    };
    let mut img = Rgba {
        width: frame.width,
        height: frame.height,
        data: frame.rgba.clone(),
    };
    match name {
        "camera" => {}
        "spot" => {
            if let Some((p, _)) = seen.and_then(|s| s.spot) {
                ring(&mut img, p, 12.0, [215, 255, 63, 255]);
            }
        }
        _ => return Reply::Error(404, format!("No vision picture called {name}.")),
    }
    png_reply(img.width, img.height, img.data)
}

fn gray_png(g: &Gray) -> Reply {
    let img = Rgba::from_gray(g);
    png_reply(img.width, img.height, img.data)
}

/// Draws each found surface's outline on a photo.
fn outline_found(img: &mut Rgba, found: &[lumap_vision::find::Suggestion]) {
    for s in found {
        polyline(img, &s.points, true, [215, 255, 63, 255]);
    }
}

/// A one-pixel polyline.
fn polyline(img: &mut Rgba, points: &[Vec2], closed: bool, c: [u8; 4]) {
    let n = points.len();
    if n < 2 {
        return;
    }
    let segments = if closed { n } else { n - 1 };
    for i in 0..segments {
        let (a, b) = (points[i], points[(i + 1) % n]);
        let steps = (a.distance(b).ceil() as usize).max(1);
        for k in 0..=steps {
            let p = a.lerp(b, k as f64 / steps as f64);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1)] {
                let (x, y) = (p.x as i64 + dx, p.y as i64 + dy);
                if x >= 0 && y >= 0 && (x as u32) < img.width && (y as u32) < img.height {
                    img.set(x as u32, y as u32, c);
                }
            }
        }
    }
}

/// A ring around a point.
fn ring(img: &mut Rgba, c: Vec2, r: f64, colour: [u8; 4]) {
    let points: Vec<Vec2> = (0..48)
        .map(|i| {
            let a = f64::from(i) / 48.0 * std::f64::consts::TAU;
            Vec2::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    polyline(img, &points, true, colour);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn pictures_encode_and_outlines_stay_inside() {
        assert!(matches!(png_reply(2, 2, vec![0; 16]), Reply::Png(_)));
        assert!(matches!(png_reply(2, 2, vec![0; 3]), Reply::Error(500, _)));
        let mut img = Rgba::new(20, 20);
        polyline(
            &mut img,
            &[
                Vec2::new(2.0, 2.0),
                Vec2::new(30.0, 2.0),
                Vec2::new(2.0, 15.0),
            ],
            true,
            [1, 2, 3, 255],
        );
        assert_eq!(img.get(5, 2), [1, 2, 3, 255]);
        ring(&mut img, Vec2::new(10.0, 10.0), 4.0, [9, 9, 9, 255]);
        assert_eq!(img.get(14, 10), [9, 9, 9, 255]);
        polyline(&mut img, &[Vec2::new(1.0, 1.0)], false, [0; 4]);
        assert!(stamp().parse::<u64>().is_ok());
        let shot = egui::ColorImage::new([2, 1], vec![egui::Color32::RED; 2]);
        assert!(matches!(color_image_png(&shot), Reply::Png(_)));
    }
}
