//! The content area of a tab: opening, indexing progress (or the preview tree), the tree,
//! or an error.

use braceray_core::job::Repaint;
use eframe::egui::{self, RichText};

use super::{Action, banner, raw, tree};
use crate::format;
use crate::panes::Pane;
use crate::tab::{OpenDocument, TabState};
use crate::theme;

/// Label of the cancel button while indexing.
pub const CANCEL: &str = "Cancel";

/// How long a raw view notice (e.g. the copy limit) stays.
const NOTICE: std::time::Duration = std::time::Duration::from_secs(4);

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
            let focus = tab.panes.focus;
            let full_width = ui.available_width();
            let tree_panel = egui::Panel::left(("tree_pane", tab_id))
                .resizable(true)
                .default_size((full_width * 0.45).max(260.0))
                .size_range(200.0..=(full_width - 200.0).max(200.0))
                .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                    right: 6,
                    ..egui::Margin::ZERO
                }))
                .show(ui, |ui| {
                    let cx = tree::TreeContext {
                        tab_id,
                        repaint,
                        rows_drawn: &mut rows_drawn,
                        keys: focus == Pane::Tree,
                    };
                    tree::show(ui, view, cx, actions);
                });
            let pressed =
                ui.input(|i| i.pointer.primary_pressed() || i.pointer.secondary_pressed());
            if pressed && ui.rect_contains_pointer(tree_panel.response.rect) {
                tab.panes.focus = Pane::Tree;
            }
            if let Some(raw_view) = tab.panes.raw.as_mut() {
                let cx = raw::RawContext {
                    tab_id,
                    repaint,
                    focused: focus == Pane::Raw,
                };
                let out = raw::show(ui, raw_view, view, cx);
                rows_drawn += out.rows_drawn;
                if out.focus {
                    tab.panes.focus = Pane::Raw;
                }
                if out.go_to {
                    actions.push(Action::GoTo);
                }
                if out.copy {
                    match raw_view.copy_text(view.nav().bytes()) {
                        Some(Ok(text)) => actions.push(Action::Copy(text)),
                        Some(Err(message)) => {
                            raw_view.notice = Some((message, std::time::Instant::now() + NOTICE));
                        }
                        None => {}
                    }
                }
                if let Some(off) = out.clicked {
                    tab.panes.locate(view, off, repaint);
                }
            }
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
