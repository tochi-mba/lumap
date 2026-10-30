//! Looks: ready-made animated content that needs no media. A look is a generator with a colour,
//! a speed and a blend mode chosen to work on a wall straight away, so a first-timer can put
//! something worth seeing on a building, a box or a dance floor in one click.

use crate::project::{BlendMode, GeneratorCategory, GeneratorKind, Project, Source, SourceKind};

/// One ready-made look.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// What the menu says.
    pub name: &'static str,
    /// The generator that draws it.
    pub kind: GeneratorKind,
    /// Tint, straight RGBA in `0..=1`.
    pub color: [f32; 4],
    /// Animation speed.
    pub speed: f32,
    /// How it lands on what is underneath.
    pub blend: BlendMode,
    /// One line on where it works best.
    pub blurb: &'static str,
}

impl Look {
    /// The menu group, from the generator.
    pub fn category(&self) -> GeneratorCategory {
        self.kind.category()
    }

    /// The source id the look's generator gets in a project.
    pub fn source_id(&self) -> String {
        format!("look-{}", self.name.to_lowercase().replace(' ', "-"))
    }

    /// The source kind for this look.
    pub fn source_kind(&self) -> SourceKind {
        SourceKind::Generator {
            pattern: self.kind,
            speed: self.speed,
            color: self.color,
        }
    }

    /// Puts the look on a surface: adds (or refreshes) its generator source, points the surface
    /// at it and sets the blend mode. Returns false when there is no such surface. Applying the
    /// same look twice leaves the project as it was after the first time.
    pub fn apply(&self, project: &mut Project, surface: &str) -> bool {
        if project.surface(surface).is_none() {
            return false;
        }
        let id = self.source_id();
        let kind = self.source_kind();
        match project.sources.iter_mut().find(|s| s.id == id) {
            Some(existing) => existing.kind = kind,
            None => project.sources.push(Source {
                id: id.clone(),
                kind,
            }),
        }
        if let Some(s) = project.surface_mut(surface) {
            s.source = id;
            s.blend_mode = self.blend;
        }
        true
    }
}

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const SIGNAL: [f32; 4] = [0.84, 1.0, 0.25, 1.0];

/// Every built-in look, grouped by category in menu order.
pub const LOOKS: &[Look] = &[
    Look {
        name: "Edge chase",
        kind: GeneratorKind::EdgeChase,
        color: SIGNAL,
        speed: 1.0,
        blend: BlendMode::Add,
        blurb: "A light running round the edge of a box, a door or a screen.",
    },
    Look {
        name: "Neon outline",
        kind: GeneratorKind::Outline,
        color: [1.0, 0.2, 0.75, 1.0],
        speed: 1.0,
        blend: BlendMode::Add,
        blurb: "A breathing neon border. Map one surface per object.",
    },
    Look {
        name: "Scanner",
        kind: GeneratorKind::Scan,
        color: [0.3, 0.85, 1.0, 1.0],
        speed: 1.0,
        blend: BlendMode::Screen,
        blurb: "A light bar sweeping across, then down. Good on a façade.",
    },
    Look {
        name: "Breathing bricks",
        kind: GeneratorKind::Bricks,
        color: [1.0, 0.85, 0.65, 1.0],
        speed: 0.6,
        blend: BlendMode::Normal,
        blurb: "A brick wall that breathes in waves. Line the courses up with real ones.",
    },
    Look {
        name: "Night windows",
        kind: GeneratorKind::Windows,
        color: [1.0, 0.78, 0.4, 1.0],
        speed: 1.0,
        blend: BlendMode::Normal,
        blurb: "Windows lighting up and going dark, like a building at night.",
    },
    Look {
        name: "Tetris",
        kind: GeneratorKind::Blocks,
        color: [0.45, 0.9, 1.0, 1.0],
        speed: 1.0,
        blend: BlendMode::Normal,
        blurb: "Blocks falling and stacking up a wall.",
    },
    Look {
        name: "Wire cube",
        kind: GeneratorKind::Wireframe,
        color: WHITE,
        speed: 1.0,
        blend: BlendMode::Add,
        blurb: "A rotating wireframe cube. Best on a flat, square surface.",
    },
    Look {
        name: "Rain on glass",
        kind: GeneratorKind::Water,
        color: [0.35, 0.7, 1.0, 1.0],
        speed: 1.0,
        blend: BlendMode::Screen,
        blurb: "Ripples spreading across the surface.",
    },
    Look {
        name: "Campfire",
        kind: GeneratorKind::Fire,
        color: [1.0, 0.45, 0.1, 1.0],
        speed: 1.0,
        blend: BlendMode::Add,
        blurb: "Flames rising up a wall or a column.",
    },
    Look {
        name: "Northern lights",
        kind: GeneratorKind::Aurora,
        color: [0.3, 1.0, 0.7, 1.0],
        speed: 0.5,
        blend: BlendMode::Screen,
        blurb: "Slow curtains of light. Calm, for a ceiling or a dome.",
    },
    Look {
        name: "Starfield",
        kind: GeneratorKind::Stars,
        color: WHITE,
        speed: 1.0,
        blend: BlendMode::Add,
        blurb: "Twinkling stars drifting slowly. Works on any dark surface.",
    },
    Look {
        name: "Disco floor",
        kind: GeneratorKind::Disco,
        color: [1.0, 0.3, 0.8, 1.0],
        speed: 1.0,
        blend: BlendMode::Normal,
        blurb: "Tiles flashing on the beat, two per second.",
    },
    Look {
        name: "Kaleido",
        kind: GeneratorKind::Kaleidoscope,
        color: [0.7, 0.5, 1.0, 1.0],
        speed: 0.8,
        blend: BlendMode::Screen,
        blurb: "A turning kaleidoscope. Centre it on a round surface.",
    },
    Look { name: "Anniversary", kind: GeneratorKind::Hearts, color: [1.0, 0.35, 0.55, 1.0], speed: 1.0, blend: BlendMode::Screen, blurb: "Soft hearts drifting up the wall, beating together. For an anniversary or a first dance." },
    Look { name: "Candlelight", kind: GeneratorKind::Fire, color: [1.0, 0.62, 0.28, 1.0], speed: 0.45, blend: BlendMode::Add, blurb: "Slow, warm flicker. Low and gentle, for a dinner table or an alcove." },
    Look {
        name: "Plasma",
        kind: GeneratorKind::Plasma,
        color: [0.5, 0.8, 1.0, 1.0],
        speed: 1.0,
        blend: BlendMode::Normal,
        blurb: "Classic flowing plasma.",
    },
    Look {
        name: "Radar",
        kind: GeneratorKind::Radar,
        color: [0.3, 1.0, 0.4, 1.0],
        speed: 1.0,
        blend: BlendMode::Add,
        blurb: "A radar sweep, green on black.",
    },
];

/// The look with this name, ignoring case.
pub fn find(name: &str) -> Option<&'static Look> {
    LOOKS
        .iter()
        .find(|l| l.name.eq_ignore_ascii_case(name.trim()))
}

/// The looks shown on the welcome screen: one per category, the most striking first.
pub const FEATURED: [&str; 7] = [
    "Anniversary",
    "Edge chase",
    "Night windows",
    "Campfire",
    "Northern lights",
    "Disco floor",
    "Starfield",
];

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::project::demo_project;

    #[test]
    fn looks_are_unique_named_and_cover_every_new_generator() {
        let mut names: Vec<_> = LOOKS.iter().map(|l| l.name.to_lowercase()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), LOOKS.len(), "names are unique");
        for kind in GeneratorKind::ALL {
            if kind.category() != GeneratorCategory::Classic {
                assert!(LOOKS.iter().any(|l| l.kind == kind), "{kind:?} has a look");
            }
        }
        for l in LOOKS {
            assert!(
                l.speed > 0.0
                    && !l.blurb.is_empty()
                    && l.color.iter().all(|c| (0.0..=1.0).contains(c))
            );
        }
        for name in FEATURED {
            assert!(find(name).is_some(), "{name} is a look");
        }
        assert_eq!(find("  campFIRE ").unwrap().kind, GeneratorKind::Fire);
        assert!(find("nope").is_none());
        assert_eq!(
            find("Wire cube").unwrap().category(),
            GeneratorCategory::Facade
        );
    }

    #[test]
    fn applying_a_look_points_the_surface_at_it_and_is_idempotent() {
        let mut p = demo_project();
        let look = find("Campfire").unwrap();
        let before = p.sources.len();
        assert!(look.apply(&mut p, "wall-left"));
        let s = p.surface("wall-left").unwrap();
        assert_eq!(s.source, "look-campfire");
        assert_eq!(s.blend_mode, BlendMode::Add);
        assert_eq!(p.sources.len(), before + 1);
        let once = p.clone();
        assert!(look.apply(&mut p, "wall-left"));
        assert_eq!(p, once, "applying twice changes nothing");
        assert!(look.apply(&mut p, "wall-right"));
        assert_eq!(p.sources.len(), before + 1, "the source is shared");
        assert!(!look.apply(&mut p, "missing"));
        assert!(p
            .validate()
            .iter()
            .all(|i| !i.to_string().contains("look-campfire")));
    }
}

#[cfg(test)]
#[test]
fn the_docs_count_the_looks_right() {
    // README, CHANGELOG and the website say "seventeen"; change them together with this list.
    assert_eq!(LOOKS.len(), 17);
}
