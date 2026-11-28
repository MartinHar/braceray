//! App-wide keyboard shortcuts and file drag & drop.

use std::path::PathBuf;

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

use super::BracerayApp;
use crate::theme;
use crate::ui::Action;

impl BracerayApp {
    pub(super) fn keyboard(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let open = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
        let close = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
        let goto = KeyboardShortcut::new(Modifiers::COMMAND, Key::L);
        let mac = ctx.os() == egui::os::OperatingSystem::Mac;
        let debug = [
            KeyboardShortcut::new(Modifiers::CTRL | Modifiers::SHIFT, Key::D),
            KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::D),
        ];
        ctx.input_mut(|i| {
            if debug.iter().any(|d| i.consume_shortcut(d)) {
                self.frame_stats.open = !self.frame_stats.open;
            }
            if i.consume_shortcut(&open) {
                actions.push(Action::OpenDialog);
            }
            if i.consume_shortcut(&close) && !self.tabs.is_empty() {
                actions.push(Action::Close(self.active));
            }
            if i.consume_shortcut(&goto) {
                actions.push(Action::GoTo);
            }
            // Shift variants first: a pattern without Shift also matches with Shift.
            let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
            if i.consume_shortcut(&KeyboardShortcut::new(cmd_shift, Key::C)) {
                actions.push(Action::CopyPath(None));
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd_shift, Key::F)) {
                actions.push(Action::ToggleResults);
            }
            for view in [crate::ui::View::Tree, crate::ui::View::Raw] {
                if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, view.key())) {
                    actions.push(Action::TogglePane(view));
                }
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::F)) {
                actions.push(Action::FocusSearch);
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::SHIFT, Key::F3)) {
                actions.push(Action::SearchPrev);
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F3)) {
                actions.push(Action::SearchNext);
            }
            if mac {
                let cmd = Modifiers::MAC_CMD;
                if i.consume_shortcut(&KeyboardShortcut::new(cmd | Modifiers::SHIFT, Key::G)) {
                    actions.push(Action::SearchPrev);
                }
                if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::G)) {
                    actions.push(Action::SearchNext);
                }
            }
        });
        let typing = ctx.egui_wants_keyboard_input();
        let help = ctx.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Text(t) if t == "?"))
        });
        if help && !typing {
            actions.push(Action::ToggleShortcuts);
        }
    }

    pub(super) fn drag_and_drop(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let (hovering, dropped) = ctx.input(|i| {
            let dropped: Vec<PathBuf> = i
                .raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect();
            (!i.raw.hovered_files.is_empty(), dropped)
        });
        actions.extend(dropped.into_iter().map(Action::Open));
        if hovering {
            let rect = ctx.content_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("drop_zone"),
            ));
            let accent = if ctx.theme() == egui::Theme::Dark {
                theme::ACCENT_DARK
            } else {
                theme::ACCENT_LIGHT
            };
            painter.rect(
                rect.shrink(6.0),
                12.0,
                accent.gamma_multiply(0.12),
                egui::Stroke::new(2.0, accent),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Drop to open",
                egui::FontId::proportional(28.0),
                accent,
            );
        }
    }
}
