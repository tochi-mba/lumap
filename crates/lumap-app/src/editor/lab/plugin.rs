//! The lab's hands and eyes inside egui: a plugin that feeds synthetic input into the editor
//! window (clicks, typing, keys, drags, scrolling, as if a person did them) and reads back the
//! widget tree egui builds for screen readers, so a script can find a button by its label.

use eframe::egui::{
    self, accesskit, Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2,
    TouchPhase, ViewportId,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

/// One widget, as the lab's `/lab/ui/tree` reports it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Widget {
    /// Role, e.g. "Button", "TextInput".
    pub role: String,
    /// Accessible name (what tests click by).
    pub label: String,
    /// Current value, for inputs and sliders.
    pub value: Option<String>,
    /// Bounds in points: left, top, right, bottom.
    pub rect: [f32; 4],
    /// False when greyed out.
    pub enabled: bool,
}

impl Widget {
    /// Centre of the widget.
    pub fn centre(&self) -> Pos2 {
        Pos2::new(
            (self.rect[0] + self.rect[2]) / 2.0,
            (self.rect[1] + self.rect[3]) / 2.0,
        )
    }
}

/// What the plugin and the lab share.
#[derive(Default)]
pub struct Shared {
    /// Batches of events, one batch per frame, for the editor window.
    pub queue: VecDeque<Vec<Event>>,
    /// The latest widget tree.
    pub tree: Vec<Widget>,
    /// Frames the tree has been seen (to wait for a fresh one).
    pub tree_frames: u64,
}

/// The egui plugin.
pub struct LabPlugin {
    shared: Arc<Mutex<Shared>>,
}

impl LabPlugin {
    /// A plugin sharing `shared` with the lab.
    pub fn new(shared: Arc<Mutex<Shared>>) -> Self {
        Self { shared }
    }
}

impl egui::Plugin for LabPlugin {
    fn debug_name(&self) -> &'static str {
        "lumap-lab"
    }

    fn input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        if input.viewport_id != ViewportId::ROOT {
            return;
        }
        let mut s = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(batch) = s.queue.pop_front() {
            input.events.extend(batch);
        }
    }

    fn output_hook(&mut self, ctx: &egui::Context, output: &mut egui::FullOutput) {
        if ctx.viewport_id() != ViewportId::ROOT {
            return;
        }
        if let Some(update) = &output.platform_output.accesskit_update {
            let tree = widgets(update);
            let mut s = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
            s.tree = tree;
            s.tree_frames += 1;
        }
    }
}

/// The labelled widgets of an AccessKit update.
pub fn widgets(update: &accesskit::TreeUpdate) -> Vec<Widget> {
    update
        .nodes
        .iter()
        .filter_map(|(_, node)| {
            let label = node.label().map(str::to_string).unwrap_or_default();
            let value = node.value().map(str::to_string);
            if label.is_empty() && value.is_none() {
                return None;
            }
            let b = node.bounds()?;
            Some(Widget {
                role: format!("{:?}", node.role()),
                label,
                value,
                rect: [b.x0 as f32, b.y0 as f32, b.x1 as f32, b.y1 as f32],
                enabled: !node.is_disabled(),
            })
        })
        .collect()
}

/// Finds the `nth` widget labelled exactly `label` (then, failing that, containing it).
pub fn find<'a>(tree: &'a [Widget], label: &str, nth: usize) -> Option<&'a Widget> {
    let exact: Vec<&Widget> = tree.iter().filter(|w| w.label == label).collect();
    if let Some(w) = exact.get(nth) {
        return Some(w);
    }
    tree.iter().filter(|w| w.label.contains(label)).nth(nth)
}

/// The frames of events for a click at `pos`: move there, press, release.
pub fn click(pos: Pos2) -> Vec<Vec<Event>> {
    vec![
        vec![Event::PointerMoved(pos)],
        vec![button(pos, true)],
        vec![button(pos, false)],
    ]
}

/// The frames of a drag from `from` to `to`, in small steps.
pub fn drag(from: Pos2, to: Pos2) -> Vec<Vec<Event>> {
    let mut frames = vec![vec![Event::PointerMoved(from)], vec![button(from, true)]];
    let steps = 8;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        frames.push(vec![Event::PointerMoved(from + (to - from) * t)]);
    }
    frames.push(vec![button(to, false)]);
    frames
}

/// The frames of a key press: down, then up.
pub fn key(key: Key, modifiers: Modifiers) -> Vec<Vec<Event>> {
    let e = |pressed| Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    };
    vec![vec![e(true)], vec![e(false)]]
}

/// The frames of a wheel scroll at `pos`.
pub fn scroll(pos: Pos2, dy: f32) -> Vec<Vec<Event>> {
    vec![
        vec![Event::PointerMoved(pos)],
        vec![Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: egui::vec2(0.0, dy),
            phase: TouchPhase::Move,
            source: MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        }],
    ]
}

/// Parses "ctrl", "shift", "alt", "cmd" names into modifiers.
pub fn modifiers(names: &[String]) -> Modifiers {
    let mut m = Modifiers::NONE;
    for n in names {
        match n.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => m.ctrl = true,
            "shift" => m.shift = true,
            "alt" | "option" => m.alt = true,
            "cmd" | "command" | "meta" => m.command = true,
            _ => {}
        }
    }
    // lumap's shortcuts are written with `COMMAND`, which egui sets for Ctrl off a Mac.
    if m.ctrl {
        m.command = true;
    }
    m
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn w(label: &str, x: f32) -> Widget {
        Widget {
            role: "Button".into(),
            label: label.into(),
            value: None,
            rect: [x, 0.0, x + 10.0, 10.0],
            enabled: true,
        }
    }

    #[test]
    fn widgets_are_found_exactly_then_by_part_of_their_label() {
        let tree = vec![w("Go live", 0.0), w("Go live now", 20.0), w("Go live", 40.0)];
        assert_eq!(find(&tree, "Go live", 1).unwrap().rect[0], 40.0);
        assert_eq!(find(&tree, "now", 0).unwrap().label, "Go live now");
        assert!(find(&tree, "Scan", 0).is_none());
        assert_eq!(tree[1].centre(), Pos2::new(25.0, 5.0));
    }

    #[test]
    fn input_sequences_have_the_right_shape() {
        assert_eq!(click(Pos2::ZERO).len(), 3);
        assert_eq!(drag(Pos2::ZERO, Pos2::new(10.0, 0.0)).len(), 11);
        assert_eq!(key(Key::Space, Modifiers::NONE).len(), 2);
        assert_eq!(scroll(Pos2::ZERO, -40.0).len(), 2);
        let m = modifiers(&["Ctrl".into(), "shift".into(), "nonsense".into()]);
        assert!(m.ctrl && m.shift && !m.alt);
    }
}
