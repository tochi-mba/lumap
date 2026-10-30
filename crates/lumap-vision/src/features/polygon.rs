//! Polygon simplification, convex hulls, areas and small polygon helpers.

use lumap_core::geom::Vec2;

/// Douglas-Peucker simplification: keeps the fewest points of `points` such that every dropped
/// point lies within `epsilon` pixels of the kept polyline.
///
/// With `closed` the points are a ring: the result is anchored at the point farthest from the
/// centroid and the point farthest from that, so it does not depend on where the ring starts,
/// and the last point is not repeated. Kept points stay in their original order. Fewer than
/// three points come back unchanged; a negative or non-finite `epsilon` counts as zero.
pub fn simplify(points: &[Vec2], epsilon: f64, closed: bool) -> Vec<Vec2> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }
    let eps = if epsilon.is_finite() {
        epsilon.max(0.0)
    } else {
        0.0
    };
    let mut keep = vec![false; n];
    if closed {
        let c = centroid(points);
        let a = farthest(points, c);
        let b = farthest(points, points[a]);
        if points[a].distance(points[b]) <= 0.0 {
            return vec![points[a]];
        }
        keep[a] = true;
        keep[b] = true;
        reduce_chain(points, a, b, eps, &mut keep);
        reduce_chain(points, b, a, eps, &mut keep);
    } else {
        keep[0] = true;
        keep[n - 1] = true;
        reduce_chain(points, 0, n - 1, eps, &mut keep);
    }
    points
        .iter()
        .zip(&keep)
        .filter(|(_, &k)| k)
        .map(|(p, _)| *p)
        .collect()
}

/// Index of the point farthest from `from` (the first one on ties).
fn farthest(points: &[Vec2], from: Vec2) -> usize {
    let mut best = (0, -1.0);
    for (i, p) in points.iter().enumerate() {
        let d = p.distance(from);
        if d > best.1 {
            best = (i, d);
        }
    }
    best.0
}

/// Marks the points to keep on the chain from index `from` forward (wrapping) to `to`.
fn reduce_chain(points: &[Vec2], from: usize, to: usize, eps: f64, keep: &mut [bool]) {
    let n = points.len();
    let len = (to + n - from) % n;
    let mut stack = vec![(0usize, len)];
    while let Some((s, e)) = stack.pop() {
        if e <= s + 1 {
            continue;
        }
        let (a, b) = (points[(from + s) % n], points[(from + e) % n]);
        let mut best = (s, -1.0);
        for k in s + 1..e {
            let d = segment_distance(points[(from + k) % n], a, b);
            if d > best.1 {
                best = (k, d);
            }
        }
        if best.1 > eps {
            keep[(from + best.0) % n] = true;
            stack.push((s, best.0));
            stack.push((best.0, e));
        }
    }
}

/// Distance from `p` to the segment `a`-`b`.
pub(crate) fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    if len2 <= 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Convex hull (Andrew's monotone chain) of the finite points, without collinear points, starting
/// at the leftmost point and running clockwise on screen (a positive shoelace sum with y pointing
/// down, like [`super::contours`]). Fewer than three distinct points come back sorted.
pub fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut pts: Vec<Vec2> = points.iter().copied().filter(|p| p.is_finite()).collect();
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup_by(|a, b| a.x.total_cmp(&b.x).is_eq() && a.y.total_cmp(&b.y).is_eq());
    if pts.len() < 3 {
        return pts;
    }
    let turn = |o: Vec2, a: Vec2, b: Vec2| (a - o).cross(b - o);
    let mut hull: Vec<Vec2> = Vec::with_capacity(pts.len() + 1);
    for &p in &pts {
        while hull.len() >= 2 && turn(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }
    let lower = hull.len() + 1;
    for &p in pts.iter().rev().skip(1) {
        while hull.len() >= lower && turn(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }
    hull.pop();
    hull
}

/// Twice the signed area (shoelace sum); positive when clockwise on screen (y down).
pub(crate) fn signed_area2(points: &[Vec2]) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    (0..n)
        .map(|i| points[i].cross(points[(i + 1) % n]))
        .sum::<f64>()
}

/// Area enclosed by a closed polygon (shoelace formula), whatever its orientation; zero for
/// fewer than three points.
pub fn polygon_area(points: &[Vec2]) -> f64 {
    0.5 * signed_area2(points).abs()
}

/// Perimeter of a closed polygon.
pub(crate) fn perimeter(points: &[Vec2]) -> f64 {
    let n = points.len();
    if n < 2 {
        return 0.0;
    }
    (0..n)
        .map(|i| points[i].distance(points[(i + 1) % n]))
        .sum()
}

/// Mean of the points (the origin for none).
pub(crate) fn centroid(points: &[Vec2]) -> Vec2 {
    if points.is_empty() {
        return Vec2::ZERO;
    }
    let sum = points.iter().fold(Vec2::ZERO, |acc, p| acc + *p);
    sum * (1.0 / points.len() as f64)
}

/// Even-odd point-in-polygon test.
pub(crate) fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if p.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// Distance from `p` to the boundary of a closed polygon.
pub(crate) fn boundary_distance(p: Vec2, poly: &[Vec2]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| segment_distance(p, poly[i], poly[(i + 1) % n]))
        .fold(f64::INFINITY, f64::min)
}
