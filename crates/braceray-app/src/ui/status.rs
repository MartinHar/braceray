//! Bottom bar: file size · index state · line:col · memory.

use braceray_core::nav::Navigator;
use eframe::egui::{self, RichText};

use crate::format;
use crate::tab::{OpenDocument, TabState};

/// Draws the status bar for `tab` (or the empty window). `memory` is the process's
/// resident memory, refreshed by the app every 2 s.
pub fn show(ui: &mut egui::Ui, tab: Option<&OpenDocument>, memory: Option<u64>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if let Some(tab) = tab {
            for (n, part) in parts(tab).into_iter().enumerate() {
                if n > 0 {
                    ui.label(RichText::new("·").weak());
                }
                ui.label(RichText::new(part).small());
            }
        } else {
            ui.label(RichText::new("No file open").small().weak());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(m) = memory {
                ui.label(
                    RichText::new(format!("Memory {}", format::bytes(m)))
                        .small()
                        .weak(),
                );
            }
        });
    });
}

/// The text parts for `tab`, e.g. `["2.00 GB", "Index: from cache (0.01 s)", "Ln 1, Col 1"]`.
pub fn parts(tab: &OpenDocument) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(doc) = tab.document() {
        out.push(format::bytes(doc.len()));
    }
    match &tab.state {
        TabState::Opening => out.push("Opening…".to_owned()),
        TabState::Indexing { .. } => {
            let pct = tab.index_progress().map_or(0.0, |p| p.fraction * 100.0);
            out.push(format!("Indexing {pct:.0} %"));
        }
        TabState::Ready { doc, loaded } => {
            out.push(loaded.status_text());
            if !loaded.index.is_valid() {
                out.push("Invalid JSON".to_owned());
            }
            if let Some(off) = tab.selection {
                let (line, col) = Navigator::new(doc.bytes(), &loaded.index).line_col(off);
                out.push(format!(
                    "Ln {}, Col {}",
                    format::group_digits(line),
                    format::group_digits(col)
                ));
            }
        }
        TabState::Failed { .. } => out.push("Can't open".to_owned()),
        TabState::Cancelled => out.push("Indexing cancelled".to_owned()),
    }
    out
}
