//! Computer vision for lumap: structured-light scans of the room, finding surfaces on the
//! wall, tracking lights, hands, motion and shadows, and the room effects drawn from them.
//! Pure and deterministic: no camera, no threads, no I/O; the app feeds images in.

#![forbid(unsafe_code)]
#![allow(missing_docs, dead_code, unused_imports, clippy::pedantic)]

pub mod correspondence;
pub mod facets;
pub mod features;
pub mod framing;
pub mod find;
pub mod gestures;
pub mod graycode;
#[cfg(feature = "hands")]
pub mod hands;
pub mod homography;
pub mod image;
pub mod letters;
pub mod motion;
pub mod overlap;
pub mod paint;
pub mod photo;
pub mod physics;
pub mod realign;
pub mod shadow;
pub mod snap;
pub mod spots;
pub mod studio;
pub mod trace;
pub mod wand;

pub use image::{Gray, Rgba};
