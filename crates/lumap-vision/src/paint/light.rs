//! Light the room effects draw: premultiplied colour accumulated with a per-channel maximum, so
//! the overlapping pieces of one stroke join without seams or beads and glows never blow out,
//! then turned into the straight-alpha [`Rgba`] frames lumap composites.

use crate::image::Rgba;
use lumap_core::geom::Vec2;

/// A premultiplied colour, each channel in `0..=1`.
pub(crate) type Light = [f32; 4];

/// Sizes `out` to `width` x `height`, reusing its buffer, and clears it to transparent black.
pub(crate) fn begin(out: &mut Rgba, width: u32, height: u32) {
    out.width = width;
    out.height = height;
    out.data.clear();
    out.data.resize(width as usize * height as usize * 4, 0);
}

/// Turns the premultiplied accumulation in `out` into straight alpha.
pub(crate) fn finish(out: &mut Rgba) {
    for px in out.data.chunks_exact_mut(4) {
        let a = px[3];
        if a == 0 {
            px[..3].fill(0);
        } else if a < 255 {
            let k = 255.0 / f32::from(a);
            for c in &mut px[..3] {
                *c = (f32::from(*c) * k).round().min(255.0) as u8;
            }
        }
    }
}

/// Max-blends premultiplied `light` into the pixel at `(x, y)`; outside the frame is ignored.
#[inline]
pub(crate) fn blend(out: &mut Rgba, x: i64, y: i64, light: Light) {
    if !(0..i64::from(out.width)).contains(&x) || !(0..i64::from(out.height)).contains(&y) {
        return;
    }
    let i = (y as usize * out.width as usize + x as usize) * 4;
    let Some(px) = out.data.get_mut(i..i + 4) else {
        return;
    };
    for (dst, src) in px.iter_mut().zip(light) {
        let v = (src.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        if v > *dst {
            *dst = v;
        }
    }
}

/// Premultiplied light of colour `rgb` at opacity `alpha`, pushed toward white by `white`.
#[inline]
pub(crate) fn tint(rgb: [f32; 3], alpha: f32, white: f32) -> Light {
    let a = alpha.clamp(0.0, 1.0);
    let w = white.clamp(0.0, 1.0);
    [
        (rgb[0] + (1.0 - rgb[0]) * w) * a,
        (rgb[1] + (1.0 - rgb[1]) * w) * a,
        (rgb[2] + (1.0 - rgb[2]) * w) * a,
        a,
    ]
}

/// Splits a straight RGBA tint (`0..=1`) into its colour and opacity, clamped; NaN reads as 0.
pub(crate) fn split(color: [f32; 4]) -> ([f32; 3], f32) {
    let c = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
    ([c(color[0]), c(color[1]), c(color[2])], c(color[3]))
}

/// RGB (`0..=1`) of a hue in turns (wrapped), a saturation and a value.
pub(crate) fn hsv(h: f64, s: f32, v: f32) -> [f32; 3] {
    let h6 = if h.is_finite() { h.rem_euclid(1.0) * 6.0 } else { 0.0 };
    let i = h6.floor();
    let f = (h6 - i) as f32;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as u8 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Anti-aliased coverage of a pixel whose centre is `d` from the centre of a disc of radius `r`.
#[inline]
pub(crate) fn disc(d: f64, r: f64) -> f32 {
    ((r - d + 0.5).clamp(0.0, 1.0) * (2.0 * r).min(1.0)) as f32
}

/// A soft Gaussian falloff: 1 at the centre, about 0.37 at `sigma`.
#[inline]
pub(crate) fn glow(d: f64, sigma: f64) -> f32 {
    let k = d / sigma.max(1e-6);
    (-k * k).exp() as f32
}

/// Smooth step from 0 at `e0` to 1 at `e1` (either order).
#[inline]
pub(crate) fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let span = e1 - e0;
    if span.abs() < 1e-12 {
        return if x < e0 { 0.0 } else { 1.0 };
    }
    let t = ((x - e0) / span).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The pixels whose centres lie in `[min_x, max_x] x [min_y, max_y]`, clipped to the frame.
fn clip(out: &Rgba, min: Vec2, max: Vec2) -> Option<(i64, i64, i64, i64)> {
    let x0 = ((min.x - 0.5).ceil() as i64).max(0);
    let x1 = ((max.x - 0.5).floor() as i64).min(i64::from(out.width) - 1);
    let y0 = ((min.y - 0.5).ceil() as i64).max(0);
    let y1 = ((max.y - 0.5).floor() as i64).min(i64::from(out.height) - 1);
    (x0 <= x1 && y0 <= y1).then_some((x0, x1, y0, y1))
}

/// Max-blends `shade(distance, offset)` into every pixel whose centre lies within `reach` of
/// `centre`, where `offset` is the pixel centre minus `centre`.
pub(crate) fn radial(
    out: &mut Rgba,
    centre: Vec2,
    reach: f64,
    mut shade: impl FnMut(f64, Vec2) -> Light,
) {
    if !centre.is_finite() || reach.is_nan() || reach <= 0.0 {
        return;
    }
    let span = Vec2::new(reach, reach);
    let Some((x0, x1, y0, y1)) = clip(out, centre - span, centre + span) else {
        return;
    };
    let r2 = reach * reach;
    for y in y0..=y1 {
        let dy = y as f64 + 0.5 - centre.y;
        for x in x0..=x1 {
            let dx = x as f64 + 0.5 - centre.x;
            let d2 = dx * dx + dy * dy;
            if d2 <= r2 {
                let light = shade(d2.sqrt(), Vec2::new(dx, dy));
                blend(out, x, y, light);
            }
        }
    }
}

/// Max-blends `shade(distance, u)` into every pixel within `reach` of the segment `a`-`b`, where
/// `u` in `0..=1` is how far along the segment the nearest point lies. Pixels beyond `b` are left
/// to the next segment unless `end` is set, so a polyline drawn segment by segment shades each
/// pixel about once and its joins and caps come out round.
pub(crate) fn segment(
    out: &mut Rgba,
    a: Vec2,
    b: Vec2,
    reach: f64,
    end: bool,
    mut shade: impl FnMut(f64, f64) -> Light,
) {
    if !a.is_finite() || !b.is_finite() || reach.is_nan() || reach <= 0.0 {
        return;
    }
    let lo = Vec2::new(a.x.min(b.x) - reach, a.y.min(b.y) - reach);
    let hi = Vec2::new(a.x.max(b.x) + reach, a.y.max(b.y) + reach);
    let Some((x0, x1, y0, y1)) = clip(out, lo, hi) else {
        return;
    };
    let ab = b - a;
    let len2 = ab.dot(ab);
    let inv = if len2 > 1e-12 { 1.0 / len2 } else { 0.0 };
    let r2 = reach * reach;
    for y in y0..=y1 {
        let py = y as f64 + 0.5;
        for x in x0..=x1 {
            let p = Vec2::new(x as f64 + 0.5, py);
            let raw = (p - a).dot(ab) * inv;
            if raw >= 1.0 && !end {
                continue;
            }
            let u = raw.clamp(0.0, 1.0);
            let q = p - (a + ab * u);
            let d2 = q.dot(q);
            if d2 <= r2 {
                let light = shade(d2.sqrt(), u);
                blend(out, x, y, light);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn light_max_blends_and_unpremultiplies() {
        let mut out = Rgba::new(1, 1);
        begin(&mut out, 4, 2);
        assert_eq!(out.data.len(), 32);
        blend(&mut out, 1, 1, tint([1.0, 0.0, 0.0], 0.5, 0.0));
        blend(&mut out, 1, 1, tint([1.0, 0.0, 0.0], 0.25, 0.0));
        blend(&mut out, -1, 9, [1.0; 4]);
        finish(&mut out);
        assert_eq!(out.get(1, 1), [255, 0, 0, 128]);
        assert_eq!(out.get(0, 0), [0, 0, 0, 0]);
        assert_eq!(split([2.0, f32::NAN, -1.0, 0.5]), ([1.0, 0.0, 0.0], 0.5));
        assert_eq!(hsv(0.0, 1.0, 1.0), [1.0, 0.0, 0.0]);
        assert_eq!(hsv(1.0 / 3.0, 1.0, 1.0)[1], 1.0);
        assert_eq!(smoothstep(1.0, 0.0, 0.0), 1.0);
        assert_eq!(smoothstep(0.0, 0.0, 1.0), 1.0);
        assert!(glow(0.0, 1.0) > glow(1.0, 1.0));
    }

    #[test]
    fn segments_cover_their_reach_and_leave_the_far_end_to_the_next() {
        let mut out = Rgba::new(20, 10);
        begin(&mut out, 20, 10);
        segment(&mut out, Vec2::new(2.0, 5.0), Vec2::new(10.0, 5.0), 2.0, false, |_, _| {
            [1.0; 4]
        });
        assert_eq!(out.get(5, 4)[3], 255);
        assert_eq!(out.get(5, 8)[3], 0);
        assert_eq!(out.get(11, 4)[3], 0);
        segment(&mut out, Vec2::new(2.0, 5.0), Vec2::new(10.0, 5.0), 2.0, true, |_, _| {
            [1.0; 4]
        });
        assert_eq!(out.get(11, 4)[3], 255);
        radial(&mut out, Vec2::new(17.0, 5.0), 1.0, |d, _| [0.0, 0.0, 0.0, disc(d, 1.0)]);
        assert!(out.get(16, 4)[3] > 0);
        radial(&mut out, Vec2::new(f64::NAN, 0.0), 1.0, |_, _| [1.0; 4]);
    }
}
