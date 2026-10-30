//! A deterministic synthetic room and camera for proving scans without hardware.
use crate::{Gray, Rgba};
use lumap_core::geom::{Homography, Vec2};
/// One planar room face in projector coordinates.
#[derive(Debug, Clone)]
pub struct Face {
    pub outline: Vec<Vec2>,
    pub to_camera: Homography,
    pub albedo: f64,
}
/// Projected room geometry; later faces occlude earlier ones.
#[derive(Debug, Clone)]
pub struct Room {
    pub proj_w: u32,
    pub proj_h: u32,
    pub faces: Vec<Face>,
}
/// Synthetic camera properties.
#[derive(Debug, Clone, Copy)]
pub struct CameraModel {
    pub width: u32,
    pub height: u32,
    pub blur: f64,
    pub noise: f64,
    pub gamma: f64,
    pub ambient: f64,
    pub vignette: f64,
    pub seed: u64,
}
impl Default for CameraModel {
    fn default() -> Self {
        Self {
            width: 640,
            height: 360,
            blur: 0.6,
            noise: 1.5,
            gamma: 1.0,
            ambient: 12.0,
            vignette: 0.12,
            seed: 1,
        }
    }
}
impl Room {
    /// A mildly keystoned wall filling most of the camera view.
    pub fn flat_wall(pw: u32, ph: u32, cw: u32, ch: u32) -> Self {
        let src = quad(0.0, 0.0, f64::from(pw), f64::from(ph));
        let dst = [
            Vec2::new(0.09 * f64::from(cw), 0.12 * f64::from(ch)),
            Vec2::new(0.91 * f64::from(cw), 0.08 * f64::from(ch)),
            Vec2::new(0.88 * f64::from(cw), 0.91 * f64::from(ch)),
            Vec2::new(0.12 * f64::from(cw), 0.88 * f64::from(ch)),
        ];
        Self {
            proj_w: pw,
            proj_h: ph,
            faces: vec![Face {
                outline: src.to_vec(),
                to_camera: Homography::from_points(src, dst)
                    .unwrap_or_else(|_| Homography::identity()),
                albedo: 0.82,
            }],
        }
    }
    /// A wall with three differently shaded box faces.
    pub fn box_on_wall(pw: u32, ph: u32, cw: u32, ch: u32) -> Self {
        let mut r = Self::flat_wall(pw, ph, cw, ch);
        let x = f64::from(pw) * 0.34;
        let y = f64::from(ph) * 0.3;
        let w = f64::from(pw) * 0.28;
        let h = f64::from(ph) * 0.38;
        for (dx, dy, a) in [
            (0.0, 0.0, 0.62),
            (w * 0.12, -h * 0.09, 0.9),
            (-w * 0.09, h * 0.08, 0.72),
        ] {
            let q = quad(x + dx, y + dy, w, h);
            let d = q.map(|p| r.faces[0].to_camera.map(p).unwrap_or(p));
            r.faces.push(Face {
                outline: q.to_vec(),
                to_camera: Homography::from_points(q, d).unwrap_or(r.faces[0].to_camera),
                albedo: a,
            });
        }
        r
    }
    /// A wall split into two planes at a vertical crease.
    pub fn bent_wall(pw: u32, ph: u32, cw: u32, ch: u32) -> Self {
        let mid = f64::from(pw) * 0.5;
        let left = [
            Vec2::new(0.0, 0.0),
            Vec2::new(mid, 0.0),
            Vec2::new(mid, f64::from(ph)),
            Vec2::new(0.0, f64::from(ph)),
        ];
        let right = [
            Vec2::new(mid, 0.0),
            Vec2::new(f64::from(pw), 0.0),
            Vec2::new(f64::from(pw), f64::from(ph)),
            Vec2::new(mid, f64::from(ph)),
        ];
        let lc = [
            Vec2::new(0.08 * f64::from(cw), 0.1 * f64::from(ch)),
            Vec2::new(0.52 * f64::from(cw), 0.18 * f64::from(ch)),
            Vec2::new(0.52 * f64::from(cw), 0.82 * f64::from(ch)),
            Vec2::new(0.1 * f64::from(cw), 0.9 * f64::from(ch)),
        ];
        let rc = [
            lc[1],
            Vec2::new(0.92 * f64::from(cw), 0.08 * f64::from(ch)),
            Vec2::new(0.9 * f64::from(cw), 0.92 * f64::from(ch)),
            lc[2],
        ];
        Self {
            proj_w: pw,
            proj_h: ph,
            faces: vec![
                Face {
                    outline: left.to_vec(),
                    to_camera: Homography::from_points(left, lc)
                        .unwrap_or_else(|_| Homography::identity()),
                    albedo: 0.78,
                },
                Face {
                    outline: right.to_vec(),
                    to_camera: Homography::from_points(right, rc)
                        .unwrap_or_else(|_| Homography::identity()),
                    albedo: 0.9,
                },
            ],
        }
    }
}
fn quad(x: f64, y: f64, w: f64, h: f64) -> [Vec2; 4] {
    [
        Vec2::new(x, y),
        Vec2::new(x + w, y),
        Vec2::new(x + w, y + h),
        Vec2::new(x, y + h),
    ]
}
fn inside(p: Vec2, poly: &[Vec2]) -> bool {
    let mut yes = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            yes = !yes
        }
    }
    yes
}
/// Ground-truth projector point seen by a camera pixel.
pub fn truth(room: &Room, _cam: &CameraModel, cx: u32, cy: u32) -> Option<Vec2> {
    let c = Vec2::new(f64::from(cx) + 0.5, f64::from(cy) + 0.5);
    room.faces
        .iter()
        .rev()
        .find_map(|f| f.to_camera.unmap(c).filter(|&p| inside(p, &f.outline)))
}
/// Photographs projected greyscale light through the room geometry.
pub fn photograph(room: &Room, projected: &Gray, cam: &CameraModel) -> Rgba {
    let raw = Rgba::from_fn(cam.width, cam.height, |x, y| {
        let c = Vec2::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
        let face = room.faces.iter().rev().find_map(|f| {
            f.to_camera
                .unmap(c)
                .filter(|&p| inside(p, &f.outline))
                .map(|p| (f, p))
        });
        let Some((f, p)) = face else {
            return [cam.ambient.clamp(0.0, 255.0) as u8; 4];
        };
        let light = projected.sample(p.x, p.y) / 255.0;
        let nx = (c.x / f64::from(cam.width) - 0.5) * 2.0;
        let ny = (c.y / f64::from(cam.height) - 0.5) * 2.0;
        let vig = (1.0 - cam.vignette * (nx * nx + ny * ny)).max(0.0);
        let mut seed = cam.seed
            ^ u64::from(x).wrapping_mul(0x9e37_79b9)
            ^ u64::from(y).wrapping_mul(0x85eb_ca6b);
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let noise = ((seed >> 11) as f64 / (u64::MAX >> 11) as f64 - 0.5) * 2.0 * cam.noise;
        let v =
            (cam.ambient + 255.0 * (light * f.albedo).powf(1.0 / cam.gamma.max(0.1)) * vig + noise)
                .clamp(0.0, 255.0) as u8;
        [v, v, v, 255]
    });
    blur(&raw, cam.blur)
}

/// A small separable box blur models projector focus and camera optics. Fractional radii blend
/// the sharp and blurred images, so the default 0.6 is meaningfully different from both.
fn blur(image: &Rgba, radius: f64) -> Rgba {
    let whole = radius.ceil() as i32;
    if whole <= 0 {
        return image.clone();
    }
    let mut out = Rgba::new(image.width, image.height);
    let mix = radius.clamp(0.0, 1.0);
    for y in 0..image.height {
        for x in 0..image.width {
            let mut sum = [0u32; 3];
            let mut n = 0u32;
            for yy in (y as i32 - whole)..=(y as i32 + whole) {
                for xx in (x as i32 - whole)..=(x as i32 + whole) {
                    if xx >= 0 && yy >= 0 && xx < image.width as i32 && yy < image.height as i32 {
                        let i = (yy as usize * image.width as usize + xx as usize) * 4;
                        for c in 0..3 {
                            sum[c] += u32::from(image.data[i + c])
                        }
                        n += 1
                    }
                }
            }
            let i = (y as usize * image.width as usize + x as usize) * 4;
            for c in 0..3 {
                let average = f64::from(sum[c]) / f64::from(n);
                out.data[i + c] =
                    ((1.0 - mix) * f64::from(image.data[i + c]) + mix * average).round() as u8
            }
            out.data[i + 3] = 255
        }
    }
    out
}
