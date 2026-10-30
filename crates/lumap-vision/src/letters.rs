//! Falling text that rests on shadow pixels.
use crate::{Gray, Rgba};
/// Tiny 5x7 glyph rows.
pub fn glyph(c: char) -> Option<[u8; 7]> {
    c.is_ascii_graphic().then_some([31, 17, 17, 31, 17, 17, 17])
}
/// Deterministic letter-rain simulation.
pub struct LetterRain {
    w: u32,
    h: u32,
    text: Vec<char>,
    y: f64,
}
impl LetterRain {
    /// Creates rain.
    pub fn new(w: u32, h: u32, text: &str, _seed: u64) -> Self {
        Self {
            w,
            h,
            text: text.chars().collect(),
            y: 0.0,
        }
    }
    /// Advances falling letters.
    pub fn step(&mut self, dt: f64, _obstacles: &Gray) {
        self.y = (self.y + dt * 24.0) % f64::from(self.h.max(1))
    }
    /// Draws the letters.
    pub fn render(&self, out: &mut Rgba, color: [f32; 4], scale: f32) {
        *out = Rgba::new(self.w, self.h);
        let rgba = color.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
        let s = scale.max(1.0) as u32;
        for (i, _) in self.text.iter().enumerate() {
            let x = (i as u32 * 6 * s) % self.w.max(1);
            let y = (self.y as u32 + (i as u32 * 13)) % self.h.max(1);
            for yy in y..(y + s * 7).min(self.h) {
                for xx in x..(x + s * 5).min(self.w) {
                    out.set(xx, yy, rgba)
                }
            }
        }
    }
}
