//! Dense camera/projector correspondence maps produced by structured light.
use lumap_core::geom::Vec2;
/// A projector boundary that is outside the camera image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}
/// Projector coordinates for every camera pixel.
#[derive(Debug, Clone)]
pub struct Correspondence {
    pub cam_w: u32,
    pub cam_h: u32,
    pub proj_w: u32,
    pub proj_h: u32,
    pub proj: Vec<[f32; 2]>,
    pub confidence: Vec<u8>,
}
impl Correspondence {
    /// Reads a valid camera pixel.
    pub fn get(&self, x: u32, y: u32) -> Option<Vec2> {
        if x >= self.cam_w || y >= self.cam_h {
            return None;
        }
        let p = self.proj[y as usize * self.cam_w as usize + x as usize];
        (p[0].is_finite() && p[1].is_finite()).then(|| Vec2::new(f64::from(p[0]), f64::from(p[1])))
    }
    /// Samples the map at camera coordinates.
    pub fn sample(&self, p: Vec2) -> Option<Vec2> {
        self.get(
            p.x.round()
                .clamp(0.0, f64::from(self.cam_w.saturating_sub(1))) as u32,
            p.y.round()
                .clamp(0.0, f64::from(self.cam_h.saturating_sub(1))) as u32,
        )
    }
    /// Builds the inverse map by splatting camera samples and filling small holes.
    /// Whether a camera pixel's reading matches its neighbourhood: a stripe misread near a
    /// boundary lands far from where the pixels around it do, and would paint a stray band.
    /// Within a 5x5 window, the median of the other readings must lie within `OUTLIER` projector
    /// pixels (scaled by how many projector pixels one camera pixel spans). Pixels with too few
    /// readings around them are kept.
    fn agrees(&self, x: u32, y: u32, p: Vec2) -> bool {
        let mut xs = Vec::with_capacity(24);
        let mut ys = Vec::with_capacity(24);
        for dy in -2i64..=2 {
            for dx in -2i64..=2 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (nx, ny) = (i64::from(x) + dx, i64::from(y) + dy);
                if nx < 0 || ny < 0 {
                    continue;
                }
                if let Some(q) = self.get(nx as u32, ny as u32) {
                    xs.push(q.x);
                    ys.push(q.y);
                }
            }
        }
        if xs.len() < 6 {
            return true;
        }
        let median = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        let scale = (f64::from(self.proj_w) / f64::from(self.cam_w))
            .max(f64::from(self.proj_h) / f64::from(self.cam_h))
            .max(1.0);
        let limit = OUTLIER * scale;
        (p.x - median(&mut xs)).abs() <= limit && (p.y - median(&mut ys)).abs() <= limit
    }

    pub fn invert(&self) -> ProjectorMap {
        let (pw, ph) = (self.proj_w as usize, self.proj_h as usize);
        // Every readable camera pixel that agrees with its neighbours: (projector pixel, camera
        // position).
        let mut samples: Vec<(usize, [f64; 2])> = Vec::new();
        for y in 0..self.cam_h {
            for x in 0..self.cam_w {
                let Some(p) = self.get(x, y) else { continue };
                if !self.agrees(x, y, p) {
                    continue;
                }
                let (px, py) = (p.x.floor() as i64, p.y.floor() as i64);
                if px >= 0 && py >= 0 && (px as usize) < pw && (py as usize) < ph {
                    let at = py as usize * pw + px as usize;
                    samples.push((at, [f64::from(x) + 0.5, f64::from(y) + 0.5]));
                }
            }
        }
        // Light bounced off nearby walls decodes to projector pixels too, from far across the
        // camera's picture. Within each block of projector pixels, keep only the samples close
        // to the block's median camera position.
        let (bw, bh) = (pw.div_ceil(BLOCK), ph.div_ceil(BLOCK));
        let block = |at: usize| ((at / pw) / BLOCK) * bw + (at % pw) / BLOCK;
        let mut per_block: Vec<(Vec<f64>, Vec<f64>)> = vec![(Vec::new(), Vec::new()); bw * bh];
        for (at, c) in &samples {
            let b = &mut per_block[block(*at)];
            b.0.push(c[0]);
            b.1.push(c[1]);
        }
        let medians: Vec<Option<[f64; 2]>> = per_block
            .into_iter()
            .map(|(mut xs, mut ys)| {
                if xs.is_empty() {
                    return None;
                }
                xs.sort_by(f64::total_cmp);
                ys.sort_by(f64::total_cmp);
                Some([xs[xs.len() / 2], ys[ys.len() / 2]])
            })
            .collect();
        // A block spans this many camera pixels, give or take: samples further than a few
        // blocks' worth from the median came from somewhere else.
        let span = (BLOCK as f64 * f64::from(self.cam_w) / f64::from(self.proj_w))
            .max(BLOCK as f64 * f64::from(self.cam_h) / f64::from(self.proj_h));
        let reach = CONSENSUS * span.max(1.0);
        let mut sum = vec![[0.0f64; 3]; pw * ph];
        for (at, c) in samples {
            let Some(m) = medians[block(at)] else {
                continue;
            };
            if (c[0] - m[0]).abs() > reach || (c[1] - m[1]).abs() > reach {
                continue;
            }
            let s = &mut sum[at];
            s[0] += c[0];
            s[1] += c[1];
            s[2] += 1.0;
        }
        let cam = push_pull(&sum, pw, ph);
        ProjectorMap {
            proj_w: self.proj_w,
            proj_h: self.proj_h,
            cam,
        }
    }
    /// Encodes the map for an RGBA16 PNG.
    pub fn to_rgba16(&self) -> Vec<u16> {
        self.proj
            .iter()
            .zip(&self.confidence)
            .flat_map(|(p, &c)| {
                if p[0].is_finite() {
                    [
                        ((p[0] / self.proj_w as f32) * 65535.0).clamp(0.0, 65535.0) as u16,
                        ((p[1] / self.proj_h as f32) * 65535.0).clamp(0.0, 65535.0) as u16,
                        u16::from(c) * 257,
                        65535,
                    ]
                } else {
                    [0, 0, 0, 0]
                }
            })
            .collect()
    }
    /// Decodes an RGBA16 map.
    pub fn from_rgba16(
        cam_w: u32,
        cam_h: u32,
        proj_w: u32,
        proj_h: u32,
        data: &[u16],
    ) -> Option<Self> {
        if data.len() != cam_w as usize * cam_h as usize * 4 {
            return None;
        }
        let mut proj = Vec::with_capacity(data.len() / 4);
        let mut confidence = Vec::with_capacity(data.len() / 4);
        for p in data.chunks_exact(4) {
            if p[3] == 0 {
                proj.push([f32::NAN; 2]);
                confidence.push(0)
            } else {
                proj.push([
                    p[0] as f32 * proj_w as f32 / 65535.0,
                    p[1] as f32 * proj_h as f32 / 65535.0,
                ]);
                confidence.push((p[2] / 257) as u8)
            }
        }
        Some(Self {
            cam_w,
            cam_h,
            proj_w,
            proj_h,
            proj,
            confidence,
        })
    }
    /// Removes isolated decode outliers.
    pub fn median_filter(&mut self) {
        if self.cam_w < 3 || self.cam_h < 3 {
            return;
        }
        let old = self.proj.clone();
        for y in 1..self.cam_h - 1 {
            for x in 1..self.cam_w - 1 {
                let i = y as usize * self.cam_w as usize + x as usize;
                if !old[i][0].is_finite() {
                    continue;
                }
                let mut xs = Vec::with_capacity(9);
                let mut ys = Vec::with_capacity(9);
                for yy in y - 1..=y + 1 {
                    for xx in x - 1..=x + 1 {
                        let p = old[yy as usize * self.cam_w as usize + xx as usize];
                        if p[0].is_finite() {
                            xs.push(p[0]);
                            ys.push(p[1]);
                        }
                    }
                }
                if xs.len() < 5 {
                    self.proj[i] = [f32::NAN; 2];
                    self.confidence[i] = 0;
                    continue;
                }
                xs.sort_by(f32::total_cmp);
                ys.sort_by(f32::total_cmp);
                let m = [xs[xs.len() / 2], ys[ys.len() / 2]];
                let dx = old[i][0] - m[0];
                let dy = old[i][1] - m[1];
                if dx * dx + dy * dy > 16.0 {
                    self.proj[i] = m;
                    self.confidence[i] /= 2;
                }
            }
        }
    }
}
/// Camera coordinates for every projector pixel.
#[derive(Debug, Clone)]
pub struct ProjectorMap {
    pub proj_w: u32,
    pub proj_h: u32,
    pub cam: Vec<[f32; 2]>,
}
impl ProjectorMap {
    /// Reads a valid projector pixel.
    pub fn get(&self, x: u32, y: u32) -> Option<Vec2> {
        if x >= self.proj_w || y >= self.proj_h {
            return None;
        }
        let p = self.cam[y as usize * self.proj_w as usize + x as usize];
        (p[0].is_finite() && p[1].is_finite()).then(|| Vec2::new(f64::from(p[0]), f64::from(p[1])))
    }
    /// Samples at projector coordinates.
    pub fn sample(&self, p: Vec2) -> Option<Vec2> {
        self.get(
            p.x.round()
                .clamp(0.0, f64::from(self.proj_w.saturating_sub(1))) as u32,
            p.y.round()
                .clamp(0.0, f64::from(self.proj_h.saturating_sub(1))) as u32,
        )
    }
    /// Fraction of projector pixels visible to the camera.
    pub fn coverage(&self) -> f64 {
        if self.cam.is_empty() {
            0.0
        } else {
            self.cam.iter().filter(|p| p[0].is_finite()).count() as f64 / self.cam.len() as f64
        }
    }
    /// Bounding footprint in camera pixels.
    pub fn footprint(&self) -> Vec<Vec2> {
        let v: Vec<_> = self.cam.iter().filter(|p| p[0].is_finite()).collect();
        if v.is_empty() {
            return Vec::new();
        }
        let minx = v.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
        let maxx = v.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
        let miny = v.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        let maxy = v.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
        vec![
            Vec2::new(minx.into(), miny.into()),
            Vec2::new(maxx.into(), miny.into()),
            Vec2::new(maxx.into(), maxy.into()),
            Vec2::new(minx.into(), maxy.into()),
        ]
    }
    /// Projector edges not fully visible in the camera.
    pub fn cut_off_edges(&self, cam_w: u32, cam_h: u32) -> Vec<Edge> {
        let outside = |p: Option<Vec2>| {
            p.is_none_or(|q| {
                q.x < 0.0 || q.y < 0.0 || q.x >= f64::from(cam_w) || q.y >= f64::from(cam_h)
            })
        };
        let mut r = Vec::new();
        if (0..self.proj_h).any(|y| outside(self.get(0, y))) {
            r.push(Edge::Left)
        }
        if (0..self.proj_h).any(|y| outside(self.get(self.proj_w.saturating_sub(1), y))) {
            r.push(Edge::Right)
        }
        if (0..self.proj_w).any(|x| outside(self.get(x, 0))) {
            r.push(Edge::Top)
        }
        if (0..self.proj_w).any(|x| outside(self.get(x, self.proj_h.saturating_sub(1)))) {
            r.push(Edge::Bottom)
        }
        r
    }
}

/// Side, in projector pixels, of the blocks the projection's outline is judged in: a camera that
/// sees the projector at a third of its resolution leaves samples a few pixels apart, and stripes
/// it cannot resolve snap them further, so the outline is decided at this coarser scale.
const BLOCK: usize = 16;

/// How many blocks across an unreadable gap inside the projection may be.
const SPAN: i64 = 10;

/// How many blocks' span (in camera pixels) a sample may sit from its block's median camera
/// position before it counts as stray light.
const CONSENSUS: f64 = 2.0;

/// How far, in camera pixels' worth of projector pixels, a reading may sit from its neighbours'
/// median before it counts as a misread.
const OUTLIER: f64 = 6.0;

/// The coarser level's value at a finer pixel: bilinear between the four nearest coarse
/// pixels, so a filled gap is a smooth ramp rather than steps; unfilled ones are left out.
fn upsample(coarse: &[[f64; 2]], w: usize, h: usize, x: usize, y: usize) -> [f64; 2] {
    let fx = ((x as f64 + 0.5) / 2.0 - 0.5).clamp(0.0, (w - 1) as f64);
    let fy = ((y as f64 + 0.5) / 2.0 - 0.5).clamp(0.0, (h - 1) as f64);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
    let mut acc = [0.0; 2];
    let mut weight = 0.0;
    for (cx, cy, wgt) in [
        (x0, y0, (1.0 - tx) * (1.0 - ty)),
        (x1, y0, tx * (1.0 - ty)),
        (x0, y1, (1.0 - tx) * ty),
        (x1, y1, tx * ty),
    ] {
        let v = coarse[cy * w + cx];
        if wgt > 0.0 && v[0].is_finite() {
            acc[0] += v[0] * wgt;
            acc[1] += v[1] * wgt;
            weight += wgt;
        }
    }
    if weight > 0.0 {
        [acc[0] / weight, acc[1] / weight]
    } else {
        coarse[(y / 2).min(h - 1) * w + (x / 2).min(w - 1)]
    }
}

/// Fills the gaps between scattered samples (`[sum x, sum y, count]` per pixel) with a push-pull
/// pyramid, inside the projection only: a pixel is filled when its block, or most of the blocks
/// around it, holds a sample. Unseen pixels stay NaN.
fn push_pull(sum: &[[f64; 3]], w: usize, h: usize) -> Vec<[f32; 2]> {
    // Push: average down to 1x1.
    let mut levels: Vec<(usize, usize, Vec<[f64; 3]>)> = vec![(w, h, sum.to_vec())];
    while let Some((lw, lh, data)) = levels.last() {
        if *lw <= 1 && *lh <= 1 {
            break;
        }
        let (nw, nh) = (lw.div_ceil(2), lh.div_ceil(2));
        let mut next = vec![[0.0; 3]; nw * nh];
        for y in 0..*lh {
            for x in 0..*lw {
                let s = data[y * lw + x];
                let n = &mut next[(y / 2) * nw + x / 2];
                n[0] += s[0];
                n[1] += s[1];
                n[2] += s[2];
            }
        }
        levels.push((nw, nh, next));
    }
    // Pull: each level fills its empty pixels from the (already filled) level above.
    let mut filled: Vec<[f64; 2]> = Vec::new();
    let (mut fw, mut fh) = (0usize, 0usize);
    for (lw, lh, data) in levels.iter().rev() {
        let mut out = vec![[f64::NAN; 2]; lw * lh];
        for y in 0..*lh {
            for x in 0..*lw {
                let s = data[y * lw + x];
                out[y * lw + x] = if s[2] > 0.0 {
                    [s[0] / s[2], s[1] / s[2]]
                } else if fw > 0 {
                    upsample(&filled, fw, fh, x, y)
                } else {
                    [f64::NAN; 2]
                };
            }
        }
        filled = out;
        fw = *lw;
        fh = *lh;
    }
    // Inside: blocks that hold samples, closed so a few empty blocks inside count too.
    let (bw, bh) = (w.div_ceil(BLOCK), h.div_ceil(BLOCK));
    let mut has = vec![false; bw * bh];
    for y in 0..h {
        for x in 0..w {
            if sum[y * w + x][2] > 0.0 {
                has[(y / BLOCK) * bw + x / BLOCK] = true;
            }
        }
    }
    // A block is inside when it holds samples, or when blocks holding samples lie on both sides
    // of it (above and below, or left and right) within `SPAN` blocks: a strip the camera could
    // not read is inside, the dark beyond the projection's edge is not.
    let at = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && (x as usize) < bw
            && (y as usize) < bh
            && has[y as usize * bw + x as usize]
    };
    let side =
        |bx: i64, by: i64, dx: i64, dy: i64| (1..=SPAN).any(|k| at(bx + dx * k, by + dy * k));
    let inside: Vec<bool> = (0..bh)
        .flat_map(|by| (0..bw).map(move |bx| (bx, by)))
        .map(|(bx, by)| {
            let (x, y) = (bx as i64, by as i64);
            at(x, y)
                || (side(x, y, 0, -1) && side(x, y, 0, 1))
                || (side(x, y, -1, 0) && side(x, y, 1, 0))
        })
        .collect();
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| {
            let v = filled[y * w + x];
            if inside[(y / BLOCK) * bw + x / BLOCK] && v[0].is_finite() {
                [v[0] as f32, v[1] as f32]
            } else {
                [f32::NAN; 2]
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "correspondence_tests.rs"]
mod tests;
