//! The picture the scan sheet fills in while a scan runs. Every pair of stripes the camera
//! photographs halves the uncertainty about where each camera pixel lands in the projector, so
//! the picture starts as two broad bands and sharpens, pair by pair, into a smooth map: colour
//! across the projector, brightness down it.

use lumap_vision::graycode::Axis;
use lumap_vision::{Gray, Rgba};

/// Preview cells are this many camera pixels on a side.
/// Cells whose white-to-black contrast is under this percentage of the projection's strong
/// contrast (its 90th percentile) are bounced light, left out of the picture.
const BOUNCE: u32 = 50;
const CELL: u32 = 4;

/// What one cell knows so far.
#[derive(Debug, Clone, Copy, Default)]
struct Cell {
    /// Lit by the projector at all.
    lit: bool,
    /// Gray-code prefix read across, and how many stripes it holds.
    gx: u32,
    nx: u32,
    /// Stopped reading across (a stripe it could not read).
    done_x: bool,
    /// The same, down.
    gy: u32,
    ny: u32,
    done_y: bool,
}

/// The filling-in picture.
#[derive(Debug, Clone)]
pub struct Preview {
    width: u32,
    height: u32,
    cells: Vec<Cell>,
}

impl Preview {
    /// A preview over the camera's picture; `footprint` (camera resolution) says which cells
    /// the projector lights.
    pub fn new(footprint: &Gray, white: &Gray, black: &Gray) -> Self {
        let (width, height) = (footprint.width / CELL, footprint.height / CELL);
        let centre = |x: u32, y: u32| (x * CELL + CELL / 2, y * CELL + CELL / 2);
        let contrast = |x: u32, y: u32| {
            let (px, py) = centre(x, y);
            white
                .at(px.into(), py.into())
                .saturating_sub(black.at(px.into(), py.into()))
        };
        let lit = |x: u32, y: u32| {
            let (px, py) = centre(x, y);
            footprint.get(px, py) > 0
        };
        // Light bounced onto the room around the projection follows the stripes too, only
        // dimmer: cells far dimmer than the projection's own brightest stay out of the picture.
        let mut strong: Vec<u8> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .filter(|&(x, y)| lit(x, y))
            .map(|(x, y)| contrast(x, y))
            .collect();
        strong.sort_unstable();
        let floor = strong
            .get(strong.len() * 9 / 10)
            .map_or(0, |&c| (u32::from(c) * BOUNCE / 100) as u8);
        let cells = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| Cell {
                lit: lit(x, y) && contrast(x, y) >= floor,
                ..Cell::default()
            })
            .collect();
        Self {
            width,
            height,
            cells,
        }
    }

    /// Reads one stripe from photos of a pattern and its inverse (camera resolution).
    pub fn add(&mut self, axis: Axis, pattern: &Gray, inverse: &Gray, white: &Gray, black: &Gray) {
        for y in 0..self.height {
            for x in 0..self.width {
                let (px, py) = (x * CELL + CELL / 2, y * CELL + CELL / 2);
                let cell = &mut self.cells[(y * self.width + x) as usize];
                let (g, n, done) = match axis {
                    Axis::X => (&mut cell.gx, &mut cell.nx, &mut cell.done_x),
                    Axis::Y => (&mut cell.gy, &mut cell.ny, &mut cell.done_y),
                };
                if !cell.lit || *done {
                    continue;
                }
                let (a, b) = (
                    pattern.at(px.into(), py.into()),
                    inverse.at(px.into(), py.into()),
                );
                let contrast = white
                    .at(px.into(), py.into())
                    .saturating_sub(black.at(px.into(), py.into()));
                let need = (f64::from(contrast) * 0.3).max(6.0);
                if f64::from(a.abs_diff(b)) < need {
                    *done = true;
                    continue;
                }
                *g = (*g << 1) | u32::from(a > b);
                *n += 1;
            }
        }
    }

    /// The picture so far, one pixel per cell.
    pub fn image(&self) -> Rgba {
        Rgba::from_fn(self.width, self.height, |x, y| {
            let c = self.cells[(y * self.width + x) as usize];
            if !c.lit {
                return [8, 10, 9, 255];
            }
            if c.nx == 0 && c.ny == 0 {
                return [40, 48, 42, 255];
            }
            let fx = fraction(c.gx, c.nx);
            let fy = fraction(c.gy, c.ny);
            hsv(fx * 300.0, 0.85, 0.35 + 0.65 * fy)
        })
    }

    /// Share of lit cells that have read at least one stripe on each axis.
    pub fn progress(&self) -> f64 {
        let lit = self.cells.iter().filter(|c| c.lit).count();
        if lit == 0 {
            return 0.0;
        }
        let known = self
            .cells
            .iter()
            .filter(|c| c.lit && (c.nx > 0 || c.ny > 0))
            .count();
        known as f64 / lit as f64
    }
}

/// Where a Gray-code prefix of `n` stripes puts a pixel, 0..1 (the middle of what it narrows
/// down to).
fn fraction(gray: u32, n: u32) -> f64 {
    if n == 0 {
        return 0.5;
    }
    let mut bin = gray;
    let mut shift = gray >> 1;
    while shift != 0 {
        bin ^= shift;
        shift >>= 1;
    }
    (f64::from(bin) + 0.5) / f64::from(1u32 << n.min(31))
}

/// A colour from hue (degrees), saturation and value.
fn hsv(h: f64, s: f64, v: f64) -> [u8; 4] {
    let c = v * s;
    let hp = (h / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to = |u: f64| ((u + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [to(r), to(g), to(b), 255]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stripes_sharpen_the_picture_cell_by_cell() {
        let footprint = Gray::from_fn(16, 8, |x, _| if x < 12 { 255 } else { 0 });
        let white = Gray::from_fn(16, 8, |_, _| 200);
        let black = Gray::from_fn(16, 8, |_, _| 10);
        let mut p = Preview::new(&footprint, &white, &black);
        assert_eq!(p.progress(), 0.0);
        assert_eq!(
            p.image().get(0, 0),
            [40, 48, 42, 255],
            "lit, nothing read yet"
        );
        assert_eq!(p.image().get(3, 0), [8, 10, 9, 255], "not lit");
        // Left half bright in the pattern: stripe 1 there, 0 on the right.
        let pattern = Gray::from_fn(16, 8, |x, _| if x < 8 { 200 } else { 10 });
        let inverse = Gray::from_fn(16, 8, |x, _| if x < 8 { 10 } else { 200 });
        p.add(Axis::X, &pattern, &inverse, &white, &black);
        assert_eq!(p.progress(), 1.0);
        assert_ne!(p.image().get(0, 0), p.image().get(2, 0), "two bands now");
        // A flat pair cannot be read: those cells stop refining across.
        p.add(Axis::X, &white, &white, &white, &black);
        assert!(p
            .cells
            .iter()
            .filter(|c| c.lit)
            .all(|c| c.done_x && c.nx == 1));
        p.add(Axis::Y, &pattern, &inverse, &white, &black);
        assert_eq!(fraction(0b1, 1), 0.75);
        assert_eq!(fraction(0, 0), 0.5);
        assert_eq!(hsv(0.0, 1.0, 1.0), [255, 0, 0, 255]);
        assert_eq!(hsv(240.0, 1.0, 1.0), [0, 0, 255, 255]);
    }

    #[test]
    fn dim_bounced_light_stays_out_of_the_picture() {
        let footprint = Gray::from_fn(16, 8, |_, _| 255);
        // The left half is the projection, the right half the wall beside it catching a glow.
        let white = Gray::from_fn(16, 8, |x, _| if x < 8 { 220 } else { 50 });
        let black = Gray::from_fn(16, 8, |_, _| 10);
        let p = Preview::new(&footprint, &white, &black);
        assert!(p.cells[0].lit);
        assert!(!p.cells[3].lit);
    }
}
