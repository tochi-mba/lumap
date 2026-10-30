//! Compare scans and move projector coordinates after a bump.
use crate::{
    correspondence::{Correspondence, ProjectorMap},
    Gray,
};
use lumap_core::geom::Vec2;
/// Drift summary in projector pixels.
#[derive(Debug, Clone, Copy, Default)]
pub struct Drift {
    pub mean: f64,
    pub max: f64,
    pub valid_fraction: f64,
}
/// Maps an old projector point through the fixed camera reference into the new scan.
pub fn realign_point(old: &ProjectorMap, new: &Correspondence, p: Vec2) -> Option<Vec2> {
    old.sample(p).and_then(|c| new.sample(c))
}
/// Measures displacement between old and new scans.
pub fn drift(old: &ProjectorMap, new: &Correspondence) -> Drift {
    let mut n = 0;
    let (mut sum, mut max) = (0.0f64, 0.0f64);
    for y in (0..old.proj_h).step_by(8) {
        for x in (0..old.proj_w).step_by(8) {
            let p = Vec2::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
            if let Some(q) = realign_point(old, new, p) {
                let d = ((q.x - p.x).powi(2) + (q.y - p.y).powi(2)).sqrt();
                sum += d;
                max = max.max(d);
                n += 1;
            }
        }
    }
    Drift {
        mean: if n == 0 { 0.0 } else { sum / f64::from(n) },
        max,
        valid_fraction: n as f64
            / ((old.proj_w.div_ceil(8) * old.proj_h.div_ceil(8)).max(1)) as f64,
    }
}
/// Brightness-invariant view difference in `0..=1`.
pub fn camera_moved(a: &Gray, b: &Gray) -> f64 {
    if a.data.len() != b.data.len() || a.data.is_empty() {
        return 1.0;
    }
    let ma = a.data.iter().map(|&v| f64::from(v)).sum::<f64>() / a.data.len() as f64;
    let mb = b.data.iter().map(|&v| f64::from(v)).sum::<f64>() / b.data.len() as f64;
    let (mut dot, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (&x, &y) in a.data.iter().zip(&b.data) {
        let (dx, dy) = (f64::from(x) - ma, f64::from(y) - mb);
        dot += dx * dy;
        aa += dx * dx;
        bb += dy * dy
    }
    if aa == 0.0 || bb == 0.0 {
        0.0
    } else {
        (1.0 - dot / (aa * bb).sqrt()).clamp(0.0, 1.0)
    }
}
