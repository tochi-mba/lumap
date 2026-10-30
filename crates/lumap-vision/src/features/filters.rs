//! Gradients, Gaussian blur and the Canny edge detector.

use crate::image::Gray;

/// `tan(22.5°)`: splits gradient directions into the four Canny sectors.
const TAN_22_5: f32 = 0.414_213_57;

/// Horizontal and vertical 3×3 Sobel derivatives of `g` (edges clamped), row-major like the
/// image. On a ramp of slope `s` grey levels per pixel the response is `8s`; across a clean step
/// of height `h` it is `4h`. An inconsistent image gives all-zero derivatives.
pub fn sobel(g: &Gray) -> (Vec<f32>, Vec<f32>) {
    let w = g.width as usize;
    let h = g.height as usize;
    let mut gx = vec![0.0f32; w * h];
    let mut gy = vec![0.0f32; w * h];
    if w == 0 || h == 0 || !g.is_consistent() {
        return (gx, gy);
    }
    let d = &g.data;
    let px = |row: usize, x: usize| f32::from(d[row * w + x]);
    for y in 0..h {
        let (ym, yp) = (y.saturating_sub(1), (y + 1).min(h - 1));
        for x in 0..w {
            let (xm, xp) = (x.saturating_sub(1), (x + 1).min(w - 1));
            let sx = (px(ym, xp) - px(ym, xm))
                + 2.0 * (px(y, xp) - px(y, xm))
                + (px(yp, xp) - px(yp, xm));
            let sy = (px(yp, xm) - px(ym, xm))
                + 2.0 * (px(yp, x) - px(ym, x))
                + (px(yp, xp) - px(ym, xp));
            let i = y * w + x;
            gx[i] = sx;
            gy[i] = sy;
        }
    }
    (gx, gy)
}

/// Gradient magnitude `hypot(gx, gy)` per pixel.
pub(crate) fn magnitude(gx: &[f32], gy: &[f32]) -> Vec<f32> {
    gx.iter().zip(gy).map(|(x, y)| x.hypot(*y)).collect()
}

/// Normalised Gaussian taps for `sigma`, radius `ceil(3 sigma)`.
fn gaussian_kernel(sigma: f64) -> Vec<f32> {
    let radius = (3.0 * sigma).ceil().clamp(1.0, 64.0) as i64;
    let taps: Vec<f64> = (-radius..=radius)
        .map(|k| (-(k * k) as f64 / (2.0 * sigma * sigma)).exp())
        .collect();
    let sum: f64 = taps.iter().sum();
    taps.iter().map(|t| (t / sum) as f32).collect()
}

/// Separable Gaussian blur with standard deviation `sigma` pixels, edges clamped. A `sigma` that
/// is not a positive number, or an inconsistent image, returns a copy.
pub fn blur(g: &Gray, sigma: f64) -> Gray {
    let w = g.width as usize;
    let h = g.height as usize;
    if !(sigma.is_finite() && sigma > 0.0) || w == 0 || h == 0 || !g.is_consistent() {
        return g.clone();
    }
    let kernel = gaussian_kernel(sigma);
    let r = kernel.len() / 2;
    let mut tmp = vec![0.0f32; w * h];
    for (row, out) in g.data.chunks_exact(w).zip(tmp.chunks_exact_mut(w)) {
        for (x, o) in out.iter_mut().enumerate() {
            let mut acc = 0.0f32;
            for (k, kv) in kernel.iter().enumerate() {
                let sx = (x + k).saturating_sub(r).min(w - 1);
                acc += kv * f32::from(row[sx]);
            }
            *o = acc;
        }
    }
    let mut data = vec![0u8; w * h];
    let mut acc = vec![0.0f32; w];
    for (y, out) in data.chunks_exact_mut(w).enumerate() {
        acc.fill(0.0);
        for (k, kv) in kernel.iter().enumerate() {
            let sy = (y + k).saturating_sub(r).min(h - 1);
            for (a, s) in acc.iter_mut().zip(&tmp[sy * w..sy * w + w]) {
                *a += kv * s;
            }
        }
        for (o, a) in out.iter_mut().zip(&acc) {
            *o = a.round().clamp(0.0, 255.0) as u8;
        }
    }
    Gray {
        width: g.width,
        height: g.height,
        data,
    }
}

/// Canny edges of `g`: 255 on thin (one pixel wide) edge pixels, 0 elsewhere. `low` and `high`
/// are the hysteresis thresholds on the Sobel gradient magnitude (see [`sobel`]: a blurred step
/// of height `h` peaks near `3h`): pixels above `high` start an edge, which then extends through
/// connected pixels above `low`. The image is not smoothed here; blur it first. The outermost
/// pixel ring never holds an edge.
pub fn canny(g: &Gray, low: f64, high: f64) -> Gray {
    if !g.is_consistent() {
        return Gray::new(g.width, g.height);
    }
    let (gx, gy) = sobel(g);
    let mag = magnitude(&gx, &gy);
    let (w, h) = (g.width as usize, g.height as usize);
    canny_from_gradient(w, h, &gx, &gy, &mag, low, high)
}

/// Canny on an already computed gradient (`w * h` values each).
pub(crate) fn canny_from_gradient(
    w: usize,
    h: usize,
    gx: &[f32],
    gy: &[f32],
    mag: &[f32],
    low: f64,
    high: f64,
) -> Gray {
    let mut out = Gray::new(w as u32, h as u32);
    if w < 3 || h < 3 {
        return out;
    }
    let low = if low.is_finite() { low.max(0.0) } else { 0.0 } as f32;
    let high = if high.is_finite() {
        high.max(f64::from(low))
    } else {
        f64::from(low)
    } as f32;
    // 0: suppressed, 1: weak candidate, 2: strong.
    let mut state = vec![0u8; w * h];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            let m = mag[i];
            if m <= 0.0 || m < low {
                continue;
            }
            let (ax, ay) = (gx[i].abs(), gy[i].abs());
            let (before, after) = if ay <= ax * TAN_22_5 {
                (i - 1, i + 1)
            } else if ax <= ay * TAN_22_5 {
                (i - w, i + w)
            } else if (gx[i] > 0.0) == (gy[i] > 0.0) {
                (i - w - 1, i + w + 1)
            } else {
                (i - w + 1, i + w - 1)
            };
            if m > mag[before] && m >= mag[after] {
                state[i] = if m >= high { 2 } else { 1 };
            }
        }
    }
    let mut stack = Vec::new();
    for start in 0..w * h {
        if state[start] != 2 || out.data[start] != 0 {
            continue;
        }
        out.data[start] = 255;
        stack.push(start);
        while let Some(i) = stack.pop() {
            // Candidates never sit on the outer ring, so their neighbours are in bounds.
            for j in [i - w - 1, i - w, i - w + 1, i - 1, i + 1, i + w - 1, i + w, i + w + 1] {
                if state[j] != 0 && out.data[j] == 0 {
                    out.data[j] = 255;
                    stack.push(j);
                }
            }
        }
    }
    out
}
