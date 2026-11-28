//! The "Keyboard shortcuts" dialog (architecture.md §7).

use eframe::egui::{self, RichText};

/// Every shortcut, as (keys with `Mod` for Ctrl/Cmd, action, available yet).
pub const SHORTCUTS: &[(&str, &str, bool)] = &[
    ("Mod+O", "Open file", true),
    ("Mod+W", "Close tab", true),
    ("?", "Keyboard shortcuts", true),
    ("Mod+F", "Search (focus the search box)", true),
    (
        "F3 / Shift+F3",
        "Next / previous match (macOS also ⌘G / ⌘⇧G)",
        true,
    ),
    (
        "Enter / Shift+Enter",
        "Next / previous match (in the search box)",
        true,
    ),
    (
        "Esc (search box)",
        "Clear the search; again: close it",
        true,
    ),
    ("Mod+Shift+F", "Show / hide the results panel", true),
    ("Mod+1 / Mod+2", "Show / hide the tree / the raw pane", true),
    (
        "↑ / ↓, Enter (results)",
        "Move in the results list / jump to the result",
        true,
    ),
    (
        "Mod+L",
        "Go to a line, line:col, @offset (@1.5g) or path ($.a[3])",
        true,
    ),
    (
        "Mod+Shift+C",
        "Copy the path of the selection (last format: JSONPath, jq, JavaScript, Pointer)",
        true,
    ),
    ("Mod+C (tree)", "Copy the selected value", true),
    ("Mod+B", "Bookmark", false),
    (
        "← / →",
        "Collapse (or go to parent) / expand (or go to first child)",
        true,
    ),
    (
        "↑ / ↓",
        "Tree: move selection · raw view: scroll a row",
        true,
    ),
    ("Home / End", "First / last row", true),
    ("PageUp / PageDown", "Move or scroll one page", true),
    (
        "Click / drag (raw view)",
        "Select the value in the tree / select text",
        true,
    ),
    ("Shift+click (raw view)", "Extend the text selection", true),
    (
        "Mod+C (raw view)",
        "Copy the selected source bytes (up to 50 MB)",
        true,
    ),
    ("Enter", "Expand / collapse", true),
    ("*", "Expand the selection and its children", true),
    ("Ctrl+Shift+D", "Debug overlay (frame times)", true),
];

/// Draws the dialog when `open`.
pub fn show(ctx: &egui::Context, open: &mut bool) {
    let modifier = if ctx.os() == egui::os::OperatingSystem::Mac {
        "⌘"
    } else {
        "Ctrl+"
    };
    egui::Window::new("Keyboard shortcuts")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            egui::Grid::new("shortcuts_grid")
                .num_columns(2)
                .spacing([24.0, 6.0])
                .show(ui, |ui| {
                    for (keys, action, available) in SHORTCUTS {
                        let keys = keys.replace("Mod+", modifier);
                        ui.label(RichText::new(keys).monospace().strong());
                        if *available {
                            ui.label(*action);
                        } else {
                            ui.label(RichText::new(format!("{action} (coming soon)")).weak());
                        }
                        ui.end_row();
                    }
                });
        });
}
