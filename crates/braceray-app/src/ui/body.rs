//! The content area of a tab: opening, indexing progress (or the preview tree), the tree,
//! or an error.

use braceray_core::job::Repaint;
use eframe::egui::{self, RichText};

use super::{Action, banner, tree};
use crate::format;
use crate::tab::{OpenDocument, TabState};
use crate::theme;

/// Label of the cancel button while indexing.
pub const CANCEL: &str = "Cancel";

/// Prefix of the preview strip text ("Preview: indexing… 37 %").
pub const PREVIEW_LABEL: &str = "Preview: indexing…";

/// Draws tab `i`; returns the number of tree rows drawn (debug overlay).
pub fn show(
    ui: &mut egui::Ui,
    i: usize,
    tab: &mut OpenDocument,
    repaint: &Repaint,
    actions: &mut Vec<Action>,
) -> usize {
    if let Some(b) = &tab.banner {
        banner::show(ui, i, b, tab.browse_invalid, actions);
        ui.add_space(8.0);
    }
    let mut rows_drawn = 0;
    let indexing = matches!(tab.state, TabState::Indexing { .. });
    let blocked = matches!(&tab.state, TabState::Ready { loaded, .. } if !loaded.index.is_valid() && !tab.browse_invalid);
    if (indexing || matches!(tab.state, TabState::Ready { .. })) && !blocked {
        let fraction = tab.index_progress().map(|p| p.fraction);
        let tab_id = tab.id;
        if let Some(view) = tab.tree.as_mut() {
            if indexing {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let pct = fraction.unwrap_or(0.0) * 100.0;
                    ui.label(
                        RichText::new(format!("{PREVIEW_LABEL} {pct:.0} %"))
                            .color(theme::accent(ui)),
                    );
                    if ui.small_button(CANCEL).clicked() {
                        actions.push(Action::Cancel(i));
                    }
                });
            }
            let cx = tree::TreeContext {
                tab_id,
                repaint,
                rows_drawn: &mut rows_drawn,
            };
            tree::show(ui, view, cx, actions);
            if rows_drawn > 0 && tab.first_rows.is_none() {
                let took = tab.opened.elapsed();
                tracing::info!("{}: first tree rows {took:?} after opening", tab.name);
                tab.first_rows = Some(took);
            }
            return rows_drawn;
        }
    }
    match &tab.state {
        TabState::Opening => centered(ui, |ui| {
            ui.spinner();
            ui.label(format!("Opening {}…", tab.name));
        }),
        TabState::Indexing { doc, .. } => centered(ui, |ui| {
            ui.heading(&tab.name);
            ui.label(RichText::new(format::bytes(doc.len())).weak());
            ui.add_space(12.0);
            let p = tab.index_progress();
            let fraction = p.map_or(0.0, |p| p.fraction);
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_width(ui.available_width().min(480.0))
                    .fill(theme::accent(ui))
                    .text(format!("Indexing… {:.0} %", fraction * 100.0)),
            );
            if let Some(p) = p {
                let eta = p
                    .eta
                    .map(|e| format!(" · about {} left", format::duration(e)))
                    .unwrap_or_default();
                ui.label(
                    RichText::new(format!(
                        "{:.0} % · {}{eta}",
                        p.fraction * 100.0,
                        format::rate(p.bytes_per_sec)
                    ))
                    .weak(),
                );
            }
            ui.add_space(8.0);
            if ui.button(CANCEL).clicked() {
                actions.push(Action::Cancel(i));
            }
        }),
        TabState::Ready { loaded, .. } => {
            let invalid = !loaded.index.is_valid();
            centered(ui, |ui| {
                if let Some(summary) = tab.summary() {
                    ui.label(RichText::new(summary).size(16.0));
                }
                ui.add_space(6.0);
                let hint = if invalid && !tab.browse_invalid {
                    "This file isn't valid JSON. Choose “Open anyway” to browse the part before the error."
                } else {
                    "Nothing to show."
                };
                ui.label(RichText::new(hint).weak());
            });
        }
        TabState::Failed { message, details } => {
            let color = theme::error_color(ui.visuals().dark_mode);
            centered(ui, |ui| {
                ui.label(RichText::new(message).color(color).size(15.0));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Retry").clicked() {
                        actions.push(Action::Reload(i));
                    }
                    if ui.button("Copy details").clicked() {
                        actions.push(Action::Copy(details.clone()));
                    }
                    if ui.button("Close tab").clicked() {
                        actions.push(Action::Close(i));
                    }
                });
                banner::details(ui, ("failed_details", tab.id), details);
            });
        }
        TabState::Cancelled => centered(ui, |ui| {
            ui.label("Indexing cancelled.");
            if ui.button("Retry").clicked() {
                actions.push(Action::Reload(i));
            }
        }),
    }
    rows_drawn
}

fn centered(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.25).min(200.0));
        ui.set_max_width(640.0);
        add(ui);
    });
}
