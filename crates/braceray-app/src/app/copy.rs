//! Copy path / copy value (step 11): applying the copy actions, polling copy jobs, and
//! the breadcrumb bar and size guard of the active tab.

use braceray_core::copy::CopyStyle;
use braceray_core::job::Repaint;
use braceray_core::path::{PathFormat, format_path};
use eframe::egui;

use super::BracerayApp;
use crate::copy_job::{Request, What};
use crate::ui::{self, Action};

impl BracerayApp {
    /// Applies a copy action; returns `false` for other actions.
    pub(super) fn apply_copy(&mut self, ctx: &egui::Context, action: &Action) -> bool {
        let repaint = self.repaint();
        match action {
            Action::CopyPath(format) => self.copy_path(ctx, *format),
            Action::CopyValue(style) => self.copy_selection(ctx, *style, What::Value, &repaint),
            Action::CopyKey => {
                self.copy_selection(ctx, CopyStyle::StringContents, What::Key, &repaint);
            }
            Action::CopyAnyway => {
                if let Some(tab) = self.tabs.get_mut(self.active)
                    && let Some(doc) = tab.document().cloned()
                    && let Some(text) = tab.copy.copy_anyway(&doc, &repaint)
                {
                    self.copy_text(ctx, text);
                }
            }
            Action::ExportValue => {
                if let Some(tab) = self.tabs.get_mut(self.active)
                    && let Some(doc) = tab.document().cloned()
                {
                    tab.copy.export(&doc, &repaint);
                }
            }
            Action::CancelGuard => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.copy.guard = None;
                }
            }
            Action::CancelCopy => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.copy.cancel();
                }
            }
            _ => return false,
        }
        true
    }

    /// Copies the path of the active tab's selection; a given `format` becomes the
    /// default.
    fn copy_path(&mut self, ctx: &egui::Context, format: Option<PathFormat>) {
        if let Some(f) = format
            && f != self.settings.path_format
        {
            self.settings.path_format = f;
            self.store.save(&self.settings);
        }
        let format = format.unwrap_or(self.settings.path_format);
        let repaint = self.repaint();
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let Some(tree) = tab.tree.as_ref() else {
            return;
        };
        if let Some(job) = tree.deep_path_job() {
            // Tens of thousands of levels: read them on a worker (I-016).
            let js_root = self.settings.js_root.clone();
            tab.copy
                .spawn_text(What::Path(format.label()), &repaint, move || {
                    let path = job.run().ok_or(braceray_core::Error::Cancelled)?;
                    Ok(format_path(job.document().bytes(), &path, format, &js_root))
                });
            return;
        }
        let Some(path) = tree.selected_path() else {
            return;
        };
        let text = format_path(
            tree.document().bytes(),
            &path,
            format,
            &self.settings.js_root,
        );
        tab.copy.notify(format!("Copied {}", format.label()), false);
        self.copy_text(ctx, text);
    }

    /// Copies the selected value (or its key) of the active tab.
    fn copy_selection(
        &mut self,
        ctx: &egui::Context,
        style: CopyStyle,
        what: What,
        repaint: &Repaint,
    ) {
        let limit = self.settings.copy_limit_bytes;
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let Some(tree) = tab.tree.as_ref() else {
            return;
        };
        let Some(sel) = tree.selected_value() else {
            return;
        };
        let span = match what {
            What::Value => sel.child.value,
            What::Key => match sel.child.key {
                Some(k) if !sel.group => k,
                _ => return,
            },
            What::Path(_) => return, // paths go through `copy_path`
        };
        let doc = tree.document().clone();
        let req = Request { span, style, what };
        if let Some(text) = tab.copy.request(&doc, req, limit, repaint) {
            self.copy_text(ctx, text);
        }
    }

    /// Every frame: finished copies go to the clipboard.
    pub(super) fn poll_copies(&mut self, ctx: &egui::Context) {
        let done: Vec<String> = self.tabs.iter_mut().filter_map(|t| t.copy.poll()).collect();
        for text in done {
            self.copy_text(ctx, text);
        }
    }

    /// The breadcrumb bar of the active tab (under the search bar), if it has a tree.
    pub(super) fn breadcrumb_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let repaint = self.repaint();
        let format = self.settings.path_format;
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let Some(tree) = tab.tree.as_mut() else {
            return;
        };
        let cx = ui::breadcrumb::BarContext {
            tab_id: tab.id,
            repaint: &repaint,
            format,
            js_root: &self.settings.js_root,
        };
        egui::Panel::top(("breadcrumbs", tab.id))
            .frame(
                egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show(ui, |ui| {
                ui::breadcrumb::show(ui, tree, &mut tab.crumbs, &mut tab.copy, &cx, actions);
            });
    }

    /// The size guard of the active tab, while open.
    pub(super) fn copy_guard(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if let Some(tab) = self.tabs.get(self.active)
            && let Some(guard) = &tab.copy.guard
        {
            ui::copy_guard::show(ctx, tab.id, guard, actions);
        }
    }
}
