//! How far a show has got: the facts behind the step ticks, the coach line and the pre-flight
//! check before going live. Pure reads of the editor; nothing here changes anything.

use super::super::MonitorInfo;
use super::Editor;
use lumap_core::project::SourceKind;

/// What a show has, counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Readiness {
    /// Projectors (outputs) in the show.
    pub projectors: usize,
    /// Projectors whose window is open.
    pub projectors_open: usize,
    /// Surfaces in the show.
    pub surfaces: usize,
    /// Surfaces showing real content: a look, media or a live input rather than a test
    /// pattern, and a source that exists.
    pub surfaces_with_content: usize,
    /// Stored cues.
    pub cues: usize,
}

impl Readiness {
    /// At least one projector.
    pub fn setup_done(&self) -> bool {
        self.projectors > 0
    }

    /// At least one surface.
    pub fn map_done(&self) -> bool {
        self.surfaces > 0
    }

    /// Every surface has real content.
    pub fn content_done(&self) -> bool {
        self.surfaces > 0 && self.surfaces_with_content == self.surfaces
    }

    /// At least one cue stored.
    pub fn show_done(&self) -> bool {
        self.cues > 0
    }
}

impl Editor {
    /// Counts what the show has so far.
    pub fn readiness(&self) -> Readiness {
        let p = &self.project;
        let real = |id: &str| {
            p.source(id)
                .is_some_and(|s| !matches!(s.kind, SourceKind::TestPattern { .. }))
        };
        Readiness {
            projectors: p.outputs.len(),
            projectors_open: p
                .outputs
                .iter()
                .filter(|o| self.open_outputs.contains(&o.id))
                .count(),
            surfaces: p.surfaces.len(),
            surfaces_with_content: p.surfaces.iter().filter(|s| real(&s.source)).count(),
            cues: p.cues.len(),
        }
    }
}

/// A one-click fix offered beside a line of the pre-flight check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fix {
    /// Put the projectors that have no display on free displays.
    Place,
    /// Finish step 1 there.
    Setup,
    /// Finish step 2 there.
    Map,
    /// Finish step 3 there.
    Content,
    /// Open Settings (where ffmpeg is set).
    Settings,
    /// Save the show.
    Save,
    /// Store the live look as a cue.
    StoreCue,
}

impl Fix {
    /// The fix's button text.
    pub fn label(self) -> &'static str {
        match self {
            Self::Place => "Place on free displays",
            Self::Setup => "Open Setup",
            Self::Map => "Open Map",
            Self::Content => "Choose content",
            Self::Settings => "Open Settings",
            Self::Save => "Save the show",
            Self::StoreCue => "Store the first cue",
        }
    }
}

/// One line of the check before going live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Nothing to do.
    pub ok: bool,
    /// What was found, in a sentence.
    pub text: String,
    /// What would put it right, when something can.
    pub fix: Option<Fix>,
}

impl Check {
    fn ok(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            text: text.into(),
            fix: None,
        }
    }

    fn fix(text: impl Into<String>, fix: Fix) -> Self {
        Self {
            ok: false,
            text: text.into(),
            fix: Some(fix),
        }
    }
}

/// "left", "left and right", "left, right and top".
fn names(ids: &[String]) -> String {
    match ids {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// "1 projector", "2 projectors".
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl Editor {
    /// Projectors that are on in show mode but have no connected display: a floating window, or
    /// a display that is not plugged in. A custom position counts as placed.
    pub fn unplaced_projectors(&self, monitors: &[MonitorInfo]) -> Vec<String> {
        self.project
            .outputs
            .iter()
            .filter(|o| o.enabled)
            .filter(|o| match &o.placement {
                None => true,
                Some(p) => !p.monitor.is_empty() && !monitors.iter().any(|m| m.name == p.monitor),
            })
            .map(|o| o.id.clone())
            .collect()
    }

    /// Displays a projector could go on: connected, not the first one (the editor's), and not
    /// already holding a projector.
    pub fn free_displays<'m>(&self, monitors: &'m [MonitorInfo]) -> Vec<&'m MonitorInfo> {
        monitors
            .iter()
            .skip(1)
            .filter(|m| {
                !self
                    .project
                    .outputs
                    .iter()
                    .any(|o| o.placement.as_ref().is_some_and(|p| p.monitor == m.name))
            })
            .collect()
    }

    /// The check before going live: projectors on displays, content on every surface, video
    /// able to play, the show saved (so autosave and crash recovery protect it) and cues to
    /// fade between. Every line is advice; going live anyway is always allowed.
    pub fn preflight(&self, monitors: &[MonitorInfo], ffmpeg_ok: bool) -> Vec<Check> {
        let r = self.readiness();
        let p = &self.project;
        let mut checks = Vec::new();
        let on = p.outputs.iter().filter(|o| o.enabled).count();
        let unplaced = self.unplaced_projectors(monitors);
        checks.push(if r.projectors == 0 {
            Check::fix("No projectors yet", Fix::Setup)
        } else if on == 0 {
            Check::fix("Every projector is off in show mode", Fix::Setup)
        } else if unplaced.is_empty() {
            Check::ok(format!(
                "{} on {}",
                count(on, "projector", "projectors"),
                if on == 1 {
                    "its display"
                } else {
                    "their displays"
                }
            ))
        } else {
            let spare = self.free_displays(monitors).len();
            let verb = if unplaced.len() == 1 { "has" } else { "have" };
            let text = format!("{} {verb} no display yet", names(&unplaced));
            Check::fix(
                text,
                if spare >= unplaced.len() {
                    Fix::Place
                } else {
                    Fix::Setup
                },
            )
        });
        checks.push(if r.surfaces == 0 {
            Check::fix("No surfaces yet", Fix::Map)
        } else if r.content_done() {
            Check::ok("Every surface has content")
        } else {
            let left = r.surfaces - r.surfaces_with_content;
            Check::fix(
                format!(
                    "{} still {} a test pattern",
                    count(left, "surface", "surfaces"),
                    if left == 1 { "shows" } else { "show" }
                ),
                Fix::Content,
            )
        });
        if p.sources
            .iter()
            .any(|s| matches!(s.kind, SourceKind::Video { .. }))
        {
            checks.push(if ffmpeg_ok {
                Check::ok("Video can play: ffmpeg found")
            } else {
                Check::fix(
                    "Video needs ffmpeg, and lumap cannot find it",
                    Fix::Settings,
                )
            });
        }
        checks.push(if self.path.is_none() {
            Check::fix(
                "Not saved yet: autosave and crash recovery need a file",
                Fix::Save,
            )
        } else if self.dirty {
            Check::fix("Unsaved changes", Fix::Save)
        } else {
            Check::ok("Saved, and autosaved while you work")
        });
        checks.push(if r.cues == 0 {
            Check::fix("No cues: GO has nothing to fade to", Fix::StoreCue)
        } else {
            Check::ok(format!("{} ready for GO", count(r.cues, "cue", "cues")))
        });
        checks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumap_core::project::{demo_project, Project};

    fn monitors(n: usize) -> Vec<MonitorInfo> {
        (0..n)
            .map(|i| MonitorInfo {
                name: format!("DISPLAY{}", i + 1),
                x: 1920 * i as i32,
                y: 0,
                width: 1920,
                height: 1080,
                scale: 1.0,
            })
            .collect()
    }

    fn failing(checks: &[Check]) -> Vec<(String, Option<Fix>)> {
        checks
            .iter()
            .filter(|c| !c.ok)
            .map(|c| (c.text.clone(), c.fix))
            .collect()
    }

    #[test]
    fn an_empty_show_is_told_where_to_start() {
        let e = Editor::new(Project::default(), None);
        let checks = e.preflight(&monitors(1), true);
        assert_eq!(
            failing(&checks),
            vec![
                ("No projectors yet".to_string(), Some(Fix::Setup)),
                ("No surfaces yet".to_string(), Some(Fix::Map)),
                (
                    "Not saved yet: autosave and crash recovery need a file".to_string(),
                    Some(Fix::Save)
                ),
                (
                    "No cues: GO has nothing to fade to".to_string(),
                    Some(Fix::StoreCue)
                ),
            ]
        );
        for fix in [
            Fix::Place,
            Fix::Setup,
            Fix::Map,
            Fix::Content,
            Fix::Settings,
            Fix::Save,
            Fix::StoreCue,
        ] {
            assert!(!fix.label().is_empty());
        }
    }

    #[test]
    fn projectors_without_a_display_are_placed_on_free_ones() {
        let mut e = Editor::new(demo_project(), None);
        // The demo's projectors float; two spare displays take them both.
        let three = monitors(3);
        assert_eq!(e.unplaced_projectors(&three), vec!["left", "right"]);
        let checks = e.preflight(&three, true);
        assert_eq!(checks[0].text, "left and right have no display yet");
        assert_eq!(checks[0].fix, Some(Fix::Place));
        // With one spare display there is not room for both: Setup decides.
        assert_eq!(e.preflight(&monitors(2), true)[0].fix, Some(Fix::Setup));
        assert_eq!(e.place_on_free_displays(&three), 2);
        assert!(e.unplaced_projectors(&three).is_empty());
        let left = e.project.output("left").unwrap_or_else(|| unreachable!());
        let placement = left.placement.clone().unwrap_or_else(|| unreachable!());
        assert_eq!(placement.monitor, "DISPLAY2", "never the editor's display");
        assert_eq!((left.width, left.height), (1920, 1080));
        assert_eq!(
            e.preflight(&three, true)[0],
            Check::ok("2 projectors on their displays")
        );
        // Unplugging a display unplaces its projector; nothing is free to take it.
        assert_eq!(e.unplaced_projectors(&monitors(2)), vec!["right"]);
        assert_eq!(e.place_on_free_displays(&monitors(2)), 0);
        assert_eq!(
            e.preflight(&monitors(2), true)[0].text,
            "right has no display yet"
        );
        // A projector that is off in show mode is not checked; all off is its own line.
        for o in &mut e.project.outputs {
            o.enabled = false;
        }
        assert!(e.unplaced_projectors(&monitors(1)).is_empty());
        assert_eq!(
            e.preflight(&three, true)[0].text,
            "Every projector is off in show mode"
        );
        e.project.outputs[0].enabled = true;
        assert_eq!(
            e.preflight(&three, true)[0].text,
            "1 projector on its display"
        );
    }

    #[test]
    fn content_video_saving_and_cues_each_have_a_line() {
        let mut e = Editor::new(demo_project(), None);
        let checks = e.preflight(&monitors(3), false);
        assert_eq!(checks[1].fix, Some(Fix::Content));
        assert!(
            checks[1].text.ends_with("still show a test pattern"),
            "{}",
            checks[1].text
        );
        assert!(
            !checks.iter().any(|c| c.text.contains("ffmpeg")),
            "no video, no ffmpeg line"
        );
        let id = e.project.surfaces[0].id.clone();
        let ids: Vec<String> = e.project.surfaces.iter().map(|s| s.id.clone()).collect();
        for sid in ids.iter().skip(1) {
            let look = lumap_core::looks::find("Campfire").unwrap_or_else(|| unreachable!());
            look.apply(&mut e.project, sid);
        }
        assert_eq!(
            e.preflight(&monitors(3), true)[1].text,
            "1 surface still shows a test pattern"
        );
        e.add_source(SourceKind::Video {
            path: "clip.mp4".into(),
            playback: lumap_core::project::Playback::default(),
        });
        let video = e
            .project
            .sources
            .last()
            .map(|s| s.id.clone())
            .unwrap_or_default();
        e.project
            .surface_mut(&id)
            .unwrap_or_else(|| unreachable!())
            .source = video;
        let checks = e.preflight(&monitors(3), false);
        assert_eq!(checks[1], Check::ok("Every surface has content"));
        assert_eq!(checks[2].fix, Some(Fix::Settings));
        assert_eq!(
            e.preflight(&monitors(3), true)[2],
            Check::ok("Video can play: ffmpeg found")
        );
        // Saving: no file, then unsaved changes, then saved.
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        e.path = Some(dir.path().join("show.lumap.toml"));
        e.dirty = true;
        let last_but_one = |e: &Editor| {
            let c = e.preflight(&monitors(3), true);
            c[c.len() - 2].clone()
        };
        assert_eq!(last_but_one(&e), Check::fix("Unsaved changes", Fix::Save));
        e.dirty = false;
        assert!(last_but_one(&e).ok);
        e.store_cue("opening", 1.0);
        e.store_cue("", 1.0);
        let checks = e.preflight(&monitors(3), true);
        assert_eq!(checks.last(), Some(&Check::ok("2 cues ready for GO")));
    }

    #[test]
    fn an_empty_show_has_nothing_done() {
        let e = Editor::new(Project::default(), None);
        let r = e.readiness();
        assert_eq!(r, Readiness::default());
        assert!(!r.setup_done() && !r.map_done() && !r.content_done() && !r.show_done());
    }

    #[test]
    fn the_demo_is_set_up_and_mapped_and_a_look_completes_its_content() {
        let mut e = Editor::new(demo_project(), None);
        let r = e.readiness();
        assert!(r.setup_done() && r.map_done());
        assert_eq!(r.projectors, e.project.outputs.len());
        assert_eq!(r.projectors_open, 0);
        assert_eq!(r.surfaces, e.project.surfaces.len());
        assert!(
            !r.content_done(),
            "test patterns are placeholders, not content: {r:?}"
        );
        let look = lumap_core::looks::find("Campfire").unwrap_or_else(|| unreachable!());
        let ids: Vec<String> = e.project.surfaces.iter().map(|s| s.id.clone()).collect();
        for id in &ids {
            look.apply(&mut e.project, id);
        }
        e.open_outputs.push(e.project.outputs[0].id.clone());
        let r = e.readiness();
        assert!(r.content_done(), "{r:?}");
        assert_eq!(r.projectors_open, 1);
        assert!(!r.show_done());
        e.store_cue("opening", 1.0);
        assert!(e.readiness().show_done());
        // A surface pointing at a source that no longer exists has no content.
        e.project.surfaces[0].source = "gone".into();
        assert!(!e.readiness().content_done());
    }
}
