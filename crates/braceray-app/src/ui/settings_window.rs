//! The settings window (gear icon).

use eframe::egui::{self, RichText};

use super::Action;
use crate::format;
use braceray_core::path::PathFormat;

use crate::settings::{
    CACHE_MAX_RANGE, CACHE_MIN_FILE_RANGE, COPY_LIMIT_RANGE, GROUP_SIZE_RANGE, MAX_RESULTS_RANGE,
    Settings, ThemeChoice, valid_js_root,
};

/// What the settings window shows about the index cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheInfo {
    /// No cache dir (no home directory in the environment).
    Unavailable,
    /// Measuring or clearing.
    Busy,
    /// Bytes in use.
    Size(u64),
}

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

/// Draws the settings window when `open`. Edits `settings` in place and pushes
/// [`Action::SettingsChanged`] when anything changed.
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    settings: &mut Settings,
    cache: CacheInfo,
    actions: &mut Vec<Action>,
) {
    let before = settings.clone();
    egui::Window::new("Settings")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .show(ctx, |ui| {
            section(ui, "Appearance");
            ui.horizontal(|ui| {
                ui.label("Theme");
                for choice in ThemeChoice::ALL {
                    ui.selectable_value(&mut settings.theme, choice, choice.label());
                }
            });

            section(ui, "Index cache");
            ui.checkbox(&mut settings.index_cache_enabled, "Cache indexes on disk")
                .on_hover_text("Re-opening a file loads its index instead of rebuilding it.");
            ui.add_enabled_ui(settings.index_cache_enabled, |ui| {
                egui::Grid::new("cache_grid").num_columns(2).show(ui, |ui| {
                    let l = ui.label("Max cache size (GB)");
                    gib_slider(
                        ui,
                        &mut settings.index_cache_max_bytes,
                        CACHE_MAX_RANGE,
                        GIB,
                    )
                    .labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label("Min file size (MB)");
                    gib_slider(
                        ui,
                        &mut settings.index_cache_min_file_bytes,
                        CACHE_MIN_FILE_RANGE,
                        MIB,
                    )
                    .labelled_by(l.id);
                    ui.end_row();
                });
            });
            ui.horizontal(|ui| match cache {
                CacheInfo::Unavailable => {
                    ui.label(RichText::new("No cache folder on this system.").weak());
                }
                CacheInfo::Busy => {
                    ui.add_enabled(false, egui::Button::new("Clear cache"));
                    ui.spinner();
                }
                CacheInfo::Size(bytes) => {
                    if ui
                        .button(format!("Clear cache ({})", format::bytes(bytes)))
                        .clicked()
                    {
                        actions.push(Action::ClearCache);
                    }
                }
            });

            section(ui, "Search");
            egui::Grid::new("search_grid")
                .num_columns(2)
                .show(ui, |ui| {
                    let l = ui.label("Max stored results");
                    ui.add(
                        egui::DragValue::new(&mut settings.max_stored_results)
                            .range(MAX_RESULTS_RANGE.0..=MAX_RESULTS_RANGE.1)
                            .speed(10_000),
                    )
                    .labelled_by(l.id);
                    ui.end_row();
                });

            section(ui, "Tree");
            egui::Grid::new("tree_grid").num_columns(2).show(ui, |ui| {
                let l = ui.label("Group size");
                ui.add(
                    egui::DragValue::new(&mut settings.tree_group_size)
                        .range(GROUP_SIZE_RANGE.0..=GROUP_SIZE_RANGE.1)
                        .speed(10),
                )
                .labelled_by(l.id);
                ui.end_row();
            });

            section(ui, "Copy");
            egui::Grid::new("copy_grid").num_columns(2).show(ui, |ui| {
                ui.label("Path format");
                ui.horizontal_wrapped(|ui| {
                    for f in PathFormat::ALL {
                        ui.selectable_value(&mut settings.path_format, f, f.label());
                    }
                });
                ui.end_row();
                let l = ui.label("JavaScript root name");
                // Typed text lives in a buffer; only valid names reach the settings.
                let buf_id = l.id.with("js_root_buffer");
                let mut root = ui
                    .data_mut(|d| d.get_temp::<String>(buf_id))
                    .unwrap_or_else(|| settings.js_root.clone());
                let edit = ui
                    .add(egui::TextEdit::singleline(&mut root).desired_width(140.0))
                    .labelled_by(l.id)
                    .on_hover_text("data.orders[0]: letters, digits, _, $ and dots");
                if valid_js_root(root.trim()) {
                    settings.js_root = root.trim().to_owned();
                }
                if edit.has_focus() {
                    ui.data_mut(|d| d.insert_temp(buf_id, root));
                } else {
                    ui.data_mut(|d| d.remove::<String>(buf_id));
                }
                ui.end_row();
                let l = ui.label("Ask before copying over (MB)");
                gib_slider(ui, &mut settings.copy_limit_bytes, COPY_LIMIT_RANGE, MIB)
                    .labelled_by(l.id)
                    .on_hover_text("Bigger values ask first and offer to export to a file.");
                ui.end_row();
            });

            ui.add_space(8.0);
            if ui.button("Reset to defaults").clicked() {
                let recent = std::mem::take(&mut settings.recent_files);
                *settings = Settings {
                    recent_files: recent,
                    ..Settings::default()
                };
            }
        });
    if *settings != before {
        actions.push(Action::SettingsChanged);
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).strong());
    ui.separator();
}

/// A drag value in `unit`s (GiB or MiB) that edits a byte count within `range`.
fn gib_slider(
    ui: &mut egui::Ui,
    bytes: &mut u64,
    (lo, hi): (u64, u64),
    unit: u64,
) -> egui::Response {
    let mut value = *bytes as f64 / unit as f64;
    let response = ui.add(
        egui::DragValue::new(&mut value)
            .range(lo as f64 / unit as f64..=hi as f64 / unit as f64)
            .speed(0.1)
            .max_decimals(1),
    );
    if response.changed() {
        *bytes = ((value * unit as f64).round() as u64).clamp(lo, hi);
    }
    response
}
