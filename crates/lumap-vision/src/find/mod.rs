//! Find surfaces: ranked polygons a projector could map onto.
use crate::Rgba;
use lumap_core::geom::Vec2;
/// Shape of a suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionKind {
    Quad,
    Polygon,
    Circle,
}
/// A ranked mapping suggestion.
#[derive(Debug, Clone)]
pub struct Suggestion {
    pub kind: SuggestionKind,
    pub points: Vec<Vec2>,
    pub score: f64,
    pub label: String,
}
/// Surface-finder limits.
#[derive(Debug, Clone, Copy)]
pub struct FindSettings {
    pub max: usize,
    pub min_area_fraction: f64,
    pub max_area_fraction: f64,
}
impl Default for FindSettings {
    fn default() -> Self {
        Self {
            max: 12,
            min_area_fraction: 0.002,
            max_area_fraction: 0.85,
        }
    }
}
/// Finds flat photographed regions, ranked by area and flatness.
pub fn find_surfaces(
    _photo: &Rgba,
    _corr: Option<&[[f32; 2]]>,
    _s: &FindSettings,
) -> Vec<Suggestion> {
    Vec::new()
}
