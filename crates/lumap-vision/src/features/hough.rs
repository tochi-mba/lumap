//! Circles in an edge map: a two-stage Hough transform (centres first, then the radius).

use super::shapes::fit_circle;
use crate::image::Gray;
use lumap_core::geom::Vec2;
use std::f64::consts::{PI, TAU};

/// Centre, radius and score.
type Circle = (Vec2, f64, f64);

/// Most centre candidates measured.
const MAX_CANDIDATES: usize = 48;
/// Most angular bins used to measure how much of a circle is supported.
const MAX_BINS: usize = 90;

/// Circles in an edge map (non-zero pixels are edges, e.g. from [`super::canny`]) with radius in
/// `min_r..=max_r` pixels, strongest first: `(centre, radius, score)`.
///
/// Every edge pixel estimates its normal from the edge pixels around it and votes for centres
/// along that normal at every radius in range (the gradient Hough transform, without needing the
/// image); the strongest centres are then measured against all edge pixels: the best-supported
/// radius, a least-squares refit of the centre and radius on the pixels near it, and the score,
/// the fraction of the circumference with an edge pixel within about a pixel (`0..=1`). Circles
/// scoring below `threshold` are dropped, as are repeats of a stronger circle. Radii below two
/// pixels are not searched.
pub fn hough_circles(edges: &Gray, min_r: f64, max_r: f64, threshold: f64) -> Vec<Circle> {
    let (w, h) = (edges.width as usize, edges.height as usize);
    if !edges.is_consistent() || w < 5 || h < 5 || !min_r.is_finite() || !max_r.is_finite() {
        return Vec::new();
    }
    let min_r = min_r.max(2.0);
    if max_r < min_r {
        return Vec::new();
    }
    let threshold = if threshold.is_finite() {
        threshold.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let pts: Vec<usize> = (0..w * h).filter(|&i| edges.data[i] != 0).collect();
    if pts.len() < 8 {
        return Vec::new();
    }
    let votes = vote(edges, &pts, min_r, max_r);
    let mut hist = vec![0u32; max_r.ceil() as usize + 3];
    let mut inliers = Vec::new();
    let found = peaks(&votes, w, h, threshold, min_r)
        .into_iter()
        .filter_map(|c| measure(c, &pts, w, (min_r, max_r), &mut hist, &mut inliers))
        .filter(|c| c.2 >= threshold)
        .collect();
    suppress(found)
}

/// Centre of pixel `i`.
fn centre_of(i: usize, w: usize) -> Vec2 {
    Vec2::new((i % w) as f64 + 0.5, (i / w) as f64 + 0.5)
}

/// Unit normal of the edge through pixel `(x, y)`: perpendicular to the principal axis of the
/// edge pixels in the 5×5 window around it. `None` when the window is too sparse or too blobby
/// (a junction or noise) to give a direction.
fn edge_normal(edges: &Gray, x: usize, y: usize) -> Option<Vec2> {
    let (w, h) = (edges.width as i64, edges.height as i64);
    let (mut sxx, mut sxy, mut syy, mut count) = (0.0, 0.0, 0.0, 0);
    for dy in -2i64..=2 {
        for dx in -2i64..=2 {
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            if (dx == 0 && dy == 0) || nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            if edges.data[(ny * w + nx) as usize] == 0 {
                continue;
            }
            let (fx, fy) = (dx as f64, dy as f64);
            sxx += fx * fx;
            sxy += fx * fy;
            syy += fy * fy;
            count += 1;
        }
    }
    if count < 2 {
        return None;
    }
    let mean = 0.5 * (sxx + syy);
    let spread = (0.5 * (sxx - syy)).hypot(sxy);
    if mean - spread > 0.35 * (mean + spread) {
        return None;
    }
    let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    Some(Vec2::new(-angle.sin(), angle.cos()))
}

/// Votes for centres along every edge normal, then summed over 3×3 cells.
fn vote(edges: &Gray, pts: &[usize], min_r: f64, max_r: f64) -> Vec<u32> {
    let (w, h) = (edges.width as usize, edges.height as usize);
    let mut acc = vec![0u32; w * h];
    let (r0, r1) = (min_r.floor() as i64, max_r.ceil() as i64);
    for &i in pts {
        let Some(n) = edge_normal(edges, i % w, i / w) else {
            continue;
        };
        let c = centre_of(i, w);
        for r in r0..=r1 {
            for s in [-(r as f64), r as f64] {
                let v = c + n * s;
                if v.x >= 0.0 && v.y >= 0.0 && v.x < w as f64 && v.y < h as f64 {
                    acc[v.y as usize * w + v.x as usize] += 1;
                }
            }
        }
    }
    let mut out = vec![0u32; w * h];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let mut s = 0;
            for row in [(y - 1) * w, y * w, (y + 1) * w] {
                s += acc[row + x - 1] + acc[row + x] + acc[row + x + 1];
            }
            out[y * w + x] = s;
        }
    }
    out
}

/// Local maxima of the votes with enough support, most votes first.
fn peaks(votes: &[u32], w: usize, h: usize, threshold: f64, min_r: f64) -> Vec<Vec2> {
    let min_votes = (threshold * 2.5 * min_r).max(6.0) as u32;
    let mut cand: Vec<(u32, usize)> = Vec::new();
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            let v = votes[i];
            if v < min_votes {
                continue;
            }
            let before = [i - w - 1, i - w, i - w + 1, i - 1]
                .iter()
                .all(|&j| v > votes[j]);
            let after = [i + 1, i + w - 1, i + w, i + w + 1]
                .iter()
                .all(|&j| v >= votes[j]);
            if before && after {
                cand.push((v, i));
            }
        }
    }
    cand.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    cand.truncate(MAX_CANDIDATES);
    cand.iter().map(|&(_, i)| centre_of(i, w)).collect()
}

/// The circle around a candidate centre: the radius with the most edge pixels per unit of
/// circumference, refitted twice on the pixels near it, scored by angular coverage.
fn measure(
    c: Vec2,
    pts: &[usize],
    w: usize,
    (min_r, max_r): (f64, f64),
    hist: &mut [u32],
    inliers: &mut Vec<Vec2>,
) -> Option<Circle> {
    hist.fill(0);
    let top = hist.len() - 1;
    for &i in pts {
        let d = centre_of(i, w).distance(c);
        if d >= min_r - 1.5 && d <= max_r + 1.5 {
            hist[(d.round() as usize).min(top)] += 1;
        }
    }
    let (lo, hi) = ((min_r.ceil() as usize).max(1), (max_r.floor() as usize).min(top - 1));
    let mut best = (0.0, 0usize);
    for r in lo..=hi {
        let s = f64::from(hist[r - 1] + hist[r] + hist[r + 1]) / r as f64;
        if s > best.0 {
            best = (s, r);
        }
    }
    if best.1 == 0 {
        return None;
    }
    let (mut centre, mut r) = (c, best.1 as f64);
    for tol in [2.0, 1.2] {
        inliers.clear();
        inliers.extend(
            pts.iter()
                .map(|&i| centre_of(i, w))
                .filter(|p| (p.distance(centre) - r).abs() <= tol),
        );
        (centre, r) = fit_circle(inliers)?;
    }
    if r < min_r || r > max_r {
        return None;
    }
    let bins = ((TAU * r / 2.0).round() as usize).clamp(8, MAX_BINS);
    let mut hit = [false; MAX_BINS];
    let mut covered = 0usize;
    for &i in pts {
        let d = centre_of(i, w) - centre;
        if (d.length() - r).abs() > 1.2 {
            continue;
        }
        let a = d.y.atan2(d.x) + PI;
        let bin = ((a / TAU * bins as f64) as usize).min(bins - 1);
        if !hit[bin] {
            hit[bin] = true;
            covered += 1;
        }
    }
    Some((centre, r, covered as f64 / bins as f64))
}

/// Drops circles that repeat a stronger one (close centre and radius).
fn suppress(mut found: Vec<Circle>) -> Vec<Circle> {
    found.sort_by(|a, b| {
        b.2.total_cmp(&a.2)
            .then(b.1.total_cmp(&a.1))
            .then(a.0.x.total_cmp(&b.0.x))
            .then(a.0.y.total_cmp(&b.0.y))
    });
    let mut kept: Vec<Circle> = Vec::new();
    for c in found {
        let repeat = kept.iter().any(|k| {
            let m = k.1.min(c.1);
            k.0.distance(c.0) < 0.5 * m && (k.1 - c.1).abs() < 0.3 * m
        });
        if !repeat {
            kept.push(c);
        }
    }
    kept
}
