//! Step 4, Show: the controllers that can press GO, black out the show or move a fader, one
//! card each with a pill that says whether it is on. Keyboard, phone remote, OSC and MIDI.

use super::super::super::Editor;
use super::super::{Actions, PanelState};
use eframe::egui::{self, Color32, RichText, Stroke};
use lumap_core::midimap::{MidiKey, MidiMode};

/// Every OSC address lumap answers, for the Addresses list.
const OSC_ADDRESSES: &[(&str, &str)] = &[
    ("/cue/go", "GO the next cue"),
    ("/cue/<id>/go", "GO one cue"),
    ("/cue/stop", "Stop the fade"),
    ("/show/blackout", "0 or 1"),
    ("/show/master", "0 to 1"),
    ("/surface/<id>/opacity", "0 to 1"),
    ("/surface/<id>/visible", "0 or 1"),
    ("/surface/<id>/z", "Order"),
    (
        "/surface/<id>/corner/<n>/x",
        "Corner position, px (also /y)",
    ),
    ("/output/<id>/enabled", "On in show, 0 or 1"),
    ("/output/<id>/brightness", "Also contrast, gamma"),
    ("/output/<id>/blend/<edge>/width", "Edge blend, px"),
    ("/source/<id>/play", "Also loop, speed, restart"),
];

/// The keys that work while the show runs.
const KEYS: &[(&str, &str)] = &[
    ("Space", "GO the next cue"),
    ("B", "Blackout on and off"),
    ("F10", "Go live without the check"),
    ("Esc Esc", "Leave show mode"),
];

/// The right-hand panel on Show with nothing selected.
pub fn controllers(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    state: &mut PanelState,
    actions: &mut Actions,
) {
    ui.heading("Control");
    ui.label(
        RichText::new("Everything that can press GO, black out the show or move a fader.")
            .weak()
            .small(),
    );
    ui.add_space(6.0);
    card(ui, "Keyboard", "Always on", true, |ui| {
        egui::Grid::new("lumap-keys")
            .num_columns(2)
            .spacing(egui::vec2(12.0, 4.0))
            .show(ui, |ui| {
                for (key, what) in KEYS {
                    ui.label(RichText::new(*key).monospace().color(crate::theme::TEXT));
                    ui.label(RichText::new(*what).weak().small());
                    ui.end_row();
                }
            });
    });
    let remote = editor.remote_url();
    card(
        ui,
        "Phone remote",
        if remote.is_some() { "On" } else { "Off" },
        remote.is_some(),
        |ui| phone(ui, remote.as_deref(), state, actions),
    );
    let osc = editor.osc.as_ref().map(|s| s.local_addr().port());
    let osc_pill = osc.map_or_else(|| "Off".to_string(), |p| format!("Port {p}"));
    card(ui, "OSC", &osc_pill, osc.is_some(), |ui| {
        osc_card(ui, editor, osc, state, actions);
    });
    let midi = editor.midi.as_ref().map(|m| m.name().to_string());
    card(
        ui,
        "MIDI",
        midi.as_deref().unwrap_or("Off"),
        midi.is_some(),
        |ui| midi_card(ui, editor, state, actions),
    );
}

/// A controller's card: name, a pill saying whether it is on, then its settings.
fn card(ui: &mut egui::Ui, name: &str, status: &str, on: bool, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(crate::theme::PANEL)
        .stroke(Stroke::new(1.0, crate::theme::LINE))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).family(crate::theme::strong_family()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    status_pill(ui, status, on);
                });
            });
            ui.add_space(4.0);
            body(ui);
        });
    ui.add_space(8.0);
}

/// On: signal outline. Off: the quiet pill.
fn status_pill(ui: &mut egui::Ui, text: &str, on: bool) {
    if !on {
        crate::theme::pill(ui, text, false);
        return;
    }
    egui::Frame::new()
        .fill(crate::theme::SIGNAL.linear_multiply(0.12))
        .stroke(Stroke::new(1.0, crate::theme::SIGNAL))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(crate::theme::SIGNAL));
        });
}

/// The phone remote: a port, start and stop, and the address with a QR code to scan.
fn phone(ui: &mut egui::Ui, url: Option<&str>, state: &mut PanelState, actions: &mut Actions) {
    ui.horizontal(|ui| {
        let label = ui.label("Remote port");
        ui.add(egui::DragValue::new(&mut state.remote_port).range(0..=65535))
            .labelled_by(label.id);
        if url.is_some() {
            if ui.button("Stop remote").clicked() {
                actions.remote_port = Some(0);
            }
        } else if ui.button("Start remote").clicked() {
            actions.remote_port = Some(state.remote_port);
        }
    });
    let Some(url) = url else {
        ui.label(
            RichText::new("GO, blackout and the master fader from any phone on the same Wi-Fi.")
                .weak()
                .small(),
        );
        return;
    };
    ui.label(RichText::new(url).monospace().color(crate::theme::SIGNAL));
    ui.label(
        RichText::new("Scan with a phone on the same Wi-Fi:")
            .weak()
            .small(),
    );
    if let Some((n, modules)) = lumap_control::web::qr_modules(url) {
        let cell = 3.0;
        let quiet = 4.0 * cell;
        let size = cell * n as f32 + 2.0 * quiet;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 4.0, Color32::WHITE);
        for y in 0..n {
            for x in 0..n {
                if modules[y * n + x] {
                    let min =
                        rect.min + egui::vec2(quiet + x as f32 * cell, quiet + y as f32 * cell);
                    painter.rect_filled(
                        egui::Rect::from_min_size(min, egui::vec2(cell, cell)),
                        0.0,
                        Color32::BLACK,
                    );
                }
            }
        }
    }
}

/// OSC: the listening port, where changes are echoed to, and every address.
fn osc_card(
    ui: &mut egui::Ui,
    editor: &Editor,
    port: Option<u16>,
    state: &mut PanelState,
    actions: &mut Actions,
) {
    ui.horizontal(|ui| {
        let label = ui.label("OSC port");
        ui.add(egui::DragValue::new(&mut state.osc_port).range(1..=65535))
            .labelled_by(label.id);
        if port.is_some() {
            if ui.button("Stop OSC").clicked() {
                actions.osc_port = Some(0);
            }
        } else if ui.button("Listen").clicked() {
            actions.osc_port = Some(state.osc_port);
        }
    });
    ui.horizontal(|ui| {
        let label = ui.label("Feedback host:port");
        ui.add(
            egui::TextEdit::singleline(&mut state.feedback_target)
                .desired_width(130.0)
                .hint_text("192.168.1.20:9001"),
        )
        .labelled_by(label.id)
        .on_hover_text("Every change is sent back here, so a control surface stays in step");
        if ui.button("Apply feedback").clicked() {
            actions.feedback = Some(state.feedback_target.trim().to_string());
        }
    });
    let feedback = editor.feedback.as_ref().map_or_else(
        || "Feedback off".to_string(),
        |f| format!("Sending changes to {}", f.target()),
    );
    ui.label(RichText::new(feedback).weak().small());
    egui::CollapsingHeader::new("OSC addresses")
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new("lumap-osc-addresses")
                .num_columns(2)
                .spacing(egui::vec2(10.0, 3.0))
                .show(ui, |ui| {
                    for (address, what) in OSC_ADDRESSES {
                        ui.label(RichText::new(*address).monospace().small());
                        ui.label(RichText::new(*what).weak().small());
                        ui.end_row();
                    }
                });
        });
}

/// "CC 7 · channel 1", "Note 60 · channel 10": channels counted from one, as on the hardware.
fn key_label(key: MidiKey) -> String {
    match key {
        MidiKey::Cc {
            channel,
            controller,
        } => format!("CC {controller} · channel {}", channel + 1),
        MidiKey::Note { channel, note } => format!("Note {note} · channel {}", channel + 1),
    }
}

fn mode_label(mode: MidiMode) -> &'static str {
    match mode {
        MidiMode::Fader => "fader",
        MidiMode::Button => "button",
        MidiMode::Switch => "switch",
    }
}

/// MIDI: the input port, learning a control for an address, and what is learned.
fn midi_card(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    state: &mut PanelState,
    actions: &mut Actions,
) {
    ui.horizontal(|ui| {
        egui::ComboBox::from_label("MIDI port")
            .selected_text(if state.midi_port.is_empty() {
                "None"
            } else {
                state.midi_port.as_str()
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.midi_port, String::new(), "None");
                let ports = state.midi_ports.clone();
                for port in ports {
                    ui.selectable_value(&mut state.midi_port, port.clone(), port);
                }
            });
    });
    ui.horizontal(|ui| {
        if ui.button("Refresh MIDI ports").clicked() {
            state.midi_ports = lumap_control::midi::ports().unwrap_or_default();
        }
        if ui.button("Open MIDI port").clicked() {
            actions.midi_port = Some(state.midi_port.clone());
        }
    });
    if let Some(e) = editor.last_midi {
        ui.label(
            RichText::new(format!("Last message: {} = {}", key_label(e.key), e.value))
                .monospace()
                .small(),
        );
    }
    crate::theme::section_header(ui, "Learn");
    ui.horizontal(|ui| {
        let label = ui.label("Learn address");
        ui.add(egui::TextEdit::singleline(&mut state.midi_address).desired_width(150.0))
            .labelled_by(label.id)
            .on_hover_text("Any OSC address above, such as /show/master or /cue/go");
    });
    ui.horizontal(|ui| {
        for (what, mode) in [
            ("Learn fader", MidiMode::Fader),
            ("Learn button", MidiMode::Button),
            ("Learn switch", MidiMode::Switch),
        ] {
            if ui.button(what).clicked() {
                editor.arm_midi_learn(state.midi_address.trim(), mode);
            }
        }
    });
    if let Some((address, _)) = editor.midi_learn.clone() {
        ui.horizontal(|ui| {
            crate::theme::pill(ui, "learning", true);
            ui.label(format!("move a control for {address}"));
            if ui.button("Cancel learn").clicked() {
                editor.midi_learn = None;
            }
        });
    }
    let mappings = editor.project.midi.mappings.clone();
    for m in mappings {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} → {} ({})",
                    key_label(m.key),
                    m.address,
                    mode_label(m.mode)
                ))
                .small(),
            );
            if ui.small_button(format!("Forget {}", m.address)).clicked() {
                let address = m.address.clone();
                editor.edit(move |p| p.midi.forget(&address));
            }
        });
    }
}
