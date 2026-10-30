//! Painted strokes and the brushes that draw them.
//!
//! A [`Canvas`] collects strokes from a pen that goes down and up (a tracked light, a pinching
//! hand) and renders them with a [`Brush`] into projector-resolution frames: anti-aliased lines
//! with round caps and joins, faded by age when asked.

pub(crate) mod light;
#[cfg(test)]
mod tests;

use crate::image::Rgba;
use crate::physics::rng::hash01;
use light::Light;
use lumap_core::geom::Vec2;
use lumap_core::project::Brush;
use std::f64::consts::TAU;
use std::sync::atomic::{AtomicU32, Ordering};

/// Closest spacing kept between stroke points, in pixels; nearer samples are dropped.
const MIN_STEP: f64 = 1.5;
/// A point lying closer than this to the line through its neighbours is merged away.
const STRAIGHT: f64 = 0.03;
/// Longest straight run merged into one segment, in pixels.
const MAX_RUN: f64 = 24.0;
/// Points one stroke holds before it carries on as a new stroke.
const MAX_POINTS: usize = 4096;
/// Strokes kept; the oldest go first.
const MAX_STROKES: usize = 512;
/// A jump longer than this fraction of the frame diagonal starts a new stroke.
const JUMP: f64 = 0.25;

/// Strokes drawn with a pen over a projector-sized frame.
#[derive(Debug)]
pub struct Canvas {
    width: u32,
    height: u32,
    strokes: Vec<Stroke>,
    down: bool,
    next_id: u64,
    /// The fade time (`f32` bits) the last render used, so `pen` can drop faded strokes.
    fade: AtomicU32,
}

impl Clone for Canvas {
    fn clone(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            strokes: self.strokes.clone(),
            down: self.down,
            next_id: self.next_id,
            fade: AtomicU32::new(self.fade.load(Ordering::Relaxed)),
        }
    }
}

#[derive(Debug, Clone)]
struct Stroke {
    id: u64,
    points: Vec<Point>,
    /// When the pen last touched it, in seconds.
    last_t: f64,
}

#[derive(Debug, Clone, Copy)]
struct Point {
    pos: Vec2,
    /// Distance along the stroke from its first point.
    along: f64,
}

/// What happened to a point offered to a stroke.
enum Extend {
    Taken,
    TooFar,
    Full(Vec2),
}

impl Stroke {
    fn extend(&mut self, p: Vec2, t: f64, jump: f64) -> Extend {
        let Some(&last) = self.points.last() else {
            return Extend::TooFar;
        };
        let d = last.pos.distance(p);
        if d > jump {
            return Extend::TooFar;
        }
        self.last_t = t;
        if d < MIN_STEP {
            return Extend::Taken;
        }
        if self.points.len() >= MAX_POINTS {
            return Extend::Full(last.pos);
        }
        let n = self.points.len();
        if n >= 2 {
            let a = self.points[n - 2];
            if a.pos.distance(p) <= MAX_RUN && between(a.pos, last.pos, p) {
                self.points[n - 1] = Point {
                    pos: p,
                    along: a.along + a.pos.distance(p),
                };
                return Extend::Taken;
            }
        }
        self.points.push(Point {
            pos: p,
            along: last.along + d,
        });
        Extend::Taken
    }

    fn length(&self) -> f64 {
        self.points.last().map_or(0.0, |p| p.along)
    }
}

/// Whether `b` lies between `a` and `p` and within [`STRAIGHT`] of the line through them.
fn between(a: Vec2, b: Vec2, p: Vec2) -> bool {
    let ap = p - a;
    let len = ap.length();
    if len < 1e-9 {
        return false;
    }
    let ab = b - a;
    let along = ab.dot(ap) / len;
    along > 0.0 && along < len && (ab.cross(ap) / len).abs() < STRAIGHT
}

impl Canvas {
    /// An empty canvas for frames of `width` x `height` projector pixels.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            strokes: Vec::new(),
            down: false,
            next_id: 0,
            fade: AtomicU32::new(0.0_f32.to_bits()),
        }
    }

    /// Moves the pen at time `t` (seconds): `Some` is the pen down at a projector pixel, `None`
    /// lifts it. Consecutive `Some` calls extend the current stroke; samples closer than 1.5 px
    /// to the last point are dropped and straight runs merge into one segment, and a jump across
    /// more than a quarter of the frame starts a new stroke. Strokes the last render faded out
    /// completely are dropped here.
    pub fn pen(&mut self, pos: Option<Vec2>, t: f64) {
        self.drop_faded(t);
        let Some(p) = pos.filter(|p| p.is_finite()) else {
            self.down = false;
            return;
        };
        let jump = JUMP * f64::from(self.width).hypot(f64::from(self.height)).max(8.0);
        if self.down {
            if let Some(stroke) = self.strokes.last_mut() {
                match stroke.extend(p, t, jump) {
                    Extend::Taken => return,
                    Extend::TooFar => {}
                    Extend::Full(last) => {
                        self.start(&[last, p], t);
                        return;
                    }
                }
            }
        }
        self.start(&[p], t);
    }

    fn start(&mut self, points: &[Vec2], t: f64) {
        if self.strokes.len() >= MAX_STROKES {
            self.strokes.remove(0);
        }
        let mut stroke = Stroke {
            id: self.next_id,
            points: Vec::with_capacity(64),
            last_t: t,
        };
        let mut along = 0.0;
        let mut prev: Option<Vec2> = None;
        for &p in points {
            along += prev.map_or(0.0, |q| q.distance(p));
            stroke.points.push(Point { pos: p, along });
            prev = Some(p);
        }
        self.strokes.push(stroke);
        self.next_id += 1;
        self.down = true;
    }

    fn drop_faded(&mut self, t: f64) {
        let fade = f64::from(f32::from_bits(self.fade.load(Ordering::Relaxed)));
        if fade > 0.0 {
            self.strokes.retain(|s| t - s.last_t < fade);
            if self.strokes.is_empty() {
                self.down = false;
            }
        }
    }

    /// Removes every stroke and lifts the pen.
    pub fn clear(&mut self) {
        self.strokes.clear();
        self.down = false;
    }

    /// How many strokes the canvas holds.
    pub fn strokes(&self) -> usize {
        self.strokes.len()
    }

    /// Draws every stroke into `out` (resized to the canvas and cleared, its buffer reused) at
    /// time `t`. `brush` picks the look, `color` is a straight RGBA tint in `0..=1` (the rainbow
    /// brush uses only its alpha), `size` is the stroke width in projector pixels, and
    /// `fade_seconds > 0` fades each stroke out over that long after the pen last touched it
    /// (`0` keeps strokes until cleared).
    pub fn render(
        &self,
        out: &mut Rgba,
        brush: Brush,
        color: [f32; 4],
        size: f32,
        fade_seconds: f32,
        t: f64,
    ) {
        let fade = if fade_seconds.is_finite() {
            fade_seconds.max(0.0)
        } else {
            0.0
        };
        self.fade.store(fade.to_bits(), Ordering::Relaxed);
        light::begin(out, self.width, self.height);
        let (rgb, alpha) = light::split(color);
        let size = if size.is_finite() {
            f64::from(size).max(0.5)
        } else {
            1.0
        };
        for stroke in &self.strokes {
            let left = if fade > 0.0 {
                (1.0 - (t - stroke.last_t) / f64::from(fade)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            if left <= 0.0 {
                continue;
            }
            let a = alpha * left as f32;
            match brush {
                Brush::Neon => neon(out, stroke, rgb, a, size),
                Brush::Ink => ink(out, stroke, rgb, a, size),
                Brush::Sparkle => sparkle(out, stroke, rgb, a, size, t),
                Brush::Rainbow => rainbow(out, stroke, a, size, t),
            }
        }
        light::finish(out);
    }
}

/// Runs `shade(distance, along)` over every pixel within `reach` of the stroke's centre line.
fn trace(out: &mut Rgba, s: &Stroke, reach: f64, mut shade: impl FnMut(f64, f64) -> Light) {
    match s.points.as_slice() {
        [] => {}
        [only] => light::radial(out, only.pos, reach, |d, _| shade(d, 0.0)),
        points => {
            for (i, w) in points.windows(2).enumerate() {
                let (a, b) = (w[0], w[1]);
                let end = i + 2 == points.len();
                light::segment(out, a.pos, b.pos, reach, end, |d, u| {
                    shade(d, a.along + (b.along - a.along) * u)
                });
            }
        }
    }
}

/// A glowing tube: a white-hot core, a coloured body and a wide soft halo.
fn neon(out: &mut Rgba, s: &Stroke, rgb: [f32; 3], alpha: f32, size: f64) {
    let half = size * 0.5;
    trace(out, s, size * 2.4, |d, _| {
        let body = light::disc(d, half * 0.8);
        let halo = light::glow(d, size * 1.1) * 0.55;
        let hot = (1.0 - d / (half * 0.55)).clamp(0.0, 1.0) as f32 * 0.9;
        light::tint(rgb, body.max(halo) * alpha, hot * body)
    });
}

/// A solid line with a soft anti-aliased edge.
fn ink(out: &mut Rgba, s: &Stroke, rgb: [f32; 3], alpha: f32, size: f64) {
    let half = size * 0.5;
    let soft = (size * 0.08).max(0.75);
    trace(out, s, half + soft, |d, _| {
        let cover = light::smoothstep(half + soft, half - soft, d) as f32;
        light::tint(rgb, cover * alpha, 0.0)
    });
}

/// A line whose hue runs along its length and drifts with time.
fn rainbow(out: &mut Rgba, s: &Stroke, alpha: f32, size: f64, t: f64) {
    let half = size * 0.5;
    let soft = (size * 0.08).max(0.75);
    let period = (size * 10.0).max(80.0);
    trace(out, s, size * 1.8, |d, along| {
        let rgb = light::hsv(along / period - t * 0.35, 0.85, 1.0);
        let cover = light::smoothstep(half + soft, half - soft, d) as f32;
        let halo = light::glow(d, size * 0.9) * 0.35;
        light::tint(rgb, cover.max(halo) * alpha, 0.2 * cover)
    });
}

/// Twinkling dots scattered along the stroke; each dot's place, size and rhythm come from a
/// hash of the stroke and the dot, so a frame depends only on `t`.
fn sparkle(out: &mut Rgba, s: &Stroke, rgb: [f32; 3], alpha: f32, size: f64, t: f64) {
    let spacing = (size * 0.55).max(2.0);
    let total = s.length();
    let count = (total / spacing).floor() as u64 + 1;
    let key = s.id.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut seg = 0usize;
    for k in 0..count {
        let h = |salt: u64| hash01(key ^ k, salt);
        let at = ((k as f64 + h(1) * 0.8) * spacing).min(total);
        while seg + 2 < s.points.len() && s.points[seg + 1].along < at {
            seg += 1;
        }
        let (pos, dir) = point_at(s, seg, at);
        let side = Vec2::new(-dir.y, dir.x);
        let centre = pos + side * ((h(2) - 0.5) * size * 1.1);
        let twinkle = (0.5 + 0.5 * (t * (3.0 + 5.0 * h(3)) + TAU * h(4)).sin()).powi(2) as f32;
        let r = size * (0.12 + 0.22 * h(5));
        let a = alpha * (0.15 + 0.85 * twinkle);
        light::radial(out, centre, r * 2.8, |d, _| {
            let core = light::disc(d, r);
            let halo = light::glow(d, r * 1.4) * 0.6;
            light::tint(rgb, core.max(halo) * a, core * 0.7)
        });
    }
}

/// The point `at` pixels along the stroke, inside segment `seg`, and the stroke's direction there.
fn point_at(s: &Stroke, seg: usize, at: f64) -> (Vec2, Vec2) {
    let right = Vec2::new(1.0, 0.0);
    match (s.points.get(seg), s.points.get(seg + 1)) {
        (Some(a), Some(b)) => {
            let span = b.along - a.along;
            let u = if span > 1e-9 {
                ((at - a.along) / span).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = b.pos - a.pos;
            let len = d.length();
            let dir = if len > 1e-9 { d * (1.0 / len) } else { right };
            (a.pos.lerp(b.pos, u), dir)
        }
        (Some(a), None) => (a.pos, right),
        _ => (Vec2::ZERO, right),
    }
}
