//! Homographies fitted to many point pairs: least-squares DLT and RANSAC.
use lumap_core::geom::{Homography, Mat3, Vec2};
/// Deterministic RANSAC controls.
#[derive(Debug, Clone, Copy)]
pub struct RansacSettings {
    pub threshold: f64,
    pub iterations: u32,
    pub seed: u64,
}
impl Default for RansacSettings {
    fn default() -> Self {
        Self {
            threshold: 2.0,
            iterations: 300,
            seed: 1,
        }
    }
}
/// Fits a projective transform to at least four `(source, destination)` pairs.
pub fn fit(pairs: &[(Vec2, Vec2)]) -> Option<Homography> {
    if pairs.len() < 4 || pairs.iter().any(|(a, b)| !a.is_finite() || !b.is_finite()) {
        return None;
    }
    let mut ata = [[0.0; 8]; 8];
    let mut atb = [0.0; 8];
    for &(s, d) in pairs {
        let rows = [
            ([s.x, s.y, 1.0, 0.0, 0.0, 0.0, -d.x * s.x, -d.x * s.y], d.x),
            ([0.0, 0.0, 0.0, s.x, s.y, 1.0, -d.y * s.x, -d.y * s.y], d.y),
        ];
        for (row, b) in rows {
            for i in 0..8 {
                atb[i] += row[i] * b;
                for j in 0..8 {
                    ata[i][j] += row[i] * row[j];
                }
            }
        }
    }
    let x = solve(ata, atb)?;
    Homography::from_matrix(Mat3([x[0], x[1], x[2], x[3], x[4], x[5], x[6], x[7], 1.0])).ok()
}
fn solve(mut a: [[f64; 8]; 8], mut b: [f64; 8]) -> Option<[f64; 8]> {
    for col in 0..8 {
        let pivot = (col..8).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let d = a[col][col];
        for j in col..8 {
            a[col][j] /= d
        }
        b[col] /= d;
        for i in 0..8 {
            if i == col {
                continue;
            }
            let f = a[i][col];
            for j in col..8 {
                a[i][j] -= f * a[col][j]
            }
            b[i] -= f * b[col];
        }
    }
    Some(b)
}
/// Robustly fits a transform and returns an inlier mask.
pub fn ransac(pairs: &[(Vec2, Vec2)], s: &RansacSettings) -> Option<(Homography, Vec<bool>)> {
    if pairs.len() < 4 {
        return None;
    }
    let mut rng = s.seed.max(1);
    let mut best = Vec::new();
    for _ in 0..s.iterations.max(1) {
        let mut ids = [0usize; 4];
        for i in 0..4 {
            loop {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                let id = (rng as usize) % pairs.len();
                if !ids[..i].contains(&id) {
                    ids[i] = id;
                    break;
                }
            }
        }
        let sample = ids.map(|i| pairs[i]);
        let Some(h) = fit(&sample) else { continue };
        let mask: Vec<bool> = pairs
            .iter()
            .map(|&(a, b)| h.map(a).is_some_and(|p| p.distance(b) <= s.threshold))
            .collect();
        if mask.iter().filter(|&&v| v).count() > best.iter().filter(|&&v| v).count() {
            best = mask
        }
    }
    if best.iter().filter(|&&v| v).count() < 4 {
        return None;
    }
    let inliers: Vec<_> = pairs
        .iter()
        .zip(&best)
        .filter_map(|(&p, &yes)| yes.then_some(p))
        .collect();
    fit(&inliers).map(|h| (h, best))
}
