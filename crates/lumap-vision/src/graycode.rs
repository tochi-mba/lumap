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
    pub min_contrast: u8,
    pub min_bit_contrast: u8,
}
impl Default for DecodeSettings {
    fn default() -> Self {
        Self {
            min_contrast: 20,
            min_bit_contrast: 6,
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
                    at += 2;
                    if a.abs_diff(b) < s.min_bit_contrast {
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
            if gx < self.code.w && gy < self.code.h {
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
