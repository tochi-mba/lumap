//! Edges, corners, contours and polygon simplification: the image primitives that the surface
//! finder, the facets, the magic wand and snapping are built from.
//!
//! Every position is in continuous pixel coordinates: the centre of pixel `(x, y)` is
//! `(x + 0.5, y + 0.5)`, so for a projector's-eye photo a point is a projector pixel position.
//! Outlines run clockwise on screen (y points down), which is a positive shoelace sum.

mod contours;
mod corners;
mod filters;
mod hough;
mod polygon;
pub(crate) mod prepared;
#[cfg(test)]
pub(crate) mod scene;
pub(crate) mod shapes;
#[cfg(test)]
mod tests;

pub use contours::contours;
pub(crate) use contours::{centres, label_regions, trace};
pub use corners::corners;
pub use filters::{blur, canny, sobel};
pub use hough::hough_circles;
pub(crate) use polygon::{centroid, point_in_polygon, signed_area2};
pub use polygon::{convex_hull, polygon_area, simplify};
pub use shapes::{fit_circle, order_quad};
