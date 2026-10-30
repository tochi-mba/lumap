//! Camera zones and hand gestures mapped onto parameter addresses, the way MIDI controls are.
//!
//! A zone reports how much is moving or present in a region of the camera view, a gesture
//! reports how sure the hand tracker is; both arrive as a value in `0..=1`. *Camera learn*
//! records which address an event drives, with the same three modes as MIDI learn. The table
//! is part of the project, so an installation's triggers travel with the show. Pure; the
//! camera and the trackers live in `lumap-vision` and the app.

use crate::midimap::MidiMode;
use crate::params::{ParamAddress, ParamPatch, ParamValue};
use serde::{Deserialize, Serialize};

/// A hand gesture the tracker recognises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gesture {
    /// Thumb and index finger touching: the pen is down.
    Pinch,
    /// A flat hand facing the camera.
    OpenPalm,
    /// A closed hand.
    Fist,
    /// A quick move to the left.
    SwipeLeft,
    /// A quick move to the right.
    SwipeRight,
    /// Thumb up, other fingers closed.
    ThumbsUp,
    /// Index finger out, the others closed.
    Point,
}

impl Gesture {
    /// Every gesture, in menu order.
    pub const ALL: [Self; 7] = [
        Self::Pinch,
        Self::OpenPalm,
        Self::Fist,
        Self::SwipeLeft,
        Self::SwipeRight,
        Self::ThumbsUp,
        Self::Point,
    ];

    /// What the interface calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pinch => "Pinch",
            Self::OpenPalm => "Open hand",
            Self::Fist => "Fist",
            Self::SwipeLeft => "Swipe left",
            Self::SwipeRight => "Swipe right",
            Self::ThumbsUp => "Thumbs up",
            Self::Point => "Point",
        }
    }
}

/// What a camera event comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CamKey {
    /// Motion or presence in a zone of the camera view.
    Zone {
        /// The zone's id.
        zone: String,
    },
    /// A hand gesture.
    Gesture {
        /// Which one.
        gesture: Gesture,
    },
}

impl CamKey {
    /// "Zone door", "Gesture Open hand".
    pub fn label(&self) -> String {
        match self {
            Self::Zone { zone } => format!("Zone {zone}"),
            Self::Gesture { gesture } => format!("Gesture {}", gesture.label()),
        }
    }
}

/// A reading from the camera: how much of `key` there is, in `0..=1`.
#[derive(Debug, Clone, PartialEq)]
pub struct CamEvent {
    /// What it comes from.
    pub key: CamKey,
    /// Amount, `0..=1`; at `0.5` or more a button is pressed and a switch is on.
    pub value: f64,
}

/// One mapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamMapping {
    /// Event to listen for.
    pub key: CamKey,
    /// Parameter to drive.
    pub address: String,
    /// Interpretation, as for MIDI.
    #[serde(default)]
    pub mode: MidiMode,
    /// Fader range start.
    #[serde(default)]
    pub min: f64,
    /// Fader range end.
    #[serde(default = "one")]
    pub max: f64,
}

fn one() -> f64 {
    1.0
}

/// The project's camera table.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CamMap {
    /// Mappings in order; every match for a key fires.
    #[serde(default)]
    pub mappings: Vec<CamMapping>,
}

impl CamMap {
    /// Adds or replaces the mapping for `key` and `address`.
    pub fn learn(&mut self, key: CamKey, address: &str, mode: MidiMode) {
        self.mappings
            .retain(|m| !(m.key == key && m.address == address));
        self.mappings.push(CamMapping {
            key,
            address: address.to_string(),
            mode,
            min: 0.0,
            max: 1.0,
        });
    }

    /// Removes mappings for an address.
    pub fn forget(&mut self, address: &str) {
        self.mappings.retain(|m| m.address != address);
    }

    /// Removes every mapping that listens to a zone (when the zone is deleted).
    pub fn forget_zone(&mut self, zone: &str) {
        self.mappings
            .retain(|m| !matches!(&m.key, CamKey::Zone { zone: z } if z == zone));
    }

    /// Patches for an event. A button fires only on the reading that crosses `0.5` upward,
    /// which the caller detects with [`CamMap::edges`]; here a button fires whenever pressed.
    pub fn translate(&self, event: &CamEvent) -> Vec<ParamPatch> {
        let value = event.value.clamp(0.0, 1.0);
        let pressed = value >= 0.5;
        self.mappings
            .iter()
            .filter(|m| m.key == event.key)
            .filter_map(|m| {
                let address = ParamAddress::parse(&m.address).ok()?;
                let value = match m.mode {
                    MidiMode::Fader => ParamValue::Number(m.min + (m.max - m.min) * value),
                    MidiMode::Button => {
                        if !pressed {
                            return None;
                        }
                        ParamValue::Bool(true)
                    }
                    MidiMode::Switch => ParamValue::Bool(pressed),
                };
                Some(ParamPatch { address, value })
            })
            .collect()
    }

    /// The events worth sending this frame, given the previous and current readings of every
    /// key: faders whenever their value moves by more than `1/256`, buttons only when they go
    /// from released to pressed, switches when they flip. Readings with no mapping are skipped.
    pub fn edges(&self, previous: &[CamEvent], current: &[CamEvent]) -> Vec<CamEvent> {
        let before = |key: &CamKey| {
            previous
                .iter()
                .find(|e| &e.key == key)
                .map_or(0.0, |e| e.value)
        };
        current
            .iter()
            .filter(|e| {
                let was = before(&e.key);
                self.mappings
                    .iter()
                    .filter(|m| m.key == e.key)
                    .any(|m| match m.mode {
                        MidiMode::Fader => (e.value - was).abs() > 1.0 / 256.0,
                        MidiMode::Button => was < 0.5 && e.value >= 0.5,
                        MidiMode::Switch => (was >= 0.5) != (e.value >= 0.5),
                    })
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    fn zone(id: &str, value: f64) -> CamEvent {
        CamEvent {
            key: CamKey::Zone { zone: id.into() },
            value,
        }
    }

    #[test]
    fn faders_scale_buttons_press_and_switches_follow() {
        let mut map = CamMap::default();
        map.learn(
            CamKey::Zone {
                zone: "door".into(),
            },
            "/show/master",
            MidiMode::Fader,
        );
        map.learn(
            CamKey::Zone {
                zone: "door".into(),
            },
            "/cue/go",
            MidiMode::Button,
        );
        map.learn(
            CamKey::Gesture {
                gesture: Gesture::OpenPalm,
            },
            "/show/blackout",
            MidiMode::Switch,
        );
        let patches = map.translate(&zone("door", 0.25));
        assert_eq!(patches.len(), 1, "the button stays up below one half");
        assert_eq!(patches[0].value, ParamValue::Number(0.25));
        let patches = map.translate(&zone("door", 2.0));
        assert_eq!(patches[0].value, ParamValue::Number(1.0), "clamped");
        assert_eq!(patches[1].value, ParamValue::Bool(true));
        let palm = |v| CamEvent {
            key: CamKey::Gesture {
                gesture: Gesture::OpenPalm,
            },
            value: v,
        };
        assert_eq!(map.translate(&palm(0.9))[0].value, ParamValue::Bool(true));
        assert_eq!(map.translate(&palm(0.1))[0].value, ParamValue::Bool(false));
        assert!(map.translate(&zone("window", 1.0)).is_empty());
    }

    #[test]
    fn learning_replaces_forgetting_removes_and_bad_addresses_are_skipped() {
        let mut map = CamMap::default();
        let door = CamKey::Zone {
            zone: "door".into(),
        };
        map.learn(door.clone(), "/show/master", MidiMode::Fader);
        map.learn(door.clone(), "/show/master", MidiMode::Switch);
        assert_eq!(map.mappings.len(), 1);
        assert_eq!(map.mappings[0].mode, MidiMode::Switch);
        map.learn(door.clone(), "not an address", MidiMode::Fader);
        assert_eq!(map.translate(&zone("door", 1.0)).len(), 1);
        map.forget("/show/master");
        assert_eq!(map.mappings.len(), 1);
        map.forget_zone("door");
        assert!(map.mappings.is_empty());
        assert_eq!(door.label(), "Zone door");
        assert_eq!(
            CamKey::Gesture {
                gesture: Gesture::ThumbsUp
            }
            .label(),
            "Gesture Thumbs up"
        );
        for g in Gesture::ALL {
            assert!(!g.label().is_empty());
        }
    }

    #[test]
    fn edges_send_buttons_once_switches_on_flips_and_faders_on_change() {
        let mut map = CamMap::default();
        map.learn(
            CamKey::Zone { zone: "b".into() },
            "/cue/go",
            MidiMode::Button,
        );
        map.learn(
            CamKey::Zone { zone: "s".into() },
            "/show/blackout",
            MidiMode::Switch,
        );
        map.learn(
            CamKey::Zone { zone: "f".into() },
            "/show/master",
            MidiMode::Fader,
        );
        let before = [zone("b", 0.2), zone("s", 0.2), zone("f", 0.5)];
        let now = [
            zone("b", 0.8),
            zone("s", 0.3),
            zone("f", 0.5),
            zone("x", 1.0),
        ];
        let fired: Vec<String> = map
            .edges(&before, &now)
            .iter()
            .map(|e| e.key.label())
            .collect();
        assert_eq!(fired, vec!["Zone b"]);
        let later = [zone("b", 0.9), zone("s", 0.7), zone("f", 0.6)];
        let fired: Vec<String> = map
            .edges(&now, &later)
            .iter()
            .map(|e| e.key.label())
            .collect();
        assert_eq!(
            fired,
            vec!["Zone s", "Zone f"],
            "held buttons do not repeat"
        );
        // A key never seen before counts as released.
        assert_eq!(map.edges(&[], &[zone("b", 1.0)]).len(), 1);
    }
}
