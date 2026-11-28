//! Bottom bar: file size · index state · line:col · memory.

use eframe::egui::{self, RichText};

use crate::format;
use crate::tab::{OpenDocument, TabState};

/// Bytes scanned at most to count the column in characters.
const COLUMN_SCAN: usize = 1 << 20;

/// Draws the status bar for `tab` (or the empty window). `memory` is the process's
/// resident memory, refreshed by the app every 2 s.
pub fn show(ui: &mut egui::Ui, tab: Option<&OpenDocument>, memory: Option<u64>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if let Some(tab) = tab {
            let summary = tab.summary();
            for (n, part) in parts(tab).into_iter().enumerate() {
                if n > 0 {
                    ui.label(RichText::new("·").weak());
                }
                let index_part = part.starts_with("Index:");
                let response = ui.label(RichText::new(part).small());
                // "Indexed 2.15 GB in 3.2 s · 19,603,166 nodes"
                if index_part && let Some(s) = &summary {
                    response.on_hover_text(s);
                }
            }
        } else {
            ui.label(RichText::new("No file open").small().weak());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some((live, peak)) = crate::alloc_stats::heap() {
                ui.label(
                    RichText::new(format!(
                        "Heap {} (peak {})",
                        format::bytes(live),
                        format::bytes(peak)
                    ))
                    .small()
                    .weak(),
                )
                .on_hover_text("alloc-stats build: heap bytes, mapped file and cache excluded");
            }
            if let Some(m) = memory {
                ui.label(
                    RichText::new(format!("Memory {}", format::bytes(m)))
                        .small()
                        .weak(),
                )
                .on_hover_text(
                    "Resident memory of the process. It includes the pages of the open \
                     files and index cache entries the OS has read into memory, which \
                     it can drop again when other apps need the space.",
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
                // Counting characters back to the line start is bounded: on a 2 GB
                // single-line file the column is given in bytes instead.
                let p = loaded.index.lines().position(doc.bytes(), off, COLUMN_SCAN);
                let col = if p.column_in_bytes { "Byte" } else { "Col" };
                out.push(format!(
                    "Ln {}, {col} {}",
                    format::group_digits(p.line),
                    format::group_digits(p.column)
                ));
            }
        }
        TabState::Failed { .. } => out.push("Can't open".to_owned()),
        TabState::Cancelled => out.push("Indexing cancelled".to_owned()),
    }
    let search = tab.search.count_text();
    if !search.is_empty() {
        out.push(format!("Search: {search}"));
    }
    out
}
