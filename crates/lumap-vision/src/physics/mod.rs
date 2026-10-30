//! Particles that fall, settle, bounce and roll against the room's edges and shadows.
//!
//! A [`World`] lives in projector pixels over a grid of solid cells: the room's edges from a
//! scan ([`World::set_static`]) and people's shadows from the camera ([`World::set_dynamic`]).
//! Snow drifts down from the top and piles up on ledges, frames and shoulders, then slowly
//! melts; balls of light bounce with gravity and restitution and get pushed around by people;
//! marbles roll along edges and fall off their ends. Time advances in fixed sub-steps of
//! [`STEP`], so a run depends only on its seed and its inputs, never on the frame rate.

mod balls;
pub(crate) mod rng;
mod snow;

use crate::image::{Gray, Rgba};
use crate::paint::light;
use lumap_core::geom::Vec2;
use rng::Rng;

/// Simulated seconds per internal sub-step.
pub const STEP: f64 = 1.0 / 120.0;
/// Most sub-steps one call to [`World::step`] runs (half a second); a longer gap is dropped.
const MAX_STEPS: u32 = 60;
/// Most snowflakes alive at once, falling or settled.
pub const MAX_SNOW: usize = 4000;
/// Most balls of light.
pub const MAX_BALLS: usize = 64;
/// Most marbles.
pub const MAX_MARBLES: usize = 24;

/// Grid bit: a room edge.
const STATIC: u8 = 1;
/// Grid bit: a shadow.
const DYNAMIC: u8 = 2;

/// What a world simulates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Snow that drifts down, piles up on ledges and shadows, and slowly melts.
    Snow,
    /// Glowing balls that bounce off edges, shadows and the frame.
    Bounce,
    /// Marbles that roll along edges, fall off their ends and start again at the top.
    Marble,
}

/// Solid cells and settled snow, a few projector pixels per cell.
#[derive(Debug, Clone)]
struct Grid {
    /// Cell side in pixels.
    cell: f64,
    w: usize,
    h: usize,
    /// `STATIC | DYNAMIC` bits per cell.
    solid: Vec<u8>,
    /// 1 where a snowflake rests.
    rest: Vec<u8>,
}

impl Grid {
    fn new(width: u32, height: u32) -> Self {
        let cell = (f64::from(width.min(height)) / 270.0).round().max(1.0);
        let w = (f64::from(width) / cell).ceil().max(1.0) as usize;
        let h = (f64::from(height) / cell).ceil().max(1.0) as usize;
        Self {
            cell,
            w,
            h,
            solid: vec![0; w * h],
            rest: vec![0; w * h],
        }
    }

    fn index(&self, cx: i64, cy: i64) -> Option<usize> {
        ((0..self.w as i64).contains(&cx) && (0..self.h as i64).contains(&cy))
            .then(|| cy as usize * self.w + cx as usize)
    }

    /// The solid bits of a cell; outside the grid is open.
    fn solid_at(&self, cx: i64, cy: i64) -> u8 {
        self.index(cx, cy).map_or(0, |i| self.solid[i])
    }

    /// Solid or holding snow.
    fn blocked(&self, cx: i64, cy: i64) -> bool {
        self.index(cx, cy)
            .is_some_and(|i| self.solid[i] != 0 || self.rest[i] != 0)
    }

    /// The column of `x`, clamped into the grid.
    fn col(&self, x: f64) -> i64 {
        ((x / self.cell).floor() as i64).clamp(0, self.w as i64 - 1)
    }

    /// The row of `y`; above and below the grid are kept as such.
    fn row(&self, y: f64) -> i64 {
        (y / self.cell).floor() as i64
    }

    /// The centre of column or row `c`, in pixels.
    fn centre(&self, c: i64) -> f64 {
        (c as f64 + 0.5) * self.cell
    }

    /// Replaces `bit` with the pixels of `image` at or above 128, scaled to `width` x `height`.
    fn paint(&mut self, image: &Gray, bit: u8, width: u32, height: u32) {
        for s in &mut self.solid {
            *s &= !bit;
        }
        if !image.is_consistent() || image.width == 0 || image.height == 0 {
            return;
        }
        let sx = f64::from(width) / f64::from(image.width) / self.cell;
        let sy = f64::from(height) / f64::from(image.height) / self.cell;
        let iw = image.width as usize;
        for (i, &v) in image.data.iter().enumerate() {
            if v < 128 {
                continue;
            }
            let (x, y) = ((i % iw) as f64, (i / iw) as f64);
            let (cx0, cx1) = (
                (x * sx).floor() as i64,
                ((x + 1.0) * sx - 1e-9).floor() as i64,
            );
            let (cy0, cy1) = (
                (y * sy).floor() as i64,
                ((y + 1.0) * sy - 1e-9).floor() as i64,
            );
            for cy in cy0..=cy1.max(cy0) {
                for cx in cx0..=cx1.max(cx0) {
                    if let Some(j) = self.index(cx, cy) {
                        self.solid[j] |= bit;
                    }
                }
            }
        }
    }
}

/// A particle simulation over a projector-sized frame.
#[derive(Debug, Clone)]
pub struct World {
    width: u32,
    height: u32,
    kind: Kind,
    grid: Grid,
    rng: Rng,
    /// Simulated seconds so far.
    time: f64,
    /// Time handed to `step` but not yet simulated.
    pending: f64,
    snow: Vec<snow::Flake>,
    snow_cap: usize,
    /// Fractional particles owed by the spawner.
    spawn_debt: f64,
    balls: Vec<balls::Ball>,
    buckets: balls::Buckets,
}

impl World {
    /// An empty room of `width` x `height` projector pixels simulating `kind`, its randomness
    /// drawn from `seed`. Balls of light start in the air; snow and marbles arrive over time.
    pub fn new(width: u32, height: u32, kind: Kind, seed: u64) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let grid = Grid::new(width, height);
        let feel = balls::feel(kind);
        let radius = balls::radius(feel, height);
        let mut world = Self {
            width,
            height,
            kind,
            grid,
            rng: Rng::new(seed),
            time: 0.0,
            pending: 0.0,
            snow: Vec::with_capacity(if kind == Kind::Snow { MAX_SNOW } else { 0 }),
            snow_cap: MAX_SNOW,
            spawn_debt: 0.0,
            balls: Vec::with_capacity(match kind {
                Kind::Snow => 0,
                Kind::Bounce => MAX_BALLS,
                Kind::Marble => MAX_MARBLES,
            }),
            buckets: balls::Buckets::new(width, height, radius * 2.0, MAX_BALLS),
        };
        if kind == Kind::Bounce {
            world.launch_balls();
        }
        world
    }

    /// The room's edges in projector pixels: 255 is solid (a dilated edge map works well;
    /// ledges catch snow). An image of another size is scaled to the world.
    pub fn set_static(&mut self, edges: &Gray) {
        self.grid.paint(edges, STATIC, self.width, self.height);
    }

    /// People's shadows in projector pixels (255 = shadow): snow settles on them and they push
    /// balls and marbles. Call it every frame with the latest mask.
    pub fn set_dynamic(&mut self, mask: &Gray) {
        self.grid.paint(mask, DYNAMIC, self.width, self.height);
    }

    /// Advances the simulation by `dt` seconds in fixed sub-steps of [`STEP`]; time left over
    /// carries to the next call, and at most half a second runs per call.
    pub fn step(&mut self, dt: f64) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.pending += dt;
        let mut steps = 0;
        while self.pending >= STEP - 1e-9 && steps < MAX_STEPS {
            self.pending -= STEP;
            match self.kind {
                Kind::Snow => self.step_snow(),
                Kind::Bounce | Kind::Marble => self.step_balls(),
            }
            self.time += STEP;
            steps += 1;
        }
        self.pending = self.pending.clamp(0.0, STEP);
    }

    /// Draws the particles into `out` (resized to the world and cleared, its buffer reused) as
    /// soft glowing light tinted by `color` (straight RGBA in `0..=1`) at time `t`.
    pub fn render(&self, out: &mut Rgba, color: [f32; 4], t: f64) {
        light::begin(out, self.width, self.height);
        let (rgb, alpha) = light::split(color);
        match self.kind {
            Kind::Snow => self.render_snow(out, rgb, alpha, t),
            Kind::Bounce | Kind::Marble => self.render_balls(out, rgb, alpha, t),
        }
        light::finish(out);
    }

    /// How many particles are alive (for snow, falling and settled).
    pub fn len(&self) -> usize {
        self.snow.len() + self.balls.len()
    }

    /// No particles alive.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// What the world simulates.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Every particle's centre in projector pixels.
    pub fn positions(&self) -> impl Iterator<Item = Vec2> + '_ {
        self.snow
            .iter()
            .map(|f| Vec2::new(f.x, f.y))
            .chain(self.balls.iter().map(|b| b.p))
    }
}
