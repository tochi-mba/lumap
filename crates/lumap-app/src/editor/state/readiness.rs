//! How far a show has got: the facts behind the step ticks, the coach line and the pre-flight
//! check before going live. Pure reads of the editor; nothing here changes anything.

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

#[cfg(test)]
mod tests {
    use super::*;
    use lumap_core::project::{demo_project, Project};

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
