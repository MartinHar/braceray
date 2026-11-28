//! Top bar: open button, one tab per file, settings and help.

use eframe::egui::{self, RichText, Role};

use super::{Action, View, label_icon};
use crate::tab::{OpenDocument, TabState};
use crate::theme;

/// Draws the toolbar and the tab strip.
pub fn show(
    ui: &mut egui::Ui,
    tabs: &[OpenDocument],
    active: usize,
    panes: (bool, bool),
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        if ui
            .button("Open file…")
            .on_hover_text(format!("Open a JSON file ({})", shortcut(ui, "O")))
            .clicked()
        {
            actions.push(Action::OpenDialog);
        }
        ui.separator();

        egui::ScrollArea::horizontal()
            .id_salt("tab_strip")
            .max_width(ui.available_width() - 210.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (i, tab) in tabs.iter().enumerate() {
                        tab_button(ui, i, tab, i == active, actions);
                    }
                });
            });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let help = ui.button("?").on_hover_text("Keyboard shortcuts (?)");
            label_icon(&help, Role::Button, "Keyboard shortcuts");
            if help.clicked() {
                actions.push(Action::ToggleShortcuts);
            }
            let gear = ui.button("Settings").on_hover_text("Settings");
            if gear.clicked() {
                actions.push(Action::ToggleSettings);
            }
            ui.separator();
            // Right to left: Raw is drawn first so the order reads "Tree Raw".
            for (view, shown) in [(View::Raw, panes.1), (View::Tree, panes.0)] {
                let key = match view.key() {
                    egui::Key::Num1 => "1",
                    _ => "2",
                };
                let verb = if shown { "Hide" } else { "Show" };
                let b = ui
                    .add(egui::Button::selectable(shown, view.label()))
                    .on_hover_text(format!(
                        "{verb} the {} pane ({})",
                        view.label().to_lowercase(),
                        shortcut(ui, key)
                    ));
                label_icon(&b, Role::Button, &format!("{} pane", view.label()));
                if b.clicked() {
                    actions.push(Action::TogglePane(view));
                }
            }
        });
    });
}

fn tab_button(
    ui: &mut egui::Ui,
    i: usize,
    tab: &OpenDocument,
    selected: bool,
    actions: &mut Vec<Action>,
) {
    let marker = match tab.state {
        TabState::Opening | TabState::Indexing { .. } => "… ",
        TabState::Failed { .. } => "! ",
        _ if tab.banner.is_some() => "! ",
        _ => "",
    };
    let frame = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(6, 2))
        .corner_radius(theme::ROUNDING)
        .fill(if selected {
            ui.visuals().selection.bg_fill
        } else {
            ui.visuals().faint_bg_color
        });
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let text = RichText::new(format!("{marker}{}", tab.name));
            let label = ui
                .add(egui::Button::selectable(selected, text).frame(false))
                .on_hover_text(tab.path.display().to_string());
            if label.clicked() {
                actions.push(Action::Focus(i));
            }
            if label.middle_clicked() {
                actions.push(Action::Close(i));
            }
            let close = ui
                .add(egui::Button::new("×").frame(false).small())
                .on_hover_text("Close tab");
            label_icon(&close, Role::Button, &format!("Close {}", tab.name));
            if close.clicked() {
                actions.push(Action::Close(i));
            }
        });
    });
}

/// `"⌘O"` on macOS, `"Ctrl+O"` elsewhere.
pub fn shortcut(ui: &egui::Ui, key: &str) -> String {
    if ui.ctx().os() == egui::os::OperatingSystem::Mac {
        format!("⌘{key}")
    } else {
        format!("Ctrl+{key}")
    }
}
