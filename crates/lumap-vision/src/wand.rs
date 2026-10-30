//! Click-to-outline selection on the projector-eye photograph.
use crate::{
    features::{self, shapes},
    Gray, Rgba,
};
use lumap_core::geom::Vec2;
/// A selected object outline.
#[derive(Debug, Clone)]
pub struct Outline {
    pub points: Vec<Vec2>,
    pub quad: Option<[Vec2; 4]>,
    pub area: f64,
}
/// Grows a colour-similar region and returns its bounding outline.
pub fn pick(photo: &Rgba, seed: Vec2, tolerance: f64) -> Option<Outline> {
    let sx = seed.x.floor() as i64;
    let sy = seed.y.floor() as i64;
    if sx < 0 || sy < 0 || sx >= i64::from(photo.width) || sy >= i64::from(photo.height) {
        return None;
    }
    let base = photo.get(sx as u32, sy as u32);
    let mut seen = vec![false; photo.width as usize * photo.height as usize];
    let mut stack = vec![(sx as u32, sy as u32)];
    let mut mask = Gray::new(photo.width, photo.height);
    let mut n = 0usize;
    while let Some((x, y)) = stack.pop() {
        let i = y as usize * photo.width as usize + x as usize;
        if seen[i] {
            continue;
        }
        seen[i] = true;
        let c = photo.get(x, y);
        let d = ((f64::from(c[0]) - f64::from(base[0])).powi(2)
            + (f64::from(c[1]) - f64::from(base[1])).powi(2)
            + (f64::from(c[2]) - f64::from(base[2])).powi(2))
        .sqrt();
        if d > tolerance {
            continue;
        }
        n += 1;
        mask.set(x, y, 255);
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let (xx, yy) = (i64::from(x) + dx, i64::from(y) + dy);
            if xx >= 0 && yy >= 0 && xx < i64::from(photo.width) && yy < i64::from(photo.height) {
                stack.push((xx as u32, yy as u32));
            }
        }
    }
    if n < 16 || n > photo.width as usize * photo.height as usize * 9 / 10 {
        return None;
    }
    let raw = features::contours(&mask).into_iter().max_by_key(Vec::len)?;
    let points = features::simplify(&raw, 2.0, true);
    if points.len() < 3 {
        return None;
    }
    let quad = shapes::quad_from_outline(&raw)
        .filter(shapes::is_quad_like)
        .map(|f| f.quad);
    Some(Outline {
        points,
        quad,
        area: n as f64,
    })
}
