//! Shi-Tomasi corners with sub-pixel refinement.

use super::filters::sobel;
use crate::image::Gray;
use lumap_core::geom::Vec2;

/// Shi-Tomasi corners ("good features to track") of `g`, strongest first, at most `max`.
///
/// The corner response is the smaller eigenvalue of the gradient's structure tensor over a 3×3
/// window; corners are its local maxima above `quality` times the strongest response (`0..=1`),
/// at least `min_distance` pixels apart (a stronger corner wins), then refined to sub-pixel
/// precision where the edges through the corner meet (the gradient at every nearby pixel is
/// perpendicular to the line from that pixel to the corner). Positions are continuous pixel
/// coordinates: the centre of pixel `(x, y)` is `(x + 0.5, y + 0.5)`. Pixels within two of the
/// border are never corners. Blur noisy images first.
pub fn corners(g: &Gray, max: usize, quality: f64, min_distance: f64) -> Vec<Vec2> {
    let w = g.width as usize;
    let h = g.height as usize;
    if max == 0 || w < 5 || h < 5 || !g.is_consistent() {
        return Vec::new();
    }
    let (gx, gy) = sobel(g);
    let response = min_eigenvalues(&gx, &gy, w, h);
    let best = response.iter().copied().fold(0.0f32, f32::max);
    if best <= 0.0 {
        return Vec::new();
    }
    let quality = if quality.is_finite() {
        quality.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let floor = (f64::from(best) * quality) as f32;
    let mut candidates = Vec::new();
    for y in 2..h - 2 {
        for x in 2..w - 2 {
            let i = y * w + x;
            let r = response[i];
            if r > 0.0 && r >= floor && is_peak(&response, w, i) {
                candidates.push((r, i));
            }
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let min_d = if min_distance.is_finite() {
        min_distance.max(0.0)
    } else {
        0.0
    };
    let picked = spread(&candidates, w, h, max, min_d);
    let half = ((min_d / 2.0).round() as i64).clamp(2, 5);
    picked
        .into_iter()
        .map(|p| refine(&gx, &gy, w, h, p, half))
        .collect()
}

/// The smaller eigenvalue of the 3×3 structure tensor at every pixel (zero on the border).
fn min_eigenvalues(gx: &[f32], gy: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let (mut a, mut b, mut c) = (0.0f64, 0.0f64, 0.0f64);
            for row in [(y - 1) * w, y * w, (y + 1) * w] {
                for i in row + x - 1..=row + x + 1 {
                    let (u, v) = (f64::from(gx[i]), f64::from(gy[i]));
                    a += u * u;
                    b += u * v;
                    c += v * v;
                }
            }
            let half_diff = 0.5 * (a - c);
            out[y * w + x] = (0.5 * (a + c) - half_diff.hypot(b)) as f32;
        }
    }
    out
}

/// Strictly above the neighbours before it in raster order and not below those after it, so a
/// plateau yields exactly one peak.
fn is_peak(r: &[f32], w: usize, i: usize) -> bool {
    let v = r[i];
    [i - w - 1, i - w, i - w + 1, i - 1]
        .iter()
        .all(|&j| v > r[j])
        && [i + 1, i + w - 1, i + w, i + w + 1]
            .iter()
            .all(|&j| v >= r[j])
}

/// Greedy pick of the strongest candidates at least `min_d` apart, bucketed in a grid of
/// `min_d`-sized cells so only the 3×3 neighbouring cells need checking.
fn spread(candidates: &[(f32, usize)], w: usize, h: usize, max: usize, min_d: f64) -> Vec<Vec2> {
    let cell = min_d.max(1.0);
    let cols = ((w as f64 / cell).ceil() as usize).max(1);
    let rows = ((h as f64 / cell).ceil() as usize).max(1);
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); if min_d > 0.0 { cols * rows } else { 0 }];
    let mut out: Vec<Vec2> = Vec::new();
    for &(_, i) in candidates {
        let p = Vec2::new((i % w) as f64 + 0.5, (i / w) as f64 + 0.5);
        if min_d > 0.0 {
            let cx = ((p.x / cell) as usize).min(cols - 1);
            let cy = ((p.y / cell) as usize).min(rows - 1);
            let near = (cy.saturating_sub(1)..=(cy + 1).min(rows - 1)).any(|gy| {
                (cx.saturating_sub(1)..=(cx + 1).min(cols - 1)).any(|gx| {
                    grid[gy * cols + gx]
                        .iter()
                        .any(|&k| out[k as usize].distance(p) < min_d)
                })
            });
            if near {
                continue;
            }
            grid[cy * cols + cx].push(out.len() as u32);
        }
        out.push(p);
        if out.len() >= max {
            break;
        }
    }
    out
}

/// Sub-pixel corner position (as `cornerSubPix`): the point `q` minimising
/// `Σ w (g · (q - p))²` over the pixels `p` of a Gaussian-weighted window, iterated until it
/// settles. Falls back to `start` when the window holds no corner or the estimate runs away.
fn refine(gx: &[f32], gy: &[f32], w: usize, h: usize, start: Vec2, half: i64) -> Vec2 {
    let two_sigma2 = 2.0 * (0.75 * half as f64).powi(2);
    let mut q = start;
    for _ in 0..16 {
        let (cx, cy) = (q.x.floor() as i64, q.y.floor() as i64);
        let (mut a, mut b, mut c, mut rx, mut ry) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for y in (cy - half).max(0)..=(cy + half).min(h as i64 - 1) {
            for x in (cx - half).max(0)..=(cx + half).min(w as i64 - 1) {
                let i = y as usize * w + x as usize;
                let (u, v) = (f64::from(gx[i]), f64::from(gy[i]));
                let p = Vec2::new(x as f64 + 0.5, y as f64 + 0.5);
                let d = p - q;
                let wt = (-d.dot(d) / two_sigma2).exp();
                let (uu, uv, vv) = (u * u * wt, u * v * wt, v * v * wt);
                a += uu;
                b += uv;
                c += vv;
                rx += uu * p.x + uv * p.y;
                ry += uv * p.x + vv * p.y;
            }
        }
        let det = a * c - b * b;
        if det <= 1e-6 * (a + c) * (a + c) {
            break;
        }
        let next = Vec2::new((c * rx - b * ry) / det, (a * ry - b * rx) / det);
        let moved = next.distance(q);
        q = next;
        if !q.is_finite() || moved < 0.005 {
            break;
        }
    }
    if q.is_finite() && q.distance(start) <= half as f64 {
        q
    } else {
        start
    }
}
