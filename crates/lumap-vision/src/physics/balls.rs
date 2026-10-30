//! Balls of light and marbles: gravity, restitution and friction against solid cells (the
//! contact normal is the local gradient of the solid field), against each other through a
//! bucket grid, and, for balls, against the frame.

use super::{Grid, Kind, World, DYNAMIC, MAX_BALLS, MAX_MARBLES, STEP};
use crate::image::Rgba;
use crate::paint::light;
use lumap_core::geom::Vec2;
use std::f64::consts::TAU;

/// Gravity as a fraction of the frame height per second squared.
const GRAVITY: f64 = 0.9;
/// Below this speed (fraction of the height per second) a touching ball counts as still.
const STILL_SPEED: f64 = 0.04;
/// How hard a moving shadow shoves what it overlaps, per pixel of overlap per sub-step.
const SHOVE: f64 = 0.5;
/// Rings of cells searched for room when a shadow swallows a ball.
const ESCAPE_RINGS: i64 = 24;
/// Restitution between two balls.
const PAIR_RESTITUTION: f64 = 0.9;

/// How one kind of ball behaves.
#[derive(Debug, Clone, Copy)]
pub(super) struct Feel {
    restitution: f64,
    /// Share of the sliding speed kept per contact sub-step.
    grip: f64,
    /// Air drag per second.
    drag: f64,
    count: usize,
    /// Radius as a fraction of the frame height.
    radius: f64,
    /// Whether the frame's sides are walls.
    walls: bool,
    /// Seconds still before a ball is kicked or a marble starts again.
    patience: f64,
}

const BOUNCE: Feel = Feel {
    restitution: 0.82,
    grip: 0.995,
    drag: 0.05,
    count: 36,
    radius: 0.018,
    walls: true,
    patience: 1.5,
};

const MARBLE: Feel = Feel {
    restitution: 0.2,
    grip: 0.9995,
    drag: 0.02,
    count: 16,
    radius: 0.015,
    walls: false,
    patience: 3.0,
};

pub(super) fn feel(kind: Kind) -> Feel {
    match kind {
        Kind::Marble => MARBLE,
        Kind::Snow | Kind::Bounce => BOUNCE,
    }
}

/// A ball's radius in pixels for a frame `height` pixels tall.
pub(super) fn radius(feel: Feel, height: u32) -> f64 {
    (feel.radius * f64::from(height)).max(2.5)
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Ball {
    pub(super) p: Vec2,
    pub(super) v: Vec2,
    r: f64,
    /// Seconds spent still and touching something.
    still: f64,
    phase: f64,
}

/// A uniform grid of linked lists over the balls, rebuilt every sub-step without allocating.
#[derive(Debug, Clone)]
pub(super) struct Buckets {
    size: f64,
    w: usize,
    h: usize,
    heads: Vec<i32>,
    next: Vec<i32>,
}

impl Buckets {
    pub(super) fn new(width: u32, height: u32, size: f64, cap: usize) -> Self {
        let size = size.max(1.0);
        let w = (f64::from(width) / size).ceil().max(1.0) as usize;
        let h = (f64::from(height) / size).ceil().max(1.0) as usize;
        Self {
            size,
            w,
            h,
            heads: vec![-1; w * h],
            next: vec![-1; cap],
        }
    }

    fn cell(&self, p: Vec2) -> (usize, usize) {
        let c = |v: f64, n: usize| ((v / self.size).floor().max(0.0) as usize).min(n - 1);
        (c(p.x, self.w), c(p.y, self.h))
    }

    fn rebuild(&mut self, balls: &[Ball]) {
        self.heads.fill(-1);
        for (i, b) in balls.iter().enumerate().take(self.next.len()) {
            let (x, y) = self.cell(b.p);
            let head = &mut self.heads[y * self.w + x];
            self.next[i] = *head;
            *head = i as i32;
        }
    }
}

/// Overlap of a disc with solid cells.
struct Contact {
    /// Sum of the push directions weighted by penetration: the solid field's local gradient.
    push: Vec2,
    depth: f64,
    dynamic: bool,
}

fn contact(grid: &Grid, p: Vec2, r: f64) -> Option<Contact> {
    let c = grid.cell;
    let (cx0, cx1) = (((p.x - r) / c).floor() as i64, ((p.x + r) / c).floor() as i64);
    let (cy0, cy1) = (((p.y - r) / c).floor() as i64, ((p.y + r) / c).floor() as i64);
    let mut found: Option<Contact> = None;
    for cy in cy0..=cy1 {
        for cx in cx0..=cx1 {
            let s = grid.solid_at(cx, cy);
            if s == 0 {
                continue;
            }
            let (lx, ly) = (cx as f64 * c, cy as f64 * c);
            let q = Vec2::new(p.x.clamp(lx, lx + c), p.y.clamp(ly, ly + c));
            let d = p - q;
            let dist = d.length();
            if dist >= r {
                continue;
            }
            let dir = if dist > 1e-9 {
                d * (1.0 / dist)
            } else {
                let e = p - Vec2::new(lx + c * 0.5, ly + c * 0.5);
                let len = e.length();
                if len > 1e-9 {
                    e * (1.0 / len)
                } else {
                    Vec2::new(0.0, -1.0)
                }
            };
            let k = found.get_or_insert(Contact {
                push: Vec2::ZERO,
                depth: 0.0,
                dynamic: false,
            });
            k.push = k.push + dir * (r - dist);
            k.depth = k.depth.max(r - dist);
            k.dynamic |= s & DYNAMIC != 0;
        }
    }
    found
}

/// The centre of the nearest open cell to `p`, searching outward ring by ring.
fn escape(grid: &Grid, p: Vec2) -> Option<Vec2> {
    let (cx, cy) = ((p.x / grid.cell).floor() as i64, grid.row(p.y));
    for ring in 1..=ESCAPE_RINGS {
        let mut best: Option<(f64, Vec2)> = None;
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dy.abs()) != ring {
                    continue;
                }
                let (x, y) = (cx + dx, cy + dy);
                if grid.index(x, y).is_none() || grid.solid_at(x, y) != 0 {
                    continue;
                }
                let c = Vec2::new(grid.centre(x), grid.centre(y));
                let d = c.distance(p);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, c));
                }
            }
        }
        if let Some((_, c)) = best {
            return Some(c);
        }
    }
    None
}

impl World {
    fn new_ball(&mut self, p: Vec2, v: Vec2) -> Ball {
        Ball {
            p,
            v,
            r: radius(feel(self.kind), self.height),
            still: 0.0,
            phase: self.rng.range(0.0, TAU),
        }
    }

    pub(super) fn launch_balls(&mut self) {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let r = radius(BOUNCE, self.height);
        for _ in 0..BOUNCE.count.min(MAX_BALLS) {
            let p = Vec2::new(
                self.rng.range(r, (w - r).max(r)),
                self.rng.range(r, (h * 0.6).max(r)),
            );
            let v = Vec2::new(
                self.rng.range(-0.4, 0.4) * h,
                self.rng.range(-0.3, 0.1) * h,
            );
            let ball = self.new_ball(p, v);
            self.balls.push(ball);
        }
    }

    /// A marble dropped in at the top somewhere across the middle of the frame.
    fn drop_marble(&mut self) -> Ball {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let r = radius(MARBLE, self.height);
        let p = Vec2::new(self.rng.range(0.1, 0.9) * w, -r * 2.0);
        let v = Vec2::new(self.rng.range(-0.05, 0.05) * h, 0.0);
        self.new_ball(p, v)
    }

    pub(super) fn step_balls(&mut self) {
        let feel = feel(self.kind);
        if self.kind == Kind::Marble {
            self.spawn_debt += STEP / 0.6;
            if self.spawn_debt >= 1.0 {
                self.spawn_debt -= 1.0;
                if self.balls.len() < feel.count.min(MAX_MARBLES) {
                    let m = self.drop_marble();
                    self.balls.push(m);
                }
            }
        }
        for i in 0..self.balls.len() {
            let mut b = self.balls[i];
            let touching = self.move_ball(&mut b, feel);
            self.balls[i] = self.settle_ball(b, touching, feel);
        }
        self.collide_balls();
    }

    /// Integrates one ball and resolves it against the frame and solids; true when it touches.
    fn move_ball(&self, b: &mut Ball, feel: Feel) -> bool {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        b.v.y += GRAVITY * h * STEP;
        b.v = b.v * (1.0 - feel.drag * STEP);
        let top = 0.5 * b.r / STEP;
        let speed = b.v.length();
        if speed > top {
            b.v = b.v * (top / speed);
        }
        b.p = b.p + b.v * STEP;
        let mut touching = false;
        if feel.walls {
            let e = feel.restitution;
            if b.p.x < b.r {
                b.p.x = b.r;
                b.v.x = b.v.x.abs() * e;
            } else if b.p.x > w - b.r {
                b.p.x = w - b.r;
                b.v.x = -b.v.x.abs() * e;
            }
            if b.p.y < b.r {
                b.p.y = b.r;
                b.v.y = b.v.y.abs() * e;
            } else if b.p.y > h - b.r {
                b.p.y = h - b.r;
                b.v.y = -b.v.y.abs() * e;
                touching = true;
            }
        }
        let g = &self.grid;
        if g.solid_at((b.p.x / g.cell).floor() as i64, g.row(b.p.y)) != 0 {
            // Swallowed by a shadow (or spawned inside an edge): pop out to open space.
            if let Some(to) = escape(g, b.p) {
                let d = to - b.p;
                let len = d.length().max(1e-9);
                b.v = d * ((len / STEP * SHOVE).min(h) / len);
                b.p = to;
            }
            return true;
        }
        for _ in 0..3 {
            let Some(c) = contact(g, b.p, b.r) else {
                break;
            };
            touching = true;
            let len = c.push.length();
            let n = if len > 1e-9 {
                c.push * (1.0 / len)
            } else {
                Vec2::new(0.0, -1.0)
            };
            b.p = b.p + n * c.depth;
            let vn = b.v.dot(n);
            if vn < 0.0 {
                let slide = b.v - n * vn;
                b.v = slide * feel.grip - n * (vn * feel.restitution);
            }
            if c.dynamic {
                b.v = b.v + n * (c.depth / STEP * SHOVE).min(0.8 * h);
            }
        }
        touching
    }

    /// Kicks still balls back into the air and starts lost or still marbles again.
    fn settle_ball(&mut self, mut b: Ball, touching: bool, feel: Feel) -> Ball {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let slow = b.v.length() < STILL_SPEED * h;
        b.still = if slow && touching { b.still + STEP } else { 0.0 };
        match self.kind {
            Kind::Marble => {
                let gone = b.p.y - b.r > h || b.p.x + b.r < 0.0 || b.p.x - b.r > w;
                if gone || b.still > feel.patience {
                    return self.drop_marble();
                }
            }
            Kind::Bounce | Kind::Snow => {
                if b.still > feel.patience {
                    b.v = Vec2::new(
                        self.rng.range(-0.35, 0.35) * h,
                        -self.rng.range(0.7, 1.05) * h,
                    );
                    b.still = 0.0;
                }
            }
        }
        b
    }

    fn collide_balls(&mut self) {
        self.buckets.rebuild(&self.balls);
        let (bw, bh) = (self.buckets.w, self.buckets.h);
        for i in 0..self.balls.len().min(self.buckets.next.len()) {
            let (x, y) = self.buckets.cell(self.balls[i].p);
            for ny in y.saturating_sub(1)..=(y + 1).min(bh - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(bw - 1) {
                    let mut j = self.buckets.heads[ny * bw + nx];
                    while j >= 0 {
                        let ju = j as usize;
                        if ju > i {
                            self.resolve_pair(i, ju);
                        }
                        j = self.buckets.next[ju];
                    }
                }
            }
        }
    }

    fn resolve_pair(&mut self, i: usize, j: usize) {
        let (a, b) = (self.balls[i], self.balls[j]);
        let d = b.p - a.p;
        let reach = a.r + b.r;
        let dist2 = d.dot(d);
        if dist2 >= reach * reach {
            return;
        }
        let dist = dist2.sqrt();
        let n = if dist > 1e-9 {
            d * (1.0 / dist)
        } else {
            Vec2::new(1.0, 0.0)
        };
        let half = (reach - dist) * 0.5;
        self.balls[i].p = a.p - n * half;
        self.balls[j].p = b.p + n * half;
        let closing = (b.v - a.v).dot(n);
        if closing < 0.0 {
            let k = -(1.0 + PAIR_RESTITUTION) * closing * 0.5;
            self.balls[i].v = a.v - n * k;
            self.balls[j].v = b.v + n * k;
        }
    }

    pub(super) fn render_balls(&self, out: &mut Rgba, rgb: [f32; 3], alpha: f32, t: f64) {
        for b in &self.balls {
            if self.kind == Kind::Marble {
                marble(out, b, rgb, alpha);
            } else {
                let a = alpha * (0.85 + 0.15 * (t * 2.0 + b.phase).sin()) as f32;
                let r = b.r;
                light::radial(out, b.p, r * 3.2, |d, _| {
                    let core = light::disc(d, r);
                    let halo = light::glow(d, r * 1.6) * 0.55;
                    let hot = (1.0 - d / r).clamp(0.0, 1.0) as f32 * 0.6;
                    light::tint(rgb, core.max(halo) * a, hot * core)
                });
            }
        }
    }
}

/// A glossy sphere lit from the top left, with a faint glow around it.
fn marble(out: &mut Rgba, b: &Ball, rgb: [f32; 3], alpha: f32) {
    let r = b.r;
    let l = [-0.45_f64, -0.6, 0.66];
    let ll = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
    let l = [l[0] / ll, l[1] / ll, l[2] / ll];
    // Half-way vector between the light and a viewer straight ahead.
    let hv = [l[0], l[1], l[2] + 1.0];
    let hl = (hv[0] * hv[0] + hv[1] * hv[1] + hv[2] * hv[2]).sqrt();
    let hv = [hv[0] / hl, hv[1] / hl, hv[2] / hl];
    light::radial(out, b.p, r * 1.9, |d, o| {
        let cover = light::disc(d, r);
        if cover <= 0.0 {
            let halo = light::glow(d - r, r * 0.5) * 0.25;
            return light::tint(rgb, halo * alpha, 0.0);
        }
        let (nx, ny) = ((o.x / r).clamp(-1.0, 1.0), (o.y / r).clamp(-1.0, 1.0));
        let nz = (1.0 - nx * nx - ny * ny).max(0.0).sqrt();
        let diffuse = (nx * l[0] + ny * l[1] + nz * l[2]).max(0.0);
        let spec = (nx * hv[0] + ny * hv[1] + nz * hv[2]).max(0.0).powi(24);
        let k = (0.3 + 0.7 * diffuse) as f32;
        light::tint(
            [rgb[0] * k, rgb[1] * k, rgb[2] * k],
            cover * alpha,
            (spec * 0.9) as f32,
        )
    });
}
