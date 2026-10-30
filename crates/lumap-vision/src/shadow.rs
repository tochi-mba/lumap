//! Separate people's shadows from projected content.
use crate::Gray;
use lumap_core::geom::Vec2;
/// Shadow thresholds.
#[derive(Debug, Clone, Copy)]
pub struct ShadowSettings {
    pub ratio: f64,
    pub margin: u8,
    pub min_area: u32,
}
impl Default for ShadowSettings {
    fn default() -> Self {
        Self {
            ratio: 0.7,
            margin: 18,
            min_area: 40,
        }
    }
}
/// Expected camera brightness for projected luma.
pub fn expected(white: &Gray, black: &Gray, projected: &Gray) -> Gray {
    Gray::from_fn(white.width, white.height, |x, y| {
        let p = projected.get(x.min(projected.width - 1), y.min(projected.height - 1));
        let b = black.get(x, y);
        b.saturating_add(
            ((u16::from(white.get(x, y).saturating_sub(b)) * u16::from(p)) / 255) as u8,
        )
    })
}
/// Produces a binary shadow mask.
pub fn mask(observed: &Gray, expected: &Gray, s: &ShadowSettings) -> Gray {
    Gray::from_fn(observed.width, observed.height, |x, y| {
        let o = observed.get(x, y);
        let e = expected.get(x, y);
        if o.saturating_add(s.margin) < e && f64::from(o) < f64::from(e) * s.ratio {
            255
        } else {
            0
        }
    })
}
/// Resamples a camera mask into projector pixels.
pub fn to_projector(mask: &Gray, w: u32, h: u32, map: &dyn Fn(Vec2) -> Option<Vec2>) -> Gray {
    Gray::from_fn(w, h, |x, y| {
        map(Vec2::new(f64::from(x) + 0.5, f64::from(y) + 0.5))
            .map_or(0, |p| mask.at(p.x.floor() as i64, p.y.floor() as i64))
    })
}
