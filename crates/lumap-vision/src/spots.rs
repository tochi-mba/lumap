//! Track a compact bright torch or laser spot against projected light or a learned background.
use crate::{Gray, Rgba};
use lumap_core::geom::Vec2;
/// Detection limits.
#[derive(Debug, Clone)]
pub struct SpotSettings {
    pub min_brightness: u8,
    pub min_area: u32,
    pub max_area: u32,
    pub prefer_color: Option<[u8; 3]>,
}
impl Default for SpotSettings {
    fn default() -> Self {
        Self {
            min_brightness: 220,
            min_area: 1,
            max_area: 400,
            prefer_color: None,
        }
    }
}
/// A detected spot.
#[derive(Debug, Clone, Copy)]
pub struct Spot {
    pub pos: Vec2,
    pub strength: f64,
}
/// Adaptive low-pass filter using the One Euro rule.
#[derive(Debug, Clone, Copy)]
pub struct OneEuro {
    value: Option<f64>,
    derivative: f64,
    last_t: Option<f64>,
    pub min_cutoff: f64,
    pub beta: f64,
    pub d_cutoff: f64,
}
impl OneEuro {
    /// Creates a filter.
    pub fn new(min_cutoff: f64, beta: f64, d_cutoff: f64) -> Self {
        Self {
            value: None,
            derivative: 0.0,
            last_t: None,
            min_cutoff,
            beta,
            d_cutoff,
        }
    }
    fn alpha(cut: f64, dt: f64) -> f64 {
        let tau = 1.0 / (std::f64::consts::TAU * cut.max(1e-4));
        1.0 / (1.0 + tau / dt.max(1e-4))
    }
    /// Filters one scalar sample.
    pub fn filter(&mut self, v: f64, t: f64) -> f64 {
        let Some(old) = self.value else {
            self.value = Some(v);
            self.last_t = Some(t);
            return v;
        };
        let dt = (t - self.last_t.unwrap_or(t - 1.0 / 60.0)).max(1e-4);
        let d = (v - old) / dt;
        self.derivative += Self::alpha(self.d_cutoff, dt) * (d - self.derivative);
        let a = Self::alpha(self.min_cutoff + self.beta * self.derivative.abs(), dt);
        let out = old + a * (v - old);
        self.value = Some(out);
        self.last_t = Some(t);
        out
    }
}
/// Bright-blob tracker.
pub struct SpotTracker {
    s: SpotSettings,
    background: Vec<f32>,
    size: (u32, u32),
    fx: OneEuro,
    fy: OneEuro,
    last: Option<(Spot, f64)>,
}
impl SpotTracker {
    /// Creates a tracker.
    pub fn new(s: SpotSettings) -> Self {
        Self {
            s,
            background: Vec::new(),
            size: (0, 0),
            fx: OneEuro::new(1.0, 0.02, 1.0),
            fy: OneEuro::new(1.0, 0.02, 1.0),
            last: None,
        }
    }
    /// Clears history.
    pub fn reset(&mut self) {
        self.background.clear();
        self.last = None
    }
    /// Analyses one frame and chooses the strongest compact connected blob.
    pub fn update(&mut self, f: &Rgba, expected: Option<&Gray>, t: f64) -> Option<Spot> {
        if self.size != (f.width, f.height) {
            self.size = (f.width, f.height);
            self.background = vec![0.0; f.width as usize * f.height as usize]
        }
        let n = f.width as usize * f.height as usize;
        let mut hot = vec![false; n];
        for i in 0..n {
            let p = &f.data[i * 4..i * 4 + 3];
            let bright = *p.iter().max().unwrap_or(&0);
            let base = expected.map_or(self.background[i], |g| {
                f32::from(g.data[i.min(g.data.len() - 1)])
            });
            let color_ok = self.s.prefer_color.is_none_or(|want| {
                let d = (0..3)
                    .map(|c| (i32::from(p[c]) - i32::from(want[c])).unsigned_abs())
                    .sum::<u32>();
                d < 280
            });
            hot[i] = bright >= self.s.min_brightness && f32::from(bright) > base + 24.0 && color_ok;
            self.background[i] = if self.background[i] == 0.0 {
                f32::from(bright)
            } else {
                self.background[i] * 0.995 + f32::from(bright) * 0.005
            };
        }
        let mut seen = vec![false; n];
        let mut best: Option<(f64, Vec2, usize)> = None;
        for start in 0..n {
            if !hot[start] || seen[start] {
                continue;
            }
            let mut stack = vec![start];
            seen[start] = true;
            let (mut sx, mut sy, mut weight, mut count) = (0.0, 0.0, 0.0, 0usize);
            while let Some(i) = stack.pop() {
                let (x, y) = (i % f.width as usize, i / f.width as usize);
                let b = f64::from(*f.data[i * 4..i * 4 + 3].iter().max().unwrap_or(&0));
                sx += (x as f64 + 0.5) * b;
                sy += (y as f64 + 0.5) * b;
                weight += b;
                count += 1;
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let (xx, yy) = (x as i64 + dx, y as i64 + dy);
                    if xx >= 0 && yy >= 0 && xx < i64::from(f.width) && yy < i64::from(f.height) {
                        let j = yy as usize * f.width as usize + xx as usize;
                        if hot[j] && !seen[j] {
                            seen[j] = true;
                            stack.push(j)
                        }
                    }
                }
            }
            if count >= self.s.min_area as usize && count <= self.s.max_area as usize {
                let score = weight / (count as f64).sqrt();
                if best.is_none_or(|b| score > b.0) {
                    best = Some((score, Vec2::new(sx / weight, sy / weight), count))
                }
            }
        }
        let found = best.map(|(_, p, count)| Spot {
            pos: Vec2::new(self.fx.filter(p.x, t), self.fy.filter(p.y, t)),
            strength: (count as f64 / f64::from(self.s.max_area.max(1)))
                .sqrt()
                .clamp(0.0, 1.0),
        });
        if let Some(s) = found {
            self.last = Some((s, t));
            return Some(s);
        }
        self.last.filter(|(_, at)| t - *at <= 0.15).map(|(s, _)| s)
    }
}
