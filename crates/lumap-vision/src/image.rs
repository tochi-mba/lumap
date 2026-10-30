//! Plain image buffers the vision code works on: 8-bit grey and 8-bit RGBA, row-major, with
//! pixel centres at `x + 0.5, y + 0.5`. `Rgba` has the layout of `lumap_sources::Frame`, so the
//! app converts between them by moving the byte vector.

/// An 8-bit single-channel image: `width * height` bytes, row-major.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Gray {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// One byte per pixel.
    pub data: Vec<u8>,
}

/// An 8-bit RGBA image: `width * height * 4` bytes, row-major, straight alpha.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rgba {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Four bytes per pixel.
    pub data: Vec<u8>,
}

impl Gray {
    /// An all-black image.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; width as usize * height as usize],
        }
    }

    /// An image whose every pixel is `f(x, y)`.
    pub fn from_fn(width: u32, height: u32, f: impl Fn(u32, u32) -> u8) -> Self {
        let mut data = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height {
            for x in 0..width {
                data.push(f(x, y));
            }
        }
        Self {
            width,
            height,
            data,
        }
    }

    /// The pixel at `(x, y)`; panics outside the image.
    pub fn get(&self, x: u32, y: u32) -> u8 {
        self.data[y as usize * self.width as usize + x as usize]
    }

    /// Sets the pixel at `(x, y)`; panics outside the image.
    pub fn set(&mut self, x: u32, y: u32, v: u8) {
        let w = self.width as usize;
        self.data[y as usize * w + x as usize] = v;
    }

    /// The pixel at `(x, y)`, clamped to the nearest edge pixel outside the image (`0` for an
    /// empty image).
    pub fn at(&self, x: i64, y: i64) -> u8 {
        if self.width == 0 || self.height == 0 {
            return 0;
        }
        let x = x.clamp(0, i64::from(self.width) - 1) as u32;
        let y = y.clamp(0, i64::from(self.height) - 1) as u32;
        self.get(x, y)
    }

    /// Bilinear sample at a continuous position (pixel centres at `+0.5`), edges clamped.
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        let fx = x - 0.5;
        let fy = y - 0.5;
        let x0 = fx.floor();
        let y0 = fy.floor();
        let tx = fx - x0;
        let ty = fy - y0;
        let (x0, y0) = (x0 as i64, y0 as i64);
        let p = |dx: i64, dy: i64| f64::from(self.at(x0 + dx, y0 + dy));
        let top = p(0, 0) * (1.0 - tx) + p(1, 0) * tx;
        let bottom = p(0, 1) * (1.0 - tx) + p(1, 1) * tx;
        top * (1.0 - ty) + bottom * ty
    }

    /// Luma (Rec. 709 weights) of an RGBA image, alpha ignored.
    pub fn from_rgba(image: &Rgba) -> Self {
        let data = image
            .data
            .chunks_exact(4)
            .map(|p| {
                let l =
                    0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]);
                l.round().clamp(0.0, 255.0) as u8
            })
            .collect();
        Self {
            width: image.width,
            height: image.height,
            data,
        }
    }

    /// The buffer length matches the size.
    pub fn is_consistent(&self) -> bool {
        self.data.len() == self.width as usize * self.height as usize
    }
}

impl Rgba {
    /// A transparent black image.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; width as usize * height as usize * 4],
        }
    }

    /// An image whose every pixel is `f(x, y)`.
    pub fn from_fn(width: u32, height: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Self {
        let mut data = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&f(x, y));
            }
        }
        Self {
            width,
            height,
            data,
        }
    }

    /// The pixel at `(x, y)`; panics outside the image.
    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }

    /// Sets the pixel at `(x, y)`; panics outside the image.
    pub fn set(&mut self, x: u32, y: u32, v: [u8; 4]) {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.data[i..i + 4].copy_from_slice(&v);
    }

    /// An opaque grey image.
    pub fn from_gray(image: &Gray) -> Self {
        let data = image.data.iter().flat_map(|&v| [v, v, v, 255]).collect();
        Self {
            width: image.width,
            height: image.height,
            data,
        }
    }

    /// The buffer length matches the size.
    pub fn is_consistent(&self) -> bool {
        self.data.len() == self.width as usize * self.height as usize * 4
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn grey_images_read_write_clamp_and_sample() {
        let mut g = Gray::from_fn(4, 2, |x, y| (x * 10 + y * 100) as u8);
        assert!(g.is_consistent());
        assert_eq!(g.get(3, 1), 130);
        g.set(0, 0, 7);
        assert_eq!(g.at(-5, -5), 7);
        assert_eq!(g.at(99, 99), 130);
        assert_eq!(Gray::new(0, 0).at(0, 0), 0);
        // Pixel centres sample exactly; half way between two centres averages them.
        assert_eq!(g.sample(1.5, 0.5), 10.0);
        assert_eq!(g.sample(2.0, 0.5), 15.0);
        assert_eq!(g.sample(1.5, 1.0), 60.0);
    }

    #[test]
    fn rgba_round_trips_through_grey() {
        let mut c = Rgba::new(2, 1);
        assert!(c.is_consistent());
        c.set(1, 0, [255, 255, 255, 255]);
        assert_eq!(c.get(1, 0), [255, 255, 255, 255]);
        let g = Gray::from_rgba(&c);
        assert_eq!(g.data, vec![0, 255]);
        let back = Rgba::from_gray(&g);
        assert_eq!(back.get(0, 0), [0, 0, 0, 255]);
        let f = Rgba::from_fn(1, 1, |_, _| [1, 2, 3, 4]);
        assert_eq!(f.data, vec![1, 2, 3, 4]);
        let green = Rgba::from_fn(1, 1, |_, _| [0, 255, 0, 255]);
        assert_eq!(Gray::from_rgba(&green).data, vec![182]);
    }
}
