//! Fast snapping to photographed corners and edges.
use crate::{features, Gray, Rgba};
use lumap_core::geom::Vec2;
/// The feature snapped to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind {
    Corner,
    Edge,
}
/// Searchable feature set.
#[derive(Debug, Clone)]
pub struct SnapIndex {
    corners: Vec<Vec2>,
    edges: Vec<Vec2>,
}
impl SnapIndex {
    /// Builds a feature index.
    pub fn build(photo: &Rgba) -> Self {
        let g = Gray::from_rgba(photo);
        let corners = features::corners(&g, 512, 0.02, 5.0);
        let e = features::canny(&g, 25.0, 70.0);
        let mut edges = Vec::new();
        for y in 0..e.height {
            for x in 0..e.width {
                if e.get(x, y) > 0 {
                    edges.push(Vec2::new(f64::from(x) + 0.5, f64::from(y) + 0.5));
                }
            }
        }
        Self { corners, edges }
    }
    /// Finds the nearest corner, then edge.
    pub fn nearest(&self, p: Vec2, r: f64) -> Option<(Vec2, SnapKind)> {
        let best = |v: &[Vec2], limit: f64| {
            v.iter()
                .copied()
                .map(|q| (q, (q.x - p.x).hypot(q.y - p.y)))
                .filter(|(_, d)| *d <= limit)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(q, _)| q)
        };
        best(&self.corners, r)
            .map(|q| (q, SnapKind::Corner))
            .or_else(|| best(&self.edges, r * 0.6).map(|q| (q, SnapKind::Edge)))
    }
}
