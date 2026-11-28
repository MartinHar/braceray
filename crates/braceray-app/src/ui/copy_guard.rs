//! The size question before copying a big value: "This value is 1.2 GB. Copying it may
//! use a lot of memory. Export to file instead?" → Export… / Copy anyway / Cancel.

use eframe::egui;

use super::Action;
use crate::copy_job::Guard;
use crate::format;

/// Button that writes the value to a file instead.
pub const EXPORT: &str = "Export…";
/// Button that copies anyway.
pub const COPY_ANYWAY: &str = "Copy anyway";
/// Button that drops the copy.
pub const CANCEL: &str = "Cancel";

/// The question's text.
pub fn message(size: u64) -> String {
    format!(
        "This value is {}. Copying it may use a lot of memory. Export to file instead?",
        format::bytes(size)
    )
}

/// Draws the dialog for `guard` of tab `tab_id`.
pub fn show(ctx: &egui::Context, tab_id: u64, guard: &Guard, actions: &mut Vec<Action>) {
    let modal = egui::Modal::new(egui::Id::unique(("copy_guard", tab_id))).show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.heading("Big value");
        ui.add_space(4.0);
        ui.label(message(guard.request.size()));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button(EXPORT).clicked() {
                actions.push(Action::ExportValue);
            }
            if ui.button(COPY_ANYWAY).clicked() {
                actions.push(Action::CopyAnyway);
            }
            if ui.button(CANCEL).clicked() {
                actions.push(Action::CancelGuard);
            }
        });
    });
    if modal.should_close()
        && !actions
            .iter()
            .any(|a| matches!(a, Action::ExportValue | Action::CopyAnyway))
    {
        actions.push(Action::CancelGuard);
    }
}
