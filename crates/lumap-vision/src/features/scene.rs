//! Synthetic projector's-eye photos for the tests: a wall lit unevenly by the projector, a box
//! seen at a slight angle (three faces of different shades), a picture frame (dark border, light
//! inside, a small dark dot on the picture), a round plate, and mild sensor noise. Shapes are
//! anti-aliased by supersampling, so their true corners and radii are exact.

use crate::image::Rgba;
use lumap_core::geom::Vec2;
use std::sync::OnceLock;

/// Scene width in pixels.
pub(crate) const W: u32 = 320;
/// Scene height in pixels.
pub(crate) const H: u32 = 180;

/// A shape painted into a synthetic photo.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Shape {
    /// A convex quad, clockwise on screen.
    Quad([Vec2; 4]),
    /// Centre and radius.
    Disc(Vec2, f64),
}

impl Shape {
    /// Signed distance to the boundary, negative inside (exact inside, close enough outside).
    fn signed_distance(&self, p: Vec2) -> f64 {
        match self {
            Shape::Quad(q) => (0..4)
                .map(|i| {
                    let (a, b) = (q[i], q[(i + 1) % 4]);
                    let d = b - a;
                    -d.cross(p - a) / d.length()
                })
                .fold(f64::NEG_INFINITY, f64::max),
            Shape::Disc(c, r) => p.distance(*c) - r,
        }
    }

    /// Fraction of pixel `(x, y)` the shape covers (8×8 supersampling near the boundary).
    fn coverage(&self, x: u32, y: u32) -> f64 {
        let (fx, fy) = (f64::from(x), f64::from(y));
        let d = self.signed_distance(Vec2::new(fx + 0.5, fy + 0.5));
        if d <= -0.8 {
            return 1.0;
        }
        if d >= 0.8 {
            return 0.0;
        }
        let mut inside = 0u32;
        for sy in 0..8 {
            for sx in 0..8 {
                let p = Vec2::new(
                    fx + (f64::from(sx) + 0.5) / 8.0,
                    fy + (f64::from(sy) + 0.5) / 8.0,
                );
                if self.signed_distance(p) < 0.0 {
                    inside += 1;
                }
            }
        }
        f64::from(inside) / 64.0
    }
}

/// Deterministic per-pixel noise in `-3..=3` grey levels.
pub(crate) fn noise(x: u32, y: u32) -> f64 {
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ 0x27D4_EB2F;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    f64::from(h % 7) - 3.0
}

/// The wall under the projector's white light: brightest in the middle, about 15 levels darker
/// in the corners, slightly warm.
pub(crate) fn wall(p: Vec2) -> [f64; 3] {
    let dx = (p.x - 160.0) / 160.0;
    let dy = (p.y - 90.0) / 90.0;
    let v = 205.0 - 15.0 * (dx * dx + dy * dy);
    [v, v, v - 6.0]
}

/// Paints `shapes` in order over `base`, then adds the noise.
pub(crate) fn paint(
    w: u32,
    h: u32,
    base: impl Fn(Vec2) -> [f64; 3],
    shapes: &[(Shape, [f64; 3])],
) -> Rgba {
    Rgba::from_fn(w, h, |x, y| {
        let mut col = base(Vec2::new(f64::from(x) + 0.5, f64::from(y) + 0.5));
        for (s, c) in shapes {
            let a = s.coverage(x, y);
            if a > 0.0 {
                for (v, t) in col.iter_mut().zip(c) {
                    *v += (t - *v) * a;
                }
            }
        }
        let n = noise(x, y);
        let q = |v: f64| (v + n).round().clamp(0.0, 255.0) as u8;
        [q(col[0]), q(col[1]), q(col[2]), 255]
    })
}

/// The test room and the truth about it.
pub(crate) struct Scene {
    pub photo: Rgba,
    pub top: [Vec2; 4],
    pub left: [Vec2; 4],
    pub right: [Vec2; 4],
    pub frame_outer: [Vec2; 4],
    pub frame_inner: [Vec2; 4],
    pub disc: (Vec2, f64),
}

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// The shared test scene, painted once per test binary.
pub(crate) fn scene() -> &'static Scene {
    static SCENE: OnceLock<Scene> = OnceLock::new();
    SCENE.get_or_init(|| {
        let (t0, t1, t2, t3) = (v(50.0, 60.0), v(100.0, 50.0), v(130.0, 68.0), v(80.0, 79.0));
        let (b0, b2, b3) = (v(51.0, 110.0), v(129.0, 115.0), v(80.0, 128.0));
        let top = [t0, t1, t2, t3];
        let left = [t0, t3, b3, b0];
        let right = [t3, t2, b2, b3];
        let frame_outer = [v(200.0, 40.0), v(290.0, 44.0), v(288.0, 108.0), v(202.0, 112.0)];
        let frame_inner = [v(211.0, 51.0), v(279.0, 54.0), v(277.0, 97.0), v(213.0, 101.0)];
        let disc = (v(165.0, 140.0), 22.0);
        let photo = paint(
            W,
            H,
            wall,
            &[
                (Shape::Quad(top), [236.0, 233.0, 226.0]),
                (Shape::Quad(left), [152.0, 148.0, 142.0]),
                (Shape::Quad(right), [104.0, 101.0, 97.0]),
                (Shape::Quad(frame_outer), [70.0, 52.0, 40.0]),
                (Shape::Quad(frame_inner), [228.0, 224.0, 212.0]),
                (Shape::Disc(v(262.0, 66.0), 3.5), [60.0, 60.0, 70.0]),
                (Shape::Disc(disc.0, disc.1), [84.0, 112.0, 160.0]),
            ],
        );
        Scene {
            photo,
            top,
            left,
            right,
            frame_outer,
            frame_inner,
            disc,
        }
    })
}

/// The same wall with nothing on it.
pub(crate) fn plain_wall() -> Rgba {
    paint(W, H, wall, &[])
}

/// Largest distance between matching corners.
pub(crate) fn corner_error(a: &[Vec2], b: &[Vec2]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(p, q)| p.distance(*q))
        .fold(0.0, f64::max)
}
