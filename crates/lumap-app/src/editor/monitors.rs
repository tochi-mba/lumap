//! Live monitor discovery. winit queries the OS each time, so attaching or removing a
//! projector updates the same monitor list used by Home, Setup and output placement.

use super::MonitorInfo;
use std::time::{Duration, Instant};

/// Refresh even if the editor is idle; the OS does not always repaint on hot-plug.
pub(super) const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Query the current desktop topology using the editor's existing native window.
pub(super) fn enumerate(window: &winit::window::Window) -> Vec<MonitorInfo> {
    let mut handles: Vec<_> = window.available_monitors().collect();
    // Setup skips the first display when creating projectors. Keep the display that
    // currently hosts the editor first, not whichever order the OS enumerates.
    if let Some(active) = window.current_monitor()
        && let Some(index) = handles.iter().position(|m| m == &active)
    {
        handles.swap(0, index);
    }
    handles
        .into_iter()
        .map(|m| MonitorInfo {
            name: m.name().unwrap_or_default(),
            x: m.position().x,
            y: m.position().y,
            width: m.size().width,
            height: m.size().height,
            scale: m.scale_factor(),
        })
        .collect()
}

/// Apply a complete OS snapshot. Ignore a transient empty enumeration rather than
/// hiding known displays, and leave projectors/show content untouched.
pub(super) fn apply_snapshot(known: &mut Vec<MonitorInfo>, detected: Vec<MonitorInfo>) -> bool {
    if detected.is_empty() || *known == detected {
        return false;
    }
    log::info!(
        "connected displays changed: {} -> {}",
        known.len(),
        detected.len()
    );
    *known = detected;
    true
}

/// Throttles native enumeration; called from eframe's UI on the main thread.
pub(super) struct Poller {
    last_check: Instant,
}

impl Default for Poller {
    fn default() -> Self {
        Self {
            last_check: Instant::now(),
        }
    }
}

impl Poller {
    /// Keep the monitor snapshot fresh without spawning another event loop.
    pub(super) fn poll(
        &mut self,
        window: Option<&winit::window::Window>,
        known: &mut Vec<MonitorInfo>,
        now: Instant,
    ) {
        if now.saturating_duration_since(self.last_check) < POLL_INTERVAL {
            return;
        }
        self.last_check = now;
        if let Some(window) = window {
            apply_snapshot(known, enumerate(window));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(name: &str, width: u32) -> MonitorInfo {
        MonitorInfo {
            name: name.into(),
            x: 0,
            y: 0,
            width,
            height: 1080,
            scale: 1.0,
        }
    }

    #[test]
    fn hotplug_and_unplug_change_the_live_snapshot() {
        let primary = display("DISPLAY1", 1920);
        let projector = display("DISPLAY2", 1280);
        let mut known = vec![primary.clone()];
        assert!(!apply_snapshot(&mut known, vec![primary.clone()]));
        assert!(apply_snapshot(
            &mut known,
            vec![primary.clone(), projector.clone()]
        ));
        assert_eq!(known, [primary.clone(), projector]);
        assert!(apply_snapshot(&mut known, vec![primary.clone()]));
        assert_eq!(known, [primary]);
    }

    #[test]
    fn geometry_changes_are_seen_and_transient_empty_queries_are_ignored() {
        let mut known = vec![display("DISPLAY1", 1920)];
        assert!(!apply_snapshot(&mut known, Vec::new()));
        assert_eq!(known[0].width, 1920);
        assert!(apply_snapshot(&mut known, vec![display("DISPLAY1", 2560)]));
        assert_eq!(known[0].width, 2560);
    }

    #[test]
    fn poller_does_not_clear_displays_without_a_native_window() {
        let mut poller = Poller::default();
        let mut known = vec![display("DISPLAY1", 1920)];
        poller.poll(
            None,
            &mut known,
            Instant::now() + POLL_INTERVAL,
        );
        assert_eq!(known.len(), 1);
    }
}
