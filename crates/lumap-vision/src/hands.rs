//! MediaPipe palm detection and hand landmarks through pure-Rust `tract` inference.
use crate::{gestures::HandPose, Rgba};
use lumap_core::geom::Vec2;
use std::{path::Path, sync::Arc};
use tract_onnx::prelude::*;
type Plan = Arc<TypedRunnableModel>;
#[derive(Clone, Copy)]
struct Anchor {
    x: f32,
    y: f32,
}
#[derive(Clone, Copy)]
struct Palm {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    score: f32,
}
/// Loaded palm detector and landmark estimator.
pub struct HandTracker {
    palm: Plan,
    landmark: Plan,
    anchors: Vec<Anchor>,
}
impl HandTracker {
    /// Loads both pinned ONNX models from `dir`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let load = |name: &str| -> Result<Plan, String> {
            onnx()
                .model_for_path(dir.join(name))
                .map_err(|e| e.to_string())?
                .into_optimized()
                .map_err(|e| e.to_string())?
                .into_runnable()
                .map_err(|e| e.to_string())
        };
        Ok(Self {
            palm: load("palm_detection_mediapipe_2023feb.onnx")?,
            landmark: load("handpose_estimation_mediapipe_2023feb.onnx")?,
            anchors: anchors(),
        })
    }
    /// Tracks up to two hands in one camera frame.
    pub fn track(&mut self, frame: &Rgba) -> Vec<HandPose> {
        let input = image_tensor(frame, 192, None);
        let Ok(outputs) = self.palm.run(tvec!(input.into_tvalue())) else {
            return Vec::new();
        };
        let mut scores = None;
        let mut boxes = None;
        for o in &outputs {
            let Ok(v) = o.to_plain_array_view::<f32>() else {
                continue;
            };
            if v.len() >= self.anchors.len() * 18 {
                boxes = Some(v.iter().copied().collect::<Vec<_>>())
            } else if v.len() >= self.anchors.len() {
                scores = Some(v.iter().copied().collect::<Vec<_>>())
            }
        }
        let (Some(scores), Some(boxes)) = (scores, boxes) else {
            return Vec::new();
        };
        let mut palms = Vec::new();
        for (i, a) in self.anchors.iter().enumerate() {
            let score = 1.0 / (1.0 + (-scores[i]).exp());
            if score < 0.65 {
                continue;
            }
            let b = i * 18;
            if b + 3 >= boxes.len() {
                break;
            }
            let x = a.x + boxes[b] / 192.0;
            let y = a.y + boxes[b + 1] / 192.0;
            let w = (boxes[b + 2] / 192.0).abs();
            let h = (boxes[b + 3] / 192.0).abs();
            palms.push(Palm {
                x,
                y,
                w: w.max(h) * 1.6,
                h: w.max(h) * 1.6,
                score,
            })
        }
        palms.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut kept: Vec<Palm> = Vec::new();
        for p in palms {
            if kept.iter().all(|q| iou(p, *q) < 0.3) {
                kept.push(p)
            }
            if kept.len() == 2 {
                break;
            }
        }
        kept.into_iter()
            .filter_map(|p| self.landmarks(frame, p))
            .collect()
    }
    fn landmarks(&self, frame: &Rgba, p: Palm) -> Option<HandPose> {
        let rect = (p.x - p.w * 0.5, p.y - p.h * 0.5, p.w, p.h);
        let input = image_tensor(frame, 224, Some(rect));
        let out = self.landmark.run(tvec!(input.into_tvalue())).ok()?;
        let mut coords = None;
        let mut scalars = Vec::new();
        for o in &out {
            let v = o.to_plain_array_view::<f32>().ok()?;
            if v.len() >= 63 {
                coords = Some(v.iter().copied().collect::<Vec<_>>())
            } else {
                scalars.extend(v.iter().copied())
            }
        }
        let c = coords?;
        let score = scalars.iter().copied().map(sigmoid).fold(p.score, f32::max);
        if score < 0.5 {
            return None;
        }
        let mut landmarks = [Vec2::ZERO; 21];
        for (i, l) in landmarks.iter_mut().enumerate() {
            let nx = c[i * 3] / 224.0;
            let ny = c[i * 3 + 1] / 224.0;
            l.x = f64::from((rect.0 + nx * rect.2) * frame.width as f32);
            l.y = f64::from((rect.1 + ny * rect.3) * frame.height as f32);
        }
        let handedness = scalars.last().copied().map(sigmoid).unwrap_or(0.5);
        Some(HandPose {
            landmarks,
            handedness: f64::from(handedness),
            score: f64::from(score),
        })
    }
}
fn sigmoid(v: f32) -> f32 {
    1.0 / (1.0 + (-v).exp())
}
fn iou(a: Palm, b: Palm) -> f32 {
    let l = (a.x - a.w / 2.0).max(b.x - b.w / 2.0);
    let r = (a.x + a.w / 2.0).min(b.x + b.w / 2.0);
    let t = (a.y - a.h / 2.0).max(b.y - b.h / 2.0);
    let d = (a.y + a.h / 2.0).min(b.y + b.h / 2.0);
    let inter = (r - l).max(0.0) * (d - t).max(0.0);
    inter / (a.w * a.h + b.w * b.h - inter).max(1e-6)
}
fn anchors() -> Vec<Anchor> {
    let mut out = Vec::with_capacity(2016);
    for (stride, layers) in [(8, 1), (16, 3)] {
        let grid = 192 / stride;
        for _ in 0..layers {
            for y in 0..grid {
                for x in 0..grid {
                    for _ in 0..2 {
                        out.push(Anchor {
                            x: (x as f32 + 0.5) / grid as f32,
                            y: (y as f32 + 0.5) / grid as f32,
                        })
                    }
                }
            }
        }
    }
    out
}
fn image_tensor(frame: &Rgba, size: usize, rect: Option<(f32, f32, f32, f32)>) -> Tensor {
    let rect = rect.unwrap_or((0.0, 0.0, 1.0, 1.0));
    let mut data = vec![0.0f32; size * size * 3];
    for y in 0..size {
        for x in 0..size {
            let nx = rect.0 + (x as f32 + 0.5) / size as f32 * rect.2;
            let ny = rect.1 + (y as f32 + 0.5) / size as f32 * rect.3;
            let sx = (nx * frame.width as f32)
                .floor()
                .clamp(0.0, frame.width.saturating_sub(1) as f32) as u32;
            let sy = (ny * frame.height as f32)
                .floor()
                .clamp(0.0, frame.height.saturating_sub(1) as f32) as u32;
            let p = frame.get(sx, sy);
            let i = (y * size + x) * 3;
            for c in 0..3 {
                data[i + c] = f32::from(p[c]) / 127.5 - 1.0
            }
        }
    }
    Tensor::from_shape(&[1, size, size, 3], &data)
        .unwrap_or_else(|_| Tensor::zero::<f32>(&[1, size, size, 3]).unwrap())
}
