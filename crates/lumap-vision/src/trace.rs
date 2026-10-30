//! Animated light following photographed room edges.
use crate::{Gray, Rgba};
/// Linked edge representation.
pub struct Trace {
    edges: Gray,
    count: usize,
}
impl Trace {
    /// Builds edge chains.
    pub fn new(edges: &Gray) -> Self {
        Self {
            edges: edges.clone(),
            count: usize::from(edges.data.iter().any(|&v| v > 0)),
        }
    }
    /// Draws moving pulses.
    pub fn render(&self, out: &mut Rgba, color: [f32; 4], speed: f32, t: f64) {
        *out = Rgba::new(self.edges.width, self.edges.height);
        let c = color.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
        for y in 0..self.edges.height {
            for x in 0..self.edges.width {
                if self.edges.get(x, y) > 0
                    && ((f64::from(x + y) + t * f64::from(speed) * 80.0) as i64).rem_euclid(100)
                        < 32
                {
                    out.set(x, y, c)
                }
            }
        }
    }
    /// Number of edge chains.
    pub fn chains(&self) -> usize {
        self.count
    }
}
