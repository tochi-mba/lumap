//! Choosing a built-in look: chips on the welcome screen, a section in the surface inspector,
//! and generators grouped by category in the "+ source" menu.

use super::super::Editor;
use super::{Actions, PanelState};
use eframe::egui::{self, RichText};
use lumap_core::looks::{self, Look, FEATURED, LOOKS};
use lumap_core::project::{GeneratorCategory, GeneratorKind, SourceKind};

/// Puts a look on one surface as a single undo step and reopens the sources.
pub fn apply_look(editor: &mut Editor, look: &Look, surface: &str) {
    if editor.edit(|p| {
        look.apply(p, surface);
    }) {
        editor.reload_sources();
        editor.status = format!("{} on {surface}", look.name);
    }
}

/// Puts a look on every surface, so a fresh project lights up the whole wall.
pub fn apply_look_everywhere(editor: &mut Editor, look: &Look) {
    let surfaces: Vec<String> = editor
        .project
        .surfaces
        .iter()
        .map(|s| s.id.clone())
        .collect();
    if surfaces.is_empty() {
        editor.status = "Add a projector and a surface first, then pick a look.".into();
        return;
    }
    if editor.edit(|p| {
        for s in &surfaces {
            look.apply(p, s);
        }
    }) {
        editor.reload_sources();
        editor.status = format!("{} on every surface", look.name);
    }
}

/// The "Try a look" row on the welcome screen.
pub fn welcome_row(ui: &mut egui::Ui, actions: &mut Actions) {
    ui.add_space(6.0);
    ui.label(RichText::new("Try a look").family(crate::theme::strong_family()));
    ui.horizontal_wrapped(|ui| {
        for name in FEATURED {
            if ui
                .button(name)
                .on_hover_text(looks::find(name).map_or("", |l| l.blurb))
                .clicked()
            {
                actions.look = Some(name.to_string());
            }
        }
    });
}

/// The Looks section of the surface inspector.
pub fn surface_section(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    surface: &str,
    state: &mut PanelState,
) {
    crate::theme::section_header(ui, "Looks");
    let chosen = LOOKS.get(state.look).copied().unwrap_or(LOOKS[0]);
    ui.horizontal(|ui| {
        egui::ComboBox::from_label("Look")
            .selected_text(chosen.name)
            .show_ui(ui, |ui| {
                for category in GeneratorCategory::ALL {
                    let mut first = true;
                    for (i, look) in LOOKS
                        .iter()
                        .enumerate()
                        .filter(|(_, l)| l.category() == category)
                    {
                        if first {
                            ui.label(RichText::new(category.label()).weak().small());
                            first = false;
                        }
                        ui.selectable_value(&mut state.look, i, look.name);
                    }
                }
            });
        if crate::theme::primary_button(ui, "Apply look").clicked() {
            apply_look(editor, &chosen, surface);
        }
    });
    ui.label(RichText::new(chosen.blurb).weak().small());
}

/// The generators in the "+ source" menu, one submenu per category.
pub fn generator_menu(ui: &mut egui::Ui, editor: &mut Editor) {
    for category in GeneratorCategory::ALL {
        ui.menu_button(category.label(), |ui| {
            for g in GeneratorKind::ALL
                .into_iter()
                .filter(|g| g.category() == category)
            {
                if ui.button(format!("Generator: {}", g.label())).clicked() {
                    editor.add_source(SourceKind::Generator {
                        pattern: g,
                        speed: 1.0,
                        color: [1.0; 4],
                    });
                    ui.close();
                }
            }
        });
    }
}
