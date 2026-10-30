//! Motion and presence zones.
use crate::Gray;
use lumap_core::geom::Vec2;
/// Frame-difference motion detector.
pub struct MotionDetector {
    previous: Option<Gray>,
    motion: Gray,
}
impl MotionDetector {
    /// Creates a detector.
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            previous: None,
            motion: Gray::new(w, h),
        }
    }
    /// Updates from a camera frame.
    pub fn update(&mut self, f: &Gray) {
        if let Some(old) = &self.previous {
            self.motion = Gray::from_fn(f.width, f.height, |x, y| {
                f.get(x, y).abs_diff(old.get(x, y))
            });
        }
        self.previous = Some(f.clone())
    }
    /// Motion amount inside a normalised polygon.
    pub fn zone(&self, poly: &[Vec2], sensitivity: f64) -> f64 {
        if poly.is_empty() {
            return 0.0;
        }
        let minx = poly.iter().map(|p| p.x).fold(1.0, f64::min);
        let maxx = poly.iter().map(|p| p.x).fold(0.0, f64::max);
        let miny = poly.iter().map(|p| p.y).fold(1.0, f64::min);
        let maxy = poly.iter().map(|p| p.y).fold(0.0, f64::max);
        let (mut sum, mut n) = (0u64, 0u64);
        for y in 0..self.motion.height {
            for x in 0..self.motion.width {
                let (nx, ny) = (
                    (f64::from(x) + 0.5) / f64::from(self.motion.width),
                    (f64::from(y) + 0.5) / f64::from(self.motion.height),
                );
                if nx >= minx && nx <= maxx && ny >= miny && ny <= maxy {
                    sum += u64::from(self.motion.get(x, y));
                    n += 1
                }
            }
        }
        (sum as f64 / f64::from((n.max(1)) as u32) / 32.0 * sensitivity).clamp(0.0, 1.0)
    }
    /// Debug motion image.
    pub fn motion_image(&self) -> Gray {
        self.motion.clone()
    }
}
