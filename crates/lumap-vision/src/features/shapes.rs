//! Shapes fitted to outlines and refined onto the photo's edges: quads in corner-pin order,
//! circles and straight sides.

use super::polygon::{
    boundary_distance, centroid, convex_hull, perimeter, polygon_area, simplify,
};
use super::prepared::{Prepared, CANNY_LOW};
use lumap_core::geom::Vec2;
use std::f64::consts::{FRAC_PI_2, TAU};

/// Smallest gradient across a side (Sobel units) that counts as an edge when refining.
const MIN_EDGE: f64 = 0.6 * CANNY_LOW;
/// The gradient must point within about 37° of the search direction.
const ALIGN: f64 = 0.8;
/// Search step along a normal, in pixels.
const STEP: f64 = 0.5;
/// Sides shorter than this are not refined on their own.
const MIN_SIDE: f64 = 6.0;
/// Rays cast when refining a circle.
const RAYS: usize = 64;

/// The four corners of a quadrilateral in lumap's corner-pin order: top-left, top-right,
/// bottom-right, bottom-left. The corners are sorted clockwise on screen around their centroid,
/// then rotated so that each sits as close as possible to the matching corner of the quad's
/// bounding box, which keeps the order stable for tilted and perspective quads.
pub fn order_quad(quad: [Vec2; 4]) -> [Vec2; 4] {
    let c = centroid(&quad);
    let mut q = quad;
    let angle = |p: &Vec2| (p.y - c.y).atan2(p.x - c.x);
    q.sort_by(|a, b| angle(a).total_cmp(&angle(b)));
    let (mut x0, mut y0) = (f64::INFINITY, f64::INFINITY);
    let (mut x1, mut y1) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in &q {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let target = [
        Vec2::new(x0, y0),
        Vec2::new(x1, y0),
        Vec2::new(x1, y1),
        Vec2::new(x0, y1),
    ];
    let cost = |r: usize| -> f64 {
        (0..4)
            .map(|k| q[(k + r) % 4].distance(target[k]).powi(2))
            .sum()
    };
    let best = (0..4)
        .min_by(|&a, &b| cost(a).total_cmp(&cost(b)))
        .unwrap_or(0);
    [q[best], q[(best + 1) % 4], q[(best + 2) % 4], q[(best + 3) % 4]]
}

/// Least-squares circle through `points` (the algebraic Kasa fit): centre and radius, or `None`
/// for fewer than three points, points on a line or non-finite input.
pub fn fit_circle(points: &[Vec2]) -> Option<(Vec2, f64)> {
    if points.len() < 3 {
        return None;
    }
    let m = centroid(points);
    let (mut suu, mut suv, mut svv, mut suz, mut svz, mut sz) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for p in points {
        let (u, v) = (p.x - m.x, p.y - m.y);
        let z = u * u + v * v;
        suu += u * u;
        suv += u * v;
        svv += v * v;
        suz += u * z;
        svz += v * z;
        sz += z;
    }
    let det = suu * svv - suv * suv;
    if det.is_nan() || det.abs() <= 1e-12 * (suu + svv).powi(2) {
        return None;
    }
    let d = -(svv * suz - suv * svz) / det;
    let e = -(suu * svz - suv * suz) / det;
    let f = -sz / points.len() as f64;
    let centre = m + Vec2::new(-0.5 * d, -0.5 * e);
    let r2 = 0.25 * (d * d + e * e) - f;
    (r2 > 0.0 && r2.is_finite() && centre.is_finite()).then(|| (centre, r2.sqrt()))
}

/// `count` points on a circle, starting at the top and running clockwise on screen.
pub(crate) fn circle_points(c: Vec2, r: f64, count: usize) -> Vec<Vec2> {
    (0..count)
        .map(|k| {
            let a = -FRAC_PI_2 + TAU * k as f64 / count as f64;
            c + Vec2::new(a.cos(), a.sin()) * r
        })
        .collect()
}

/// An infinite line through `point` along the unit vector `dir`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Line {
    pub point: Vec2,
    pub dir: Vec2,
}

impl Line {
    /// Total-least-squares line through the points (their principal axis).
    pub(crate) fn fit(points: &[Vec2]) -> Option<Self> {
        if points.len() < 2 {
            return None;
        }
        let c = centroid(points);
        let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
        for p in points {
            let d = *p - c;
            sxx += d.x * d.x;
            sxy += d.x * d.y;
            syy += d.y * d.y;
        }
        if sxx + syy <= 0.0 || !c.is_finite() {
            return None;
        }
        let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
        Some(Self {
            point: c,
            dir: Vec2::new(angle.cos(), angle.sin()),
        })
    }

    /// Distance from `p` to the line.
    pub(crate) fn distance(&self, p: Vec2) -> f64 {
        (p - self.point).cross(self.dir).abs()
    }

    /// The foot of the perpendicular from `p`.
    pub(crate) fn project(&self, p: Vec2) -> Vec2 {
        self.point + self.dir * (p - self.point).dot(self.dir)
    }

    /// Where two lines cross, unless they are within about 3° of parallel.
    pub(crate) fn intersect(&self, o: &Self) -> Option<Vec2> {
        let den = self.dir.cross(o.dir);
        if den.abs() < 0.05 {
            return None;
        }
        let t = (o.point - self.point).cross(o.dir) / den;
        Some(self.point + self.dir * t)
    }
}

/// Offset along the unit vector `n` of the strongest edge crossing the segment `p ± n·range`
/// (at most 8 px), to sub-pixel precision: the peak of the gradient's component along `n`,
/// counted only where the gradient points along `n`. `None` when no edge is strong enough or the
/// strongest sits at the end of the range (the real edge is further away).
pub(crate) fn edge_offset(prep: &Prepared, p: Vec2, n: Vec2, range: f64) -> Option<f64> {
    let steps = ((range / STEP).round() as usize).clamp(1, 16);
    let count = 2 * steps + 1;
    let mut vals = [0.0f64; 33];
    for (k, v) in vals.iter_mut().enumerate().take(count) {
        let o = (k as f64 - steps as f64) * STEP;
        let (gx, gy) = prep.gradient_at(p + n * o);
        let along = (gx * n.x + gy * n.y).abs();
        let full = gx.hypot(gy);
        *v = if full > 0.0 && along >= ALIGN * full {
            along
        } else {
            0.0
        };
    }
    let mut best = 0;
    for (k, v) in vals.iter().enumerate().take(count).skip(1) {
        if *v > vals[best] {
            best = k;
        }
    }
    if vals[best] < MIN_EDGE || best == 0 || best == count - 1 {
        return None;
    }
    let (a, b, c) = (vals[best - 1], vals[best], vals[best + 1]);
    let den = a - 2.0 * b + c;
    let delta = if den < -1e-9 {
        (0.5 * (a - c) / den).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    Some((best as f64 - steps as f64 + delta) * STEP)
}

/// The edge line a side `a`-`b` lies near: edge points found along the side's normal over its
/// middle 60 % (away from the corners), fitted by total least squares with one round of outlier
/// rejection. `None` for short or unsupported sides, or when the edge found is not roughly
/// parallel to the side.
fn fit_side(prep: &Prepared, a: Vec2, b: Vec2, range: f64) -> Option<Line> {
    let len = a.distance(b);
    if len < MIN_SIDE || !len.is_finite() {
        return None;
    }
    let d = (b - a) * (1.0 / len);
    let n = Vec2::new(-d.y, d.x);
    let samples = ((len * 0.6).ceil() as usize).max(4);
    let mut pts = Vec::with_capacity(samples);
    for k in 0..samples {
        let p = a.lerp(b, 0.2 + 0.6 * (k as f64 + 0.5) / samples as f64);
        if let Some(o) = edge_offset(prep, p, n, range) {
            pts.push(p + n * o);
        }
    }
    if pts.len() < 3 || (pts.len() as f64) < 0.4 * samples as f64 {
        return None;
    }
    let line = Line::fit(&pts)?;
    pts.retain(|&p| line.distance(p) <= 1.0);
    if pts.len() < 3 {
        return None;
    }
    let line = Line::fit(&pts)?;
    (line.dir.cross(d).abs() <= 0.34).then_some(line)
}

/// Moves a closed polygon onto the photo's edges: every side long enough is replaced by the edge
/// line it lies near (see [`fit_side`]) and each vertex becomes the crossing of its two side
/// lines; a vertex with one refined side slides onto that line, one with none is pushed along
/// its normal onto the nearest edge. Anything that finds no edge stays where it was.
pub(crate) fn refine_polygon(prep: &Prepared, poly: &[Vec2], range: f64) -> Vec<Vec2> {
    let n = poly.len();
    if n < 3 || prep.w == 0 {
        return poly.to_vec();
    }
    let lines: Vec<Option<Line>> = (0..n)
        .map(|i| fit_side(prep, poly[i], poly[(i + 1) % n], range))
        .collect();
    (0..n)
        .map(|i| {
            let v = poly[i];
            let moved = match (lines[(i + n - 1) % n], lines[i]) {
                (Some(a), Some(b)) => a
                    .intersect(&b)
                    .filter(|p| p.distance(v) <= 2.5 * range)
                    .or_else(|| Some(a.project(v))),
                (Some(a), None) | (None, Some(a)) => Some(a.project(v)),
                (None, None) => push_vertex(prep, poly[(i + n - 1) % n], v, poly[(i + 1) % n], range),
            };
            moved.unwrap_or(v)
        })
        .collect()
}

/// A vertex pushed along the normal of the chord between its neighbours onto the nearest edge.
fn push_vertex(prep: &Prepared, prev: Vec2, v: Vec2, next: Vec2, range: f64) -> Option<Vec2> {
    let d = next - prev;
    let len = d.length();
    if len <= 0.0 || !len.is_finite() {
        return None;
    }
    let n = Vec2::new(d.y / len, -d.x / len);
    edge_offset(prep, v, n, range).map(|o| v + n * o)
}

/// Fraction of points, sampled every two pixels along a closed outline, that sit within two
/// pixels of an edge running along the outline.
pub(crate) fn edge_support(prep: &Prepared, poly: &[Vec2]) -> f64 {
    let n = poly.len();
    let (mut hit, mut total) = (0usize, 0usize);
    if n < 2 || prep.w == 0 {
        return 0.0;
    }
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let len = a.distance(b);
        if len <= 0.0 || !len.is_finite() {
            continue;
        }
        let d = (b - a) * (1.0 / len);
        let nrm = Vec2::new(-d.y, d.x);
        let count = (len / 2.0).ceil().max(1.0) as usize;
        for k in 0..count {
            let p = a.lerp(b, (k as f64 + 0.5) / count as f64);
            total += 1;
            if edge_offset(prep, p, nrm, 2.0).is_some() {
                hit += 1;
            }
        }
    }
    if total == 0 {
        0.0
    } else {
        hit as f64 / total as f64
    }
}

/// The best quadrilateral for an outline and how well it fits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct QuadFit {
    /// Corners in corner-pin order.
    pub quad: [Vec2; 4],
    /// Outline area over quad area (1 for a perfect quad, about 1.57 for a circle).
    pub ratio: f64,
    /// Largest distance from an outline point to the quad's boundary.
    pub deviation: f64,
}

/// The largest quadrilateral spanned by the outline's dominant turning points: the convex hull,
/// simplified to at most ten vertices, and the four of them enclosing the most area.
pub(crate) fn quad_from_outline(outline: &[Vec2]) -> Option<QuadFit> {
    let hull = convex_hull(outline);
    if hull.len() < 4 {
        return None;
    }
    let mut eps = (0.015 * perimeter(&hull)).max(1.0);
    let mut simp = simplify(&hull, eps, true);
    while simp.len() > 10 {
        eps *= 1.5;
        simp = simplify(&hull, eps, true);
    }
    let n = simp.len();
    let mut best = (0.0, [0usize; 4]);
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                for l in k + 1..n {
                    let a = polygon_area(&[simp[i], simp[j], simp[k], simp[l]]);
                    if a > best.0 {
                        best = (a, [i, j, k, l]);
                    }
                }
            }
        }
    }
    if best.0 < 1.0 {
        return None;
    }
    let quad = best.1.map(|i| simp[i]);
    let ratio = polygon_area(outline) / best.0;
    let deviation = outline
        .iter()
        .map(|&p| boundary_distance(p, &quad))
        .fold(0.0, f64::max);
    Some(QuadFit {
        quad: order_quad(quad),
        ratio,
        deviation,
    })
}

/// A fit close enough to call the outline a quadrilateral: the areas agree, no outline point
/// strays far from the quad, and every corner is between about 26° and 154°.
pub(crate) fn is_quad_like(fit: &QuadFit) -> bool {
    let size = polygon_area(&fit.quad).sqrt();
    let angles_ok = (0..4).all(|i| {
        let v = fit.quad[i];
        let (a, b) = (fit.quad[(i + 3) % 4] - v, fit.quad[(i + 1) % 4] - v);
        let (la, lb) = (a.length(), b.length());
        la >= 3.0 && lb >= 3.0 && (a.dot(b) / (la * lb)).abs() <= 0.9
    });
    (0.9..=1.08).contains(&fit.ratio) && fit.deviation <= (0.06 * size).max(2.5) && angles_ok
}

/// A circle refined onto the photo's edges along 64 rays from `centre`: the new centre and
/// radius, the fraction of rays that met the edge, and the RMS radial residual of those points.
pub(crate) fn refine_circle(
    prep: &Prepared,
    centre: Vec2,
    r: f64,
    range: f64,
) -> Option<(Vec2, f64, f64, f64)> {
    let mut pts = Vec::with_capacity(RAYS);
    for k in 0..RAYS {
        let a = TAU * k as f64 / RAYS as f64;
        let n = Vec2::new(a.cos(), a.sin());
        let p = centre + n * r;
        if let Some(o) = edge_offset(prep, p, n, range) {
            pts.push(p + n * o);
        }
    }
    if pts.len() < 12 {
        return None;
    }
    let (c, rr) = fit_circle(&pts)?;
    pts.retain(|p| (p.distance(c) - rr).abs() <= 1.0);
    if pts.len() < 12 {
        return None;
    }
    let (c, rr) = fit_circle(&pts)?;
    let sq: f64 = pts.iter().map(|p| (p.distance(c) - rr).powi(2)).sum();
    let rms = (sq / pts.len() as f64).sqrt();
    Some((c, rr, pts.len() as f64 / RAYS as f64, rms))
}
