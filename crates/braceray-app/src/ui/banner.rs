//! Message bar with a coloured stripe, copyable details and actions.

use eframe::egui::{self, RichText};

use super::Action;
use crate::tab::{Banner, BannerKind};
use crate::theme;

/// Draws `banner` of tab `tab`. `browse_invalid` hides "Open anyway" once chosen.
pub fn show(
    ui: &mut egui::Ui,
    tab: usize,
    banner: &Banner,
    browse_invalid: bool,
    actions: &mut Vec<Action>,
) {
    let dark = ui.visuals().dark_mode;
    let color = match banner.kind {
        BannerKind::InvalidJson => theme::error_color(dark),
        BannerKind::FileChanged => theme::warn_color(dark),
    };
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.6)))
        .corner_radius(theme::ROUNDING)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&banner.message).color(color).strong());
            });
            ui.horizontal(|ui| {
                match banner.kind {
                    BannerKind::InvalidJson if !browse_invalid => {
                        if ui.button("Open anyway").clicked() {
                            actions.push(Action::OpenAnyway(tab));
                        }
                    }
                    BannerKind::InvalidJson => {}
                    BannerKind::FileChanged => {
                        if ui.button("Reload").clicked() {
                            actions.push(Action::Reload(tab));
                        }
                    }
                }
                if ui.button("Copy details").clicked() {
                    actions.push(Action::Copy(banner.details.clone()));
                }
                if ui.button("Dismiss").clicked() {
                    actions.push(Action::DismissBanner(tab));
                }
            });
            details(ui, ("banner_details", tab), &banner.details);
        });
}

/// A collapsible, selectable "Details" block.
pub fn details(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, text: &str) {
    egui::CollapsingHeader::new("Details")
        .id_salt(id)
        .default_open(false)
        .show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).monospace()).selectable(true));
        });
}
