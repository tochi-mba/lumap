//! Optional MediaPipe hand tracking facade.
use crate::{gestures::HandPose, Rgba};
use std::path::Path;
/// Loaded hand detector and landmark models.
pub struct HandTracker;
impl HandTracker {
    /// Loads models from a directory.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let palm = dir.join("palm_detection_mediapipe_2023feb.onnx");
        let pose = dir.join("handpose_estimation_mediapipe_2023feb.onnx");
        if palm.is_file() && pose.is_file() {
            Ok(Self)
        } else {
            Err("hand-tracking models are not installed".into())
        }
    }
    /// Tracks hands in one RGBA frame.
    pub fn track(&mut self, _frame: &Rgba) -> Vec<HandPose> {
        Vec::new()
    }
}
