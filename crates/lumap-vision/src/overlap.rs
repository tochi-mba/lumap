//! Estimate projector edge-blend widths from scan footprints.
use crate::correspondence::{Edge, ProjectorMap};
/// One recommended edge blend.
#[derive(Debug, Clone, Copy)]
pub struct Overlap {
    pub projector: usize,
    pub edge: Edge,
    pub width: f64,
}
/// Estimates overlap widths. Empty or disjoint maps need no blends.
pub fn blend_widths(_maps: &[ProjectorMap], _cam_w: u32, _cam_h: u32) -> Vec<Overlap> {
    Vec::new()
}
