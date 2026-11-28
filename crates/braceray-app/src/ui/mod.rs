//! UI components. Each draws from app state and returns [`Action`]s; the app applies
//! them after drawing, so components never need `&mut` access to the whole app.

pub mod banner;
pub mod body;
pub mod breadcrumb;
pub mod copy_guard;
pub mod debug_overlay;
pub mod empty;
pub mod goto;
pub mod raw;
mod raw_input;
pub mod results;
pub mod search;
pub mod settings_window;
pub mod shortcuts;
pub mod status;
pub mod toolbar;
pub mod tree;
pub mod tree_row;
pub mod vlist;

use std::path::PathBuf;

use braceray_core::copy::CopyStyle;
use braceray_core::path::PathFormat;
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
    /// Open the Go to dialog.
    GoTo,
    /// Scroll the raw view to the tree selection.
    ShowInRaw,
    /// Focus the search field and select its text.
    FocusSearch,
    /// Next match from the selection.
    SearchNext,
    /// Previous match from the selection.
    SearchPrev,
    /// Jump to stored match `n` of the active tab.
    SearchJump(usize),
    /// Show or hide the results panel.
    ToggleResults,
    /// Esc in an empty search field: hide the results and leave the field.
    CloseSearch,
    /// Search for a text from the history.
    SearchHistory(String),
    /// Open a file with the system's default app (the "Copy all paths" file).
    OpenExternal(PathBuf),
    /// Copy the path of the tree selection: in this format (which becomes the default),
    /// or in the last one.
    CopyPath(Option<PathFormat>),
    /// Copy the selected value in a style.
    CopyValue(CopyStyle),
    /// Copy the selected member's key (decoded).
    CopyKey,
    /// The size guard's "Copy anyway".
    CopyAnyway,
    /// The size guard's "Export…".
    ExportValue,
    /// The size guard's "Cancel".
    CancelGuard,
    /// Stop a running copy or export.
    CancelCopy,
}

/// Sets the accessible name of an icon-only widget (screen readers, kittest, egui_mcp).
pub fn label_icon(response: &egui::Response, role: Role, label: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(role, response.enabled(), label));
}
