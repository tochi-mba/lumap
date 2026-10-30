//! Snow: flakes drift down, come to rest on solid cells and on each other, slide off the
//! shoulders of piles, fall again when what held them goes, and melt.

use super::{World, STEP};
use crate::image::Rgba;
use crate::paint::light;
use lumap_core::geom::Vec2;
use std::f64::consts::{PI, TAU};

/// Fall speed as a fraction of the frame height per second.
const FALL: f64 = 0.10;
/// Sideways sway as a fraction of the frame height per second.
const SWAY: f64 = 0.03;
/// Flakes spawned per second for each grid column.
const SPAWN_PER_COLUMN: f64 = 0.35;
/// Seconds a settled flake lasts, shortest and longest.
const MELT: (f64, f64) = (25.0, 45.0);
/// Seconds over which a melting flake fades out.
const MELT_FADE: f64 = 3.0;
/// Cells a buried flake may be lifted to find room.
const LIFT: i64 = 8;

#[derive(Debug, Clone, Copy)]
pub(super) struct Flake {
    pub(super) x: f64,
    pub(super) y: f64,
    speed: f64,
    phase: f64,
    sway: f64,
    /// Seconds left before a settled flake melts.
    life: f64,
    /// The grid cell it rests in, if settled.
    pub(super) rest: Option<usize>,
}

impl World {
    pub(super) fn step_snow(&mut self) {
        self.spawn_snow();
        let mut i = 0;
        while i < self.snow.len() {
            let keep = if self.snow[i].rest.is_some() {
                self.settled(i)
            } else {
                self.falling(i)
            };
            if keep {
                i += 1;
            } else {
                self.snow.swap_remove(i);
            }
        }
    }

    fn spawn_snow(&mut self) {
        self.spawn_debt += SPAWN_PER_COLUMN * self.grid.w as f64 * STEP;
        let h = f64::from(self.height);
        while self.spawn_debt >= 1.0 {
            self.spawn_debt -= 1.0;
            if self.snow.len() >= self.snow_cap {
                self.spawn_debt = 0.0;
                break;
            }
            let flake = Flake {
                x: self.rng.unit() * f64::from(self.width),
                y: -self.rng.unit() * self.grid.cell * 2.0,
                speed: FALL * h * self.rng.range(0.7, 1.3),
                phase: self.rng.range(0.0, TAU),
                sway: SWAY * h * self.rng.range(0.3, 1.0),
                life: 0.0,
                rest: None,
            };
            self.snow.push(flake);
        }
    }

    /// Moves a falling flake; false when it leaves the bottom or finds no room to settle.
    fn falling(&mut self, i: usize) -> bool {
        let mut f = self.snow[i];
        let vx = f.sway * (f.phase + self.time * 1.3).sin();
        let nx = (f.x + vx * STEP).rem_euclid(f64::from(self.width));
        let ny = f.y + f.speed * STEP;
        if ny >= f64::from(self.height) {
            return false;
        }
        let g = &self.grid;
        let (ox, oy) = (g.col(f.x), g.row(f.y));
        let cy = g.row(ny);
        if !g.blocked(g.col(nx), cy) {
            f.x = nx;
            f.y = ny;
        } else if !g.blocked(ox, cy) {
            // Only the sideways drift was blocked.
            f.y = ny;
        } else {
            // Something below: slide off a pile's shoulder, else settle here.
            let first = if f.phase < PI { -1 } else { 1 };
            let slide = [first, -first].into_iter().find_map(|dx| {
                let sx = (ox + dx).rem_euclid(g.w as i64);
                (!g.blocked(sx, oy) && !g.blocked(sx, oy + 1)).then_some(sx)
            });
            match slide {
                Some(sx) => f.x = g.centre(sx),
                None => return self.settle(i, f, ox, oy),
            }
        }
        self.snow[i] = f;
        true
    }

    /// Rests flake `i` in the first free cell at or above `(cx, cy)`; false if there is none.
    fn settle(&mut self, i: usize, mut f: Flake, cx: i64, cy: i64) -> bool {
        let mut row = cy;
        for _ in 0..LIFT {
            if row < 0 {
                return false;
            }
            if !self.grid.blocked(cx, row) {
                let Some(cell) = self.grid.index(cx, row) else {
                    return false;
                };
                self.grid.rest[cell] = 1;
                f.rest = Some(cell);
                f.x = self.grid.centre(cx);
                f.y = self.grid.centre(row);
                f.life = self.rng.range(MELT.0, MELT.1);
                self.snow[i] = f;
                return true;
            }
            row -= 1;
        }
        false
    }

    /// Ages a settled flake; it melts, is lifted out of a shadow that covers it, or falls when
    /// its support goes. False when it is gone.
    fn settled(&mut self, i: usize) -> bool {
        let mut f = self.snow[i];
        let Some(cell) = f.rest else {
            return true;
        };
        f.life -= STEP;
        if f.life <= 0.0 {
            self.grid.rest[cell] = 0;
            return false;
        }
        let (cx, cy) = ((cell % self.grid.w) as i64, (cell / self.grid.w) as i64);
        if self.grid.solid_at(cx, cy) != 0 {
            self.grid.rest[cell] = 0;
            f.rest = None;
            let life = f.life;
            let kept = self.settle(i, f, cx, cy - 1);
            if kept {
                self.snow[i].life = life;
            }
            return kept;
        }
        if !self.grid.blocked(cx, cy + 1) {
            self.grid.rest[cell] = 0;
            f.rest = None;
        }
        self.snow[i] = f;
        true
    }

    pub(super) fn render_snow(&self, out: &mut Rgba, rgb: [f32; 3], alpha: f32, t: f64) {
        let r = (self.grid.cell * 0.55).max(0.9);
        for f in &self.snow {
            let melt = if f.rest.is_some() {
                (f.life / MELT_FADE).min(1.0)
            } else {
                1.0
            };
            let twinkle = 0.8 + 0.2 * (t * 2.3 + f.phase * 3.0).sin();
            let a = alpha * (melt * twinkle) as f32;
            light::radial(out, Vec2::new(f.x, f.y), r * 2.2, |d, _| {
                let core = light::disc(d, r);
                let halo = light::glow(d, r * 1.4) * 0.45;
                light::tint(rgb, core.max(halo) * a, core * 0.3)
            });
        }
    }
}
