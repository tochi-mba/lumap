//! Webcams: list the cameras the system has and stream one as frames.
//!
//! The default backend runs the bundled `ffmpeg` as a sibling process (the same way videos are
//! decoded): DirectShow on Windows, AVFoundation on macOS, Video4Linux on Linux. This is
//! platform code, so it lives here in `live_input` and nowhere else.

use crate::{FrameSource, SourceError};

/// Opens a camera as a live source. Filled in by the camera work.
pub fn open_source(id: &str, device: &str) -> Result<Box<dyn FrameSource>, SourceError> {
    let _ = device;
    Err(SourceError::Unavailable(id.to_string(), "camera"))
}
