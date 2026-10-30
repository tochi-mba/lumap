//! Warp photographs between camera and projector views.
use crate::{
    correspondence::{Correspondence, ProjectorMap},
    Rgba,
};
/// Camera photograph resampled in projector pixels.
pub fn projector_eye(map: &ProjectorMap, photo: &Rgba) -> Rgba {
    Rgba::from_fn(map.proj_w, map.proj_h, |x, y| {
        map.get(x, y).map_or([0; 4], |p| {
            let cx =
                p.x.round()
                    .clamp(0.0, f64::from(photo.width.saturating_sub(1))) as u32;
            let cy =
                p.y.round()
                    .clamp(0.0, f64::from(photo.height.saturating_sub(1))) as u32;
            photo.get(cx, cy)
        })
    })
}
/// Projector image resampled in camera pixels.
pub fn camera_eye(corr: &Correspondence, image: &Rgba) -> Rgba {
    Rgba::from_fn(corr.cam_w, corr.cam_h, |x, y| {
        corr.get(x, y).map_or([0; 4], |p| {
            image.get(
                p.x.floor()
                    .clamp(0.0, f64::from(image.width.saturating_sub(1))) as u32,
                p.y.floor()
                    .clamp(0.0, f64::from(image.height.saturating_sub(1))) as u32,
            )
        })
    })
}
