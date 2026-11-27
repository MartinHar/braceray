//! The window with no open file: drop zone, "Open file…" and recent files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use super::Action;
use crate::theme;

/// Text of the drop zone.
pub const DROP_HINT: &str = "Drop a JSON file here";

/// Draws the empty state. `missing` holds recent files that no longer exist.
pub fn show(
    ui: &mut egui::Ui,
    recent: &[PathBuf],
    missing: &HashSet<PathBuf>,
    actions: &mut Vec<Action>,
) {
    let accent = theme::accent(ui);
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.12).min(80.0));
        let width = ui.available_width().min(560.0);
        egui::Frame::new()
            .stroke(egui::Stroke::new(1.5, accent.gamma_multiply(0.6)))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(24, 40))
            .show(ui, |ui| {
                ui.set_width(width);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("{ }").size(40.0).color(accent).monospace());
                    ui.add_space(8.0);
                    ui.heading(DROP_HINT);
                    ui.label(
                        RichText::new("Any size. The file is never loaded into memory.").weak(),
                    );
                    ui.add_space(12.0);
                    if ui.button(RichText::new("Open file…").size(16.0)).clicked() {
                        actions.push(Action::OpenDialog);
                    }
                });
            });

        if recent.is_empty() {
            return;
        }
        ui.add_space(24.0);
        ui.allocate_ui(egui::vec2(width + 48.0, 0.0), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Recent files").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Clear recent").clicked() {
                        actions.push(Action::ClearRecent);
                    }
                });
            });
            ui.separator();
            for path in recent {
                recent_row(ui, path, missing.contains(path), actions);
            }
        });
    });
}

fn recent_row(ui: &mut egui::Ui, path: &Path, missing: bool, actions: &mut Vec<Action>) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let folder = path
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        if missing {
            ui.add_enabled(false, egui::Button::new(&name).frame(false))
                .on_disabled_hover_text(format!("{} no longer exists", path.display()));
            ui.label(RichText::new("(missing)").weak().italics());
            if ui.small_button("Remove").clicked() {
                actions.push(Action::RemoveRecent(path.to_path_buf()));
            }
        } else if ui
            .add(egui::Button::new(&name).frame(false))
            .on_hover_text(path.display().to_string())
            .clicked()
        {
            actions.push(Action::Open(path.to_path_buf()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(folder).weak().small()).truncate());
        });
    });
}
