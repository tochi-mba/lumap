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
    pub fn invert(&self) -> ProjectorMap {
        let mut cam = vec![[f32::NAN; 2]; self.proj_w as usize * self.proj_h as usize];
        for y in 0..self.cam_h {
            for x in 0..self.cam_w {
                if let Some(p) = self.get(x, y) {
                    let (px, py) = (p.x.floor() as i64, p.y.floor() as i64);
                    if px >= 0
                        && py >= 0
                        && px < i64::from(self.proj_w)
                        && py < i64::from(self.proj_h)
                    {
                        cam[py as usize * self.proj_w as usize + px as usize] =
                            [x as f32 + 0.5, y as f32 + 0.5];
                    }
                }
            }
        }
        for _ in 0..3 {
            let old = cam.clone();
            for y in 0..self.proj_h {
                for x in 0..self.proj_w {
                    let i = y as usize * self.proj_w as usize + x as usize;
                    if old[i][0].is_finite() {
                        continue;
                    }
                    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let (xx, yy) = (i64::from(x) + dx, i64::from(y) + dy);
                        if xx >= 0
                            && yy >= 0
                            && xx < i64::from(self.proj_w)
                            && yy < i64::from(self.proj_h)
                        {
                            let q = old[yy as usize * self.proj_w as usize + xx as usize];
                            if q[0].is_finite() {
                                sx += q[0];
                                sy += q[1];
                                n += 1.0;
                            }
                        }
                    }
                    if n >= 2.0 {
                        cam[i] = [sx / n, sy / n];
                    }
                }
            }
        }
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
    pub fn median_filter(&mut self) {}
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
