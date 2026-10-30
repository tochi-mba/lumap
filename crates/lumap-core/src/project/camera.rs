//! What the camera features keep in a show: the camera, its zones and what they drive, and the
//! choices of the content kinds that draw from the room (room effects and paint).

use crate::cammap::CamMap;
use crate::geom::Vec2;
use serde::{Deserialize, Serialize};

/// The camera a show uses and what it watches for.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraSetup {
    /// The device as the camera list names it; empty means none chosen yet.
    pub device: String,
    /// Regions of the camera view that report motion or presence.
    pub zones: Vec<Zone>,
    /// What zones and gestures drive.
    #[serde(flatten)]
    pub map: CamMap,
}

impl CameraSetup {
    /// Nothing chosen or drawn: left out of saved project files.
    pub fn is_empty(&self) -> bool {
        self.device.is_empty() && self.zones.is_empty() && self.map.mappings.is_empty()
    }
}

/// A region of the camera view, as a polygon in normalised camera coordinates (`0..=1` on
/// both axes, origin top left).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    /// Unique id, used by Camera learn.
    pub id: String,
    /// Corners, at least three.
    pub points: Vec<Vec2>,
    /// How easily it triggers: `1` is normal, higher fires on smaller movements.
    #[serde(default = "one")]
    pub sensitivity: f64,
}

fn one() -> f64 {
    1.0
}

impl Zone {
    /// A rectangle zone.
    pub fn rect(id: &str, min: Vec2, max: Vec2) -> Self {
        Self {
            id: id.to_string(),
            points: vec![min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)],
            sensitivity: 1.0,
        }
    }

    /// At least three finite corners inside the view.
    pub fn is_valid(&self) -> bool {
        self.points.len() >= 3
            && self
                .points
                .iter()
                .all(|p| p.is_finite() && (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y))
            && self.sensitivity.is_finite()
            && self.sensitivity > 0.0
    }
}

/// An effect drawn from a projector's scan of the room and, for some, the live camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomEffect {
    /// The room's real edges light up and trace themselves.
    Trace,
    /// Letters fall and land on people's shadows.
    Letters,
    /// People's shadows fill with colour.
    ShadowFill,
    /// Shadows grow eyes and teeth.
    ShadowMonsters,
    /// Snow falls and settles on the room's real ledges.
    Snow,
    /// Balls of light bounce off edges and shadows.
    Bounce,
    /// Marbles roll down the room's edges.
    MarbleRun,
}

impl RoomEffect {
    /// Every effect, in library order.
    pub const ALL: [Self; 7] = [
        Self::Trace,
        Self::Letters,
        Self::ShadowFill,
        Self::ShadowMonsters,
        Self::Snow,
        Self::Bounce,
        Self::MarbleRun,
    ];

    /// What the library calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Trace => "Trace the room",
            Self::Letters => "Letters",
            Self::ShadowFill => "Shadow fill",
            Self::ShadowMonsters => "Shadow monsters",
            Self::Snow => "Snow that settles",
            Self::Bounce => "Bouncing light",
            Self::MarbleRun => "Marble run",
        }
    }

    /// One line for the library tile.
    pub fn blurb(self) -> &'static str {
        match self {
            Self::Trace => "Light runs along every real edge the scan found.",
            Self::Letters => "Letters rain down and land on anyone standing in the light.",
            Self::ShadowFill => "Your shadow fills with moving colour.",
            Self::ShadowMonsters => "Shadows grow eyes and teeth.",
            Self::Snow => "Snow drifts down and piles up on shelves and frames.",
            Self::Bounce => "Balls of light bounce off the room and off people.",
            Self::MarbleRun => "Marbles roll along the room's edges.",
        }
    }

    /// Whether it watches the live camera (shadows), not only the scan.
    pub fn needs_camera(self) -> bool {
        matches!(
            self,
            Self::Letters | Self::ShadowFill | Self::ShadowMonsters | Self::Bounce
        )
    }
}

/// What draws a paint source's strokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaintInput {
    /// The brightest moving spot: a phone torch, a laser pointer, a glow stick.
    #[default]
    Light,
    /// An index finger, drawing while the hand pinches.
    Hand,
}

impl PaintInput {
    /// Every input, in menu order.
    pub const ALL: [Self; 2] = [Self::Light, Self::Hand];

    /// What the interface calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Torch or laser",
            Self::Hand => "Hand (pinch to draw)",
        }
    }
}

/// How painted strokes look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Brush {
    /// A glowing tube.
    #[default]
    Neon,
    /// A solid line with soft edges.
    Ink,
    /// Twinkling dots along the stroke.
    Sparkle,
    /// A line whose colour runs through the rainbow.
    Rainbow,
}

impl Brush {
    /// Every brush, in menu order.
    pub const ALL: [Self; 4] = [Self::Neon, Self::Ink, Self::Sparkle, Self::Rainbow];

    /// What the interface calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Neon => "Neon",
            Self::Ink => "Ink",
            Self::Sparkle => "Sparkle",
            Self::Rainbow => "Rainbow",
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn zones_check_their_corners_and_setups_know_when_they_are_empty() {
        let z = Zone::rect("door", Vec2::new(0.1, 0.2), Vec2::new(0.4, 0.9));
        assert!(z.is_valid());
        assert_eq!(z.points.len(), 4);
        let mut bad = z.clone();
        bad.points.truncate(2);
        assert!(!bad.is_valid());
        let mut outside = z.clone();
        outside.points[0] = Vec2::new(1.5, 0.0);
        assert!(!outside.is_valid());
        let mut numb = z.clone();
        numb.sensitivity = 0.0;
        assert!(!numb.is_valid());
        let mut setup = CameraSetup::default();
        assert!(setup.is_empty());
        setup.zones.push(z);
        assert!(!setup.is_empty());
    }

    #[test]
    fn every_effect_input_and_brush_has_words() {
        for e in RoomEffect::ALL {
            assert!(!e.label().is_empty() && !e.blurb().is_empty());
        }
        assert!(RoomEffect::Letters.needs_camera());
        assert!(!RoomEffect::Trace.needs_camera());
        for i in PaintInput::ALL {
            assert!(!i.label().is_empty());
        }
        for b in Brush::ALL {
            assert!(!b.label().is_empty());
        }
    }

    #[test]
    fn a_setup_round_trips_through_toml() {
        let mut setup = CameraSetup {
            device: "Integrated Camera".into(),
            ..CameraSetup::default()
        };
        setup
            .zones
            .push(Zone::rect("path", Vec2::new(0.0, 0.5), Vec2::new(0.3, 1.0)));
        setup.map.learn(
            crate::cammap::CamKey::Zone {
                zone: "path".into(),
            },
            "/cue/go",
            crate::midimap::MidiMode::Button,
        );
        let text = toml::to_string(&setup).unwrap();
        let back: CameraSetup = toml::from_str(&text).unwrap();
        assert_eq!(back, setup);
    }
}
