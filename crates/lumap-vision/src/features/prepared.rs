//! The smoothed photo, its gradient and its Canny edges, computed once and shared by the facets,
//! the surface finder, the wand and snapping.

use super::filters::{blur, canny_from_gradient, magnitude, sobel};
use crate::image::{Gray, Rgba};
use lumap_core::geom::Vec2;

/// Gaussian blur applied before anything is measured, in pixels.
pub(crate) const SIGMA: f64 = 1.0;
/// Canny thresholds on the Sobel magnitude of the smoothed luma. A blurred step of `h` grey
/// levels peaks near `3h`, so edges need roughly 15 levels of contrast to start and continue
/// through 7; webcam noise and the projector's fall-off stay far below.
pub(crate) const CANNY_LOW: f64 = 20.0;
/// See [`CANNY_LOW`].
pub(crate) const CANNY_HIGH: f64 = 45.0;

/// A photo analysed for edges. Every buffer is `w * h`, row-major.
pub(crate) struct Prepared {
    pub w: usize,
    pub h: usize,
    /// Smoothed red, green and blue; empty for a grey-only analysis.
    pub rgb: [Vec<u8>; 3],
    /// Smoothed luma.
    pub luma: Gray,
    pub gx: Vec<f32>,
    pub gy: Vec<f32>,
    pub mag: Vec<f32>,
    /// Canny edges of the smoothed luma (255 on edges).
    pub edges: Gray,
}

impl Prepared {
    /// Full analysis: colour, luma, gradient and edges.
    pub(crate) fn new(photo: &Rgba) -> Self {
        Self::build(photo, true)
    }

    /// Luma, gradient and edges only (no colour channels).
    pub(crate) fn grey(photo: &Rgba) -> Self {
        Self::build(photo, false)
    }

    fn build(photo: &Rgba, colour: bool) -> Self {
        if !photo.is_consistent() || photo.width == 0 || photo.height == 0 {
            return Self {
                w: 0,
                h: 0,
                rgb: [Vec::new(), Vec::new(), Vec::new()],
                luma: Gray::default(),
                gx: Vec::new(),
                gy: Vec::new(),
                mag: Vec::new(),
                edges: Gray::default(),
            };
        }
        let (w, h) = (photo.width as usize, photo.height as usize);
        let luma = blur(&Gray::from_rgba(photo), SIGMA);
        let rgb = if colour {
            let mut channels = [
                Gray::new(photo.width, photo.height),
                Gray::new(photo.width, photo.height),
                Gray::new(photo.width, photo.height),
            ];
            for (i, px) in photo.data.chunks_exact(4).enumerate() {
                for (c, ch) in channels.iter_mut().enumerate() {
                    ch.data[i] = px[c];
                }
            }
            channels.map(|c| blur(&c, SIGMA).data)
        } else {
            [Vec::new(), Vec::new(), Vec::new()]
        };
        let (gx, gy) = sobel(&luma);
        let mag = magnitude(&gx, &gy);
        let edges = canny_from_gradient(w, h, &gx, &gy, &mag, CANNY_LOW, CANNY_HIGH);
        Self {
            w,
            h,
            rgb,
            luma,
            gx,
            gy,
            mag,
            edges,
        }
    }

    /// Smoothed colour of pixel `i`.
    pub(crate) fn colour(&self, i: usize) -> [f32; 3] {
        [
            f32::from(self.rgb[0][i]),
            f32::from(self.rgb[1][i]),
            f32::from(self.rgb[2][i]),
        ]
    }

    /// Euclidean distance between the smoothed colours of two pixels.
    pub(crate) fn colour_distance(&self, i: usize, j: usize) -> f32 {
        let (a, b) = (self.colour(i), self.colour(j));
        let (dr, dg, db) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
        (dr * dr + dg * dg + db * db).sqrt()
    }

    /// Bilinear gradient at a continuous position.
    pub(crate) fn gradient_at(&self, p: Vec2) -> (f64, f64) {
        (
            sample(&self.gx, self.w, self.h, p),
            sample(&self.gy, self.w, self.h, p),
        )
    }
}

/// Bilinear sample of a `w * h` buffer at a continuous position (pixel centres at `+0.5`),
/// edges clamped; zero for an empty buffer.
pub(crate) fn sample(buf: &[f32], w: usize, h: usize, p: Vec2) -> f64 {
    if w == 0 || h == 0 || !p.is_finite() {
        return 0.0;
    }
    let fx = (p.x - 0.5).clamp(0.0, (w - 1) as f64);
    let fy = (p.y - 0.5).clamp(0.0, (h - 1) as f64);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
    let v = |x: usize, y: usize| f64::from(buf[y * w + x]);
    let top = v(x0, y0) * (1.0 - tx) + v(x1, y0) * tx;
    let bottom = v(x0, y1) * (1.0 - tx) + v(x1, y1) * tx;
    top * (1.0 - ty) + bottom * ty
}
