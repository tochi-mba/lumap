//! Desktop integration, monitor geometry and scene helpers.
use super::{Editor, MonitorInfo};
use lumap_core::project::Project;

/// Points the video decoder at a user-chosen ffmpeg binary (empty restores the PATH lookup).
pub fn apply_ffmpeg_path(path: &str) {
    if path.trim().is_empty() {
        std::env::remove_var("LUMAP_FFMPEG");
    } else {
        std::env::set_var("LUMAP_FFMPEG", path.trim());
    }
}

/// Converts physical desktop pixels to egui points for a viewport on a monitor with `scale`.
pub fn physical_to_points(px: i32, scale: f64) -> f32 {
    (f64::from(px) / scale.max(0.01)) as f32
}

/// Chooses the monitor whose area contains `(x, y)`, else the first, else none.
pub fn monitor_at(monitors: &[MonitorInfo], x: i32, y: i32) -> Option<&MonitorInfo> {
    monitors
        .iter()
        .find(|m| x >= m.x && y >= m.y && x < m.x + m.width as i32 && y < m.y + m.height as i32)
        .or_else(|| monitors.first())
}

/// Builds a project scene for display, ignoring errors (a broken warp just draws nothing).
pub fn scene_for(editor: &mut Editor, output: &str) -> Option<lumap_render::Scene> {
    let project: &Project = &editor.project;
    lumap_render::Scene::build_with(
        project,
        output,
        &editor.sources,
        lumap_render::scene::DEFAULT_MESH_DENSITY,
        &mut editor.mask_cache,
        !editor.show_mode,
    )
    .ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use lumap_core::project::demo_project;

    #[test]
    fn unit_conversions_and_monitor_lookup() {
        assert!((physical_to_points(200, 2.0) - 100.0).abs() < 1e-6);
        assert!((physical_to_points(200, 0.0) - 20000.0).abs() < 1e-3);
        let monitors = vec![
            MonitorInfo {
                name: "a".into(),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                scale: 1.0,
            },
            MonitorInfo {
                name: "b".into(),
                x: 1920,
                y: 0,
                width: 1280,
                height: 720,
                scale: 1.5,
            },
        ];
        assert_eq!(
            monitor_at(&monitors, 2000, 10).map(|m| m.name.as_str()),
            Some("b")
        );
        assert_eq!(
            monitor_at(&monitors, -5, -5).map(|m| m.name.as_str()),
            Some("a")
        );
        assert_eq!(monitor_at(&[], 0, 0), None);
    }

    #[test]
    fn scene_for_ignores_bad_outputs_and_drops_overlays_in_show_mode() {
        let mut e = Editor::new(demo_project(), None);
        assert!(scene_for(&mut e, "left").is_some());
        assert!(scene_for(&mut e, "ghost").is_none());
        e.set_overlay("left", Some(lumap_core::project::Overlay::Grid));
        let n = scene_for(&mut e, "left").unwrap().draws.len();
        e.enter_show_mode();
        assert_eq!(scene_for(&mut e, "left").unwrap().draws.len(), n - 1);
    }
}
