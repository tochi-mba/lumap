//! Scanning the room. Each projector in turn shows Gray-code stripes while the camera watches;
//! decoding the photos tells lumap where every projector pixel lands on the wall. From that come
//! the projector's-eye photo under the Map canvas, the surfaces lumap finds, corners that snap
//! to real edges, measured edge blends, and re-alignment after a projector is bumped.
//!
//! A scan runs as a small state machine ticked once per frame: show a pattern, wait for the
//! camera to see it and settle, take the median of a few frames, move on. Decoding and the
//! analysis afterwards run on worker threads, so the editor never waits.

mod data;
mod session;

pub use data::{ScanData, ScanStore};
pub use session::{DriftReport, Kind, Phase, Progress, ScanSession, CAMERA_MOVED};

/// Where a show's scans live: `<show>.scan/` beside the project file, or a temporary folder for
/// a show that has not been saved yet (moved beside it on the first save).
pub fn scan_root(project: Option<&std::path::Path>) -> std::path::PathBuf {
    match project {
        Some(p) => {
            let stem = p.file_name().and_then(|n| n.to_str()).map_or("show", |n| {
                n.trim_end_matches(".toml").trim_end_matches(".lumap")
            });
            p.with_file_name(format!("{stem}.scan"))
        }
        None => std::env::temp_dir()
            .join("lumap-scans")
            .join(format!("unsaved-{}", std::process::id())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn scans_live_beside_the_show_or_in_a_temporary_folder() {
        let root = scan_root(Some(Path::new("/shows/gala.lumap.toml")));
        assert!(root.ends_with("gala.scan"), "{}", root.display());
        let unsaved = scan_root(None);
        assert!(unsaved.to_string_lossy().contains("lumap-scans"));
    }
}
