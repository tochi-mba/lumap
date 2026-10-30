//! Track a compact bright torch or laser spot.
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
/// Simple low-pass filter.
#[derive(Debug, Clone, Copy)]
pub struct OneEuro {
    value: Option<f64>,
    pub min_cutoff: f64,
    pub beta: f64,
    pub d_cutoff: f64,
}
impl OneEuro {
    /// Creates a filter.
    pub fn new(min_cutoff: f64, beta: f64, d_cutoff: f64) -> Self {
        Self {
            value: None,
            min_cutoff,
            beta,
            d_cutoff,
        }
    }
    /// Filters one scalar sample.
    pub fn filter(&mut self, v: f64, _t: f64) -> f64 {
        let out = self.value.map_or(v, |old| old * 0.65 + v * 0.35);
        self.value = Some(out);
        out
    }
}
/// Bright-blob tracker.
pub struct SpotTracker {
    s: SpotSettings,
    last: Option<Spot>,
}
impl SpotTracker {
    /// Creates a tracker.
    pub fn new(s: SpotSettings) -> Self {
        Self { s, last: None }
    }
    /// Clears history.
    pub fn reset(&mut self) {
        self.last = None
    }
    /// Analyses one frame.
    pub fn update(&mut self, f: &Rgba, expected: Option<&Gray>, _t: f64) -> Option<Spot> {
        let (mut sx, mut sy, mut weight) = (0.0, 0.0, 0.0);
        for y in 0..f.height {
            for x in 0..f.width {
                let p = f.get(x, y);
                let b = *p[..3].iter().max().unwrap_or(&0);
                let base = expected.map_or(0, |g| g.get(x.min(g.width - 1), y.min(g.height - 1)));
                if b >= self.s.min_brightness && b.saturating_sub(base) > 20 {
                    let w = f64::from(b.saturating_sub(base));
                    sx += (f64::from(x) + 0.5) * w;
                    sy += (f64::from(y) + 0.5) * w;
                    weight += w;
                }
            }
        }
        let spot = (weight > 0.0).then(|| Spot {
            pos: Vec2::new(sx / weight, sy / weight),
            strength: (weight / 2550.0).min(1.0),
        });
        self.last = spot;
        spot
    }
}
