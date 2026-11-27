//! The "Keyboard shortcuts" dialog (architecture.md §7).

use eframe::egui::{self, RichText};

/// Every shortcut, as (keys with `Mod` for Ctrl/Cmd, action, available yet).
pub const SHORTCUTS: &[(&str, &str, bool)] = &[
    ("Mod+O", "Open file", true),
    ("Mod+W", "Close tab", true),
    ("?", "Keyboard shortcuts", true),
    ("Mod+F", "Search", false),
    ("F3 / Shift+F3", "Next / previous match", false),
    (
        "Enter / Shift+Enter",
        "Next / previous match (in the search box)",
        false,
    ),
    ("Mod+L", "Go to line", false),
    ("Mod+Shift+C", "Copy path", false),
    ("Mod+B", "Bookmark", false),
    ("← / →", "Collapse / expand", false),
    ("↑ / ↓", "Move selection", false),
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
