//! UI components. Each draws from app state and returns [`Action`]s; the app applies
//! them after drawing, so components never need `&mut` access to the whole app.

pub mod banner;
pub mod body;
pub mod empty;
pub mod settings_window;
pub mod shortcuts;
pub mod status;
pub mod toolbar;

use std::path::PathBuf;

use eframe::egui::{self, Role};

/// Something the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Show the file dialog.
    OpenDialog,
    /// Open (or focus) a file.
    Open(PathBuf),
    /// Make tab `n` active.
    Focus(usize),
    /// Close tab `n`.
    Close(usize),
    /// Cancel indexing of tab `n`.
    Cancel(usize),
    /// Re-open tab `n` from disk.
    Reload(usize),
    /// Hide the banner of tab `n`.
    DismissBanner(usize),
    /// Browse the valid prefix of invalid tab `n`.
    OpenAnyway(usize),
    /// Copy text to the clipboard.
    Copy(String),
    /// Remove a file from the recent list.
    RemoveRecent(PathBuf),
    /// Empty the recent list.
    ClearRecent,
    /// Settings were edited (save and apply them).
    SettingsChanged,
    /// Delete every index cache entry.
    ClearCache,
    /// Open or close the settings window.
    ToggleSettings,
    /// Open or close the keyboard shortcuts dialog.
    ToggleShortcuts,
}

/// Sets the accessible name of an icon-only widget (screen readers, kittest, egui_mcp).
pub fn label_icon(response: &egui::Response, role: Role, label: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(role, response.enabled(), label));
}
