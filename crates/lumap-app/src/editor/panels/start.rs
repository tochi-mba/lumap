//! The start page: what lumap shows before a show is open, or from Menu ▸ Home. It replaces the
//! old welcome window that sat on top of a live editor.

use super::super::MonitorInfo;
use super::Actions;
use crate::settings::Settings;
use crate::update::CheckUi;
use eframe::egui::{self, RichText};

/// Draws the start page over the whole window.
pub fn start_page(
    ui: &mut egui::Ui,
    monitors: &[MonitorInfo],
    settings: &Settings,
    actions: &mut Actions,
    check: &CheckUi,
) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(crate::theme::INK)
                .inner_margin(egui::Margin::same(32)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.set_max_width(720.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        header(ui);
                        ui.add_space(20.0);
                        start_card(ui, monitors, actions);
                        ui.add_space(16.0);
                        recent_card(ui, settings, actions);
                        ui.add_space(16.0);
                        card(ui, |ui| super::looks::welcome_row(ui, actions));
                        ui.add_space(16.0);
                        footer(ui, settings, actions, check);
                    });
                });
            });
        });
}

fn header(ui: &mut egui::Ui) {
    ui.label(
        RichText::new("REX TECHNOLOGIES")
            .size(11.0)
            .family(crate::theme::strong_family())
            .color(crate::theme::SIGNAL),
    );
    ui.label(
        RichText::new("Welcome to lumap")
            .size(34.0)
            .family(crate::theme::strong_family()),
    );
    ui.label(
        RichText::new(
            "Map light onto walls, objects and stages with one projector or many. Four steps: \
             set up your projectors, map the surfaces, choose what they show, go live.",
        )
        .weak(),
    );
}

/// A rounded panel on the ink page.
fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(crate::theme::PANEL)
        .stroke(egui::Stroke::new(1.0, crate::theme::LINE))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn start_card(ui: &mut egui::Ui, monitors: &[MonitorInfo], actions: &mut Actions) {
    card(ui, |ui| {
        crate::theme::section_header(ui, "Start a show");
        let extra = monitors.len().saturating_sub(1);
        if extra > 0 {
            ui.label(format!(
                "{} displays found. lumap makes a projector for each extra display, with a test \
                 grid on it so you can see where it lands.",
                monitors.len()
            ));
            ui.add_space(6.0);
            if crate::theme::primary_button(ui, &format!("Create {extra} projector(s)")).clicked() {
                actions.create_outputs = true;
            }
        } else {
            ui.label(
                RichText::new(
                    "Only one display found. Plug in a projector and reopen lumap, or map on \
                     this screen for now.",
                )
                .weak(),
            );
        }
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Start from the demo").clicked() {
                actions.leave_home = true;
            }
            if ui.button("Open a show…").clicked() {
                actions.open = true;
            }
            if ui.button("Add media…").clicked() {
                actions.add_media = true;
                actions.leave_home = true;
            }
        });
    });
}

fn recent_card(ui: &mut egui::Ui, settings: &Settings, actions: &mut Actions) {
    let recent = settings.existing_recent();
    if recent.is_empty() {
        return;
    }
    card(ui, |ui| {
        crate::theme::section_header(ui, "Recent shows");
        for p in recent.iter().take(6) {
            let name = p.file_name().map_or_else(
                || p.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            ui.horizontal(|ui| {
                if ui.button(name).clicked() {
                    actions.open_path = Some(p.clone());
                }
                if let Some(dir) = p.parent() {
                    ui.label(RichText::new(dir.display().to_string()).weak().small());
                }
            });
        }
    });
}

fn footer(ui: &mut egui::Ui, settings: &Settings, actions: &mut Actions, check: &CheckUi) {
    ui.horizontal(|ui| {
        let mut show = !settings.skip_welcome;
        if ui
            .checkbox(&mut show, "Show this page when lumap opens")
            .changed()
        {
            actions.start_page = Some(show);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            super::settings::check_button(ui, check, actions, "Check for updates");
            ui.label(
                RichText::new(concat!("lumap ", env!("CARGO_PKG_VERSION")))
                    .weak()
                    .small(),
            );
        });
    });
    if check.checking || check.detail.contains("up to date") || check.detail.contains("Couldn't check") {
        ui.label(RichText::new(&check.detail).weak().small());
    });
}
