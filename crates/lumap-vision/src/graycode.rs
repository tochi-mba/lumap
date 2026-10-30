//! Gray-code pattern generation and decoding.
use crate::{correspondence::Correspondence, Gray};
/// Stripe axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}
/// One projected frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternFrame {
    White,
    Black,
    Bit {
        axis: Axis,
        bit: u32,
        inverted: bool,
    },
}
/// A pattern sequence.
#[derive(Debug, Clone)]
pub struct GrayCode {
    w: u32,
    h: u32,
    bx: u32,
    by: u32,
    sx: u32,
    sy: u32,
    frames: Vec<PatternFrame>,
}
fn bits(n: u32) -> u32 {
    if n <= 1 {
        0
    } else {
        32 - (n - 1).leading_zeros()
    }
}
impl GrayCode {
    /// Full-resolution sequence.
    pub fn new(w: u32, h: u32) -> Self {
        Self::build(w, h, bits(w), bits(h), 0, 0)
    }
    /// Coarse sequence using high bits.
    pub fn coarse(w: u32, h: u32, n: u32) -> Self {
        let (full_x, full_y) = (bits(w), bits(h));
        let (bx, by) = (full_x.min(n), full_y.min(n));
        Self::build(w, h, bx, by, full_x - bx, full_y - by)
    }
    fn build(w: u32, h: u32, bx: u32, by: u32, sx: u32, sy: u32) -> Self {
        let mut frames = vec![PatternFrame::White, PatternFrame::Black];
        for (axis, n, start) in [(Axis::X, bx, sx), (Axis::Y, by, sy)] {
            for bit in (start..start + n).rev() {
                frames.push(PatternFrame::Bit {
                    axis,
                    bit,
                    inverted: false,
                });
                frames.push(PatternFrame::Bit {
                    axis,
                    bit,
                    inverted: true,
                });
            }
        }
        Self {
            w,
            h,
            bx,
            by,
            sx,
            sy,
            frames,
        }
    }
    /// Frames in projection order.
    pub fn frames(&self) -> &[PatternFrame] {
        &self.frames
    }
    /// Renders into an existing buffer.
    pub fn render_into(&self, frame: &PatternFrame, out: &mut Gray) {
        *out = Gray::from_fn(self.w, self.h, |x, y| match *frame {
            PatternFrame::White => 255,
            PatternFrame::Black => 0,
            PatternFrame::Bit {
                axis,
                bit,
                inverted,
            } => {
                let v = match axis {
                    Axis::X => x,
                    Axis::Y => y,
                };
                let on = (((v ^ (v >> 1)) >> bit) & 1) != 0;
                if on ^ inverted {
                    255
                } else {
                    0
                }
            }
        });
    }
    /// Renders one pattern.
    pub fn render(&self, frame: &PatternFrame) -> Gray {
        let mut g = Gray::new(self.w, self.h);
        self.render_into(frame, &mut g);
        g
    }
}
/// Decoder thresholds.
#[derive(Debug, Clone, Copy)]
pub struct DecodeSettings {
    /// White-minus-black brightness a camera pixel needs to be lit by the projector at all.
    pub min_contrast: u8,
    /// Pattern-minus-inverse brightness a stripe needs to be read.
    pub min_bit_contrast: u8,
    /// Share of the pixel's own white-minus-black contrast a stripe must reach to be read. Light
    /// bounced off the room changes a little with every pattern; light from the projector
    /// changes by about the whole contrast.
    pub min_bit_ratio: f64,
    /// Stripes per axis that must be read, coarsest first, for the pixel to count at all.
    pub min_bits: u32,
}
impl Default for DecodeSettings {
    fn default() -> Self {
        Self {
            min_contrast: 20,
            min_bit_contrast: 6,
            min_bit_ratio: 0.3,
            min_bits: 3,
        }
    }
}
/// Quality summary.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScanStats {
    pub valid_fraction: f64,
    pub mean_contrast: f64,
    pub bits_x: u32,
    pub bits_y: u32,
}
/// Finished scan.
pub struct Scan {
    pub correspondence: Correspondence,
    pub white: Gray,
    pub black: Gray,
    pub stats: ScanStats,
}
/// Collects camera photographs.
pub struct Decoder {
    code: GrayCode,
    cw: u32,
    ch: u32,
    photos: Vec<Option<Gray>>,
}
impl Decoder {
    /// Creates a decoder.
    pub fn new(code: &GrayCode, cw: u32, ch: u32) -> Self {
        Self {
            code: code.clone(),
            cw,
            ch,
            photos: vec![None; code.frames.len()],
        }
    }
    /// Adds a frame in any order.
    pub fn add(&mut self, i: usize, p: &Gray) {
        if i < self.photos.len() && p.width == self.cw && p.height == self.ch {
            self.photos[i] = Some(p.clone())
        }
    }
    /// Whether every frame arrived.
    pub fn is_complete(&self) -> bool {
        self.photos.iter().all(Option::is_some)
    }
    /// Decodes all camera pixels.
    pub fn finish(&self, s: &DecodeSettings) -> Scan {
        let white = self
            .photos
            .first()
            .and_then(Clone::clone)
            .unwrap_or_else(|| Gray::new(self.cw, self.ch));
        let black = self
            .photos
            .get(1)
            .and_then(Clone::clone)
            .unwrap_or_else(|| Gray::new(self.cw, self.ch));
        // A pattern and its inverse each light half the projection, so they should be equally
        // bright overall. A camera that changes its exposure between them (a phone streamed as a
        // webcam does, all the time) makes one darker; scale each inverse to its pattern.
        let means: Vec<f64> = self
            .photos
            .iter()
            .map(|p| {
                p.as_ref().map_or(0.0, |g| {
                    g.data.iter().map(|&v| f64::from(v)).sum::<f64>() / g.data.len().max(1) as f64
                })
            })
            .collect();
        // Only patterns that really light about half the projector can be matched that way:
        // with 1080 rows in an 11-bit code, the coarsest row stripe lights just the bottom 56.
        let lit = |axis: Axis, bit: u32| {
            let n = match axis {
                Axis::X => self.code.w,
                Axis::Y => self.code.h,
            };
            let on = (0..n).filter(|v| ((v ^ (v >> 1)) >> bit) & 1 != 0).count();
            on as f64 / f64::from(n.max(1))
        };
        let mut balanced = vec![false; self.photos.len()];
        let mut slot = 2;
        for (axis, n, start) in [
            (Axis::X, self.code.bx, self.code.sx),
            (Axis::Y, self.code.by, self.code.sy),
        ] {
            for bit in (start..start + n).rev() {
                if let Some(b) = balanced.get_mut(slot) {
                    *b = (0.4..=0.6).contains(&lit(axis, bit));
                }
                slot += 2;
            }
        }
        let gain = |at: usize| {
            let (a, b) = (
                means.get(at).copied().unwrap_or(0.0),
                means.get(at + 1).copied().unwrap_or(0.0),
            );
            if balanced.get(at).copied().unwrap_or(false) && a > 1.0 && b > 1.0 {
                (a / b).clamp(0.25, 4.0)
            } else {
                1.0
            }
        };
        let mut proj = vec![[f32::NAN; 2]; self.cw as usize * self.ch as usize];
        let mut confidence = vec![0; proj.len()];
        let (mut valid, mut sum) = (0usize, 0.0);
        for i in 0..proj.len() {
            let contrast = white.data[i].saturating_sub(black.data[i]);
            sum += f64::from(contrast);
            if contrast < s.min_contrast {
                continue;
            }
            let (mut gx, mut gy, mut at) = (0, 0, 2);
            let need = (f64::from(contrast) * s.min_bit_ratio)
                .max(f64::from(s.min_bit_contrast))
                .round() as u8;
            let mut readable = true;
            for (axis, n, start) in [
                (Axis::X, self.code.bx, self.code.sx),
                (Axis::Y, self.code.by, self.code.sy),
            ] {
                let mut g = 0u32;
                let mut unknown = start;
                for bit in (start..start + n).rev() {
                    let a = self
                        .photos
                        .get(at)
                        .and_then(Option::as_ref)
                        .map_or(0, |p| p.data[i]);
                    let b = self
                        .photos
                        .get(at + 1)
                        .and_then(Option::as_ref)
                        .map_or(0, |p| p.data[i]);
                    let b = (f64::from(b) * gain(at)).round().clamp(0.0, 255.0) as u8;
                    at += 2;
                    if a.abs_diff(b) < need {
                        unknown = bit + 1;
                        at += 2 * (bit - start) as usize;
                        break;
                    }
                    if a > b {
                        g |= 1 << bit
                    }
                }
                let mut bin = g;
                let mut shift = g >> 1;
                while shift != 0 {
                    bin ^= shift;
                    shift >>= 1
                }
                // Too few stripes read: this is not a pixel the projector lights directly.
                if unknown - start + s.min_bits.min(n) > n {
                    readable = false;
                }
                if unknown > 0 {
                    let low = (1u32 << unknown) - 1;
                    bin &= !low;
                    bin += 1u32 << (unknown - 1);
                }
                match axis {
                    Axis::X => gx = bin,
                    Axis::Y => gy = bin,
                }
            }
            if readable && gx < self.code.w && gy < self.code.h {
                proj[i] = [gx as f32 + 0.5, gy as f32 + 0.5];
                confidence[i] = contrast;
                valid += 1
            }
        }
        let correspondence = Correspondence {
            cam_w: self.cw,
            cam_h: self.ch,
            proj_w: self.code.w,
            proj_h: self.code.h,
            proj,
            confidence,
        };
        let total = correspondence.proj.len().max(1);
        Scan {
            correspondence,
            white,
            black,
            stats: ScanStats {
                valid_fraction: valid as f64 / total as f64,
                mean_contrast: sum / total as f64,
                bits_x: self.code.bx,
                bits_y: self.code.by,
            },
        }
    }
}
