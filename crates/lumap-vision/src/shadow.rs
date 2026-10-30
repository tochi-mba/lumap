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
            ratio: 0.72,
            margin: 18,
            min_area: 40,
        }
    }
}
/// Expected camera brightness for projected luma.
pub fn expected(white: &Gray, black: &Gray, projected: &Gray) -> Gray {
    Gray::from_fn(white.width, white.height, |x, y| {
        let p = projected.get(
            x.min(projected.width.saturating_sub(1)),
            y.min(projected.height.saturating_sub(1)),
        );
        let b = black.get(x, y);
        b.saturating_add(
            ((u16::from(white.get(x, y).saturating_sub(b)) * u16::from(p)) / 255) as u8,
        )
    })
}
fn morph(g: &Gray, dilate: bool) -> Gray {
    Gray::from_fn(g.width, g.height, |x, y| {
        let (x, y) = (i64::from(x), i64::from(y));
        let hit = if dilate {
            (-1..=1).any(|dy| (-1..=1).any(|dx| g.at(x + dx, y + dy) > 0))
        } else {
            (-1..=1).all(|dy| (-1..=1).all(|dx| g.at(x + dx, y + dy) > 0))
        };
        if hit {
            255
        } else {
            0
        }
    })
}
/// Produces a cleaned binary shadow mask.
pub fn mask(observed: &Gray, expected: &Gray, s: &ShadowSettings) -> Gray {
    let raw = Gray::from_fn(observed.width, observed.height, |x, y| {
        let o = observed.get(x, y);
        let e = expected.get(x, y);
        if o.saturating_add(s.margin) < e && f64::from(o) < f64::from(e) * s.ratio {
            255
        } else {
            0
        }
    });
    let mut clean = morph(&morph(&raw, false), true);
    let mut seen = vec![false; clean.data.len()];
    for start in 0..clean.data.len() {
        if clean.data[start] == 0 || seen[start] {
            continue;
        }
        let mut stack = vec![start];
        let mut blob = Vec::new();
        seen[start] = true;
        while let Some(i) = stack.pop() {
            blob.push(i);
            let (x, y) = (
                (i % clean.width as usize) as i64,
                (i / clean.width as usize) as i64,
            );
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let (xx, yy) = (x + dx, y + dy);
                if xx >= 0 && yy >= 0 && xx < i64::from(clean.width) && yy < i64::from(clean.height)
                {
                    let j = yy as usize * clean.width as usize + xx as usize;
                    if clean.data[j] > 0 && !seen[j] {
                        seen[j] = true;
                        stack.push(j)
                    }
                }
            }
        }
        if blob.len() < s.min_area as usize {
            for i in blob {
                clean.data[i] = 0
            }
        }
    }
    clean
}
/// Resamples a camera mask into projector pixels.
pub fn to_projector(mask: &Gray, w: u32, h: u32, map: &dyn Fn(Vec2) -> Option<Vec2>) -> Gray {
    Gray::from_fn(w, h, |x, y| {
        map(Vec2::new(f64::from(x) + 0.5, f64::from(y) + 0.5))
            .map_or(0, |p| mask.at(p.x.floor() as i64, p.y.floor() as i64))
    })
}
