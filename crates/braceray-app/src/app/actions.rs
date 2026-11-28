//! Applying the [`Action`]s the UI components return, and the native file dialog.

use std::sync::mpsc;

use braceray_core::job::JobHandle;
use eframe::egui;

use super::BracerayApp;
use crate::ui::goto::GoToDialog;
use crate::ui::settings_window::CacheInfo;
use crate::ui::{self, Action};

impl BracerayApp {
    pub(super) fn apply(&mut self, ctx: &egui::Context, action: Action) {
        if self.apply_copy(ctx, &action) {
            return;
        }
        match action {
            Action::OpenDialog => self.open_dialog(),
            Action::Open(path) => self.open_now(&path),
            Action::Focus(i) if i < self.tabs.len() => self.active = i,
            Action::Close(i) if i < self.tabs.len() => {
                // Dropping the tab drops its job handle, which cancels the job.
                self.tabs.remove(i);
                if self.active > i || self.active >= self.tabs.len() {
                    self.active = self.active.saturating_sub(1);
                }
                if self.tabs.is_empty() {
                    self.refresh_recent();
                }
            }
            Action::Cancel(i) => {
                if let Some(t) = self.tabs.get_mut(i) {
                    t.cancel();
                }
            }
            Action::Reload(i) => {
                let env = self.open_env();
                if let Some(t) = self.tabs.get_mut(i) {
                    t.reload(env);
                }
            }
            Action::DismissBanner(i) => {
                if let Some(t) = self.tabs.get_mut(i) {
                    t.banner = None;
                    t.browse_invalid = true;
                }
            }
            Action::OpenAnyway(i) => {
                if let Some(t) = self.tabs.get_mut(i) {
                    t.browse_invalid = true;
                }
            }
            Action::Copy(text) => self.copy_text(ctx, text),
            Action::RemoveRecent(path) => {
                self.settings.remove_recent(&path);
                self.missing_recent.remove(&path);
                self.store.save(&self.settings);
            }
            Action::ClearRecent => {
                self.settings.recent_files.clear();
                self.missing_recent.clear();
                self.store.save(&self.settings);
            }
            Action::SettingsChanged => {
                self.settings.validate();
                self.store.save(&self.settings);
                let repaint = self.repaint();
                for tab in &mut self.tabs {
                    tab.set_group_size(self.settings.tree_group_size, &repaint);
                }
            }
            Action::ClearCache => {
                if let Some(cache) = self.index_cache() {
                    self.cache_info = CacheInfo::Busy;
                    self.cache_job = Some(JobHandle::spawn(
                        "cache-clear",
                        0,
                        self.repaint(),
                        move |_| {
                            if let Err(e) = cache.clear() {
                                tracing::warn!("index cache: clear failed: {e}");
                            }
                            Ok(cache.total_bytes())
                        },
                    ));
                }
            }
            Action::ToggleSettings => {
                self.show_settings = !self.show_settings;
                if self.show_settings {
                    self.refresh_cache_size();
                }
            }
            Action::ToggleShortcuts => self.show_shortcuts = !self.show_shortcuts,
            Action::GoTo => {
                if self.active_tab().is_some_and(|t| t.tree.is_some()) {
                    self.goto.get_or_insert_with(GoToDialog::default);
                }
            }
            Action::ShowInRaw => {
                let repaint = self.repaint();
                if let Some(tab) = self.tabs.get_mut(self.active)
                    && let Some(tree) = &tab.tree
                {
                    tab.panes.show_selection(tree, &repaint);
                }
            }
            Action::FocusSearch => self.search.focus = true,
            Action::ToggleResults => self.search.results_open = !self.search.results_open,
            Action::TogglePane(view) => self.toggle_pane(view),
            Action::CloseSearch => {
                self.search.results_open = false;
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.panes.focus = crate::panes::Pane::Tree;
                }
            }
            Action::SearchHistory(text) => self.search.set_text(text),
            Action::SearchNext | Action::SearchPrev | Action::SearchJump(_) => {
                let repaint = self.repaint();
                let text = self.search.settled.text.clone();
                if self.settings.push_history(&text) {
                    self.store.save(&self.settings);
                }
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    match action {
                        Action::SearchJump(i) => tab.search_jump_index(i, &repaint),
                        Action::SearchNext => tab.search_step(true, &repaint),
                        _ => tab.search_step(false, &repaint),
                    }
                }
            }
            Action::OpenExternal(path) => open_external(&path),
            Action::Focus(_) | Action::Close(_) => {}
            Action::CopyPath(_)
            | Action::CopyValue(_)
            | Action::CopyKey
            | Action::CopyAnyway
            | Action::ExportValue
            | Action::CancelGuard
            | Action::CancelCopy => {} // apply_copy
        }
    }

    /// Draws the Go to dialog, starts requests and applies finished ones.
    pub(super) fn goto_frame(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.goto.as_mut() else {
            return;
        };
        let repaint = self
            .repaint
            .clone()
            .unwrap_or_else(braceray_core::job::no_repaint);
        let out = ui::goto::show(ctx, dialog);
        if out.close {
            self.goto = None;
            return;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else {
            self.goto = None;
            return;
        };
        if out.submit
            && let Some(tree) = &tab.tree
        {
            dialog.submit(tab.id, tree, &repaint);
        }
        if let Some(hit) = dialog.poll()
            && dialog.tab_id == Some(tab.id)
            && let Some(tree) = tab.tree.as_mut()
        {
            if let Some(raw) = tab.panes.raw.as_mut() {
                raw.reveal(tree, hit.span, &repaint);
            }
            tab.panes.reveal_in_tree(tree, hit.address, &repaint);
            tab.panes.focus = crate::panes::Pane::Raw;
            self.goto = None;
        } else if dialog.is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }

    /// Shows the native file dialog without blocking rendering: the dialog is created on
    /// the UI thread (macOS needs that) and awaited on a worker.
    fn open_dialog(&mut self) {
        if self.dialog.is_some() {
            return;
        }
        let future = rfd::AsyncFileDialog::new()
            .set_title("Open JSON file")
            .add_filter("JSON", &["json"])
            .add_filter("NDJSON", &["ndjson", "jsonl"])
            .add_filter("All files", &["*"])
            .pick_files();
        let (tx, rx) = mpsc::channel();
        let repaint = self.repaint();
        let spawned = std::thread::Builder::new()
            .name("file-dialog".to_owned())
            .spawn(move || {
                let files = pollster::block_on(future).unwrap_or_default();
                let _ = tx.send(files.into_iter().map(|f| f.path().to_path_buf()).collect());
                repaint();
            });
        match spawned {
            Ok(_) => self.dialog = Some(rx),
            Err(e) => tracing::warn!("can't start the file dialog thread: {e}"),
        }
    }
}

/// Opens `path` with the system's default app (not waiting for it).
fn open_external(path: &std::path::Path) {
    let mut cmd = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else {
        std::process::Command::new("xdg-open")
    };
    if let Err(e) = cmd.arg(path).spawn() {
        tracing::warn!("can't open {}: {e}", path.display());
    }
}

impl BracerayApp {
    /// Shows or hides a pane, keeping at least one visible, and moves the keyboard focus off
    /// a pane that was just hidden.
    fn toggle_pane(&mut self, view: ui::View) {
        let s = &mut self.settings;
        match view {
            ui::View::Tree => s.show_tree = !s.show_tree,
            ui::View::Raw => s.show_raw = !s.show_raw,
        }
        if !s.show_tree && !s.show_raw {
            match view {
                ui::View::Tree => s.show_raw = true,
                ui::View::Raw => s.show_tree = true,
            }
        }
        let (tree, raw) = (s.show_tree, s.show_raw);
        for tab in &mut self.tabs {
            let focus = &mut tab.panes.focus;
            if *focus == crate::panes::Pane::Tree && !tree {
                *focus = crate::panes::Pane::Raw;
            } else if *focus == crate::panes::Pane::Raw && !raw {
                *focus = crate::panes::Pane::Tree;
            }
        }
        self.store.save(&self.settings);
    }
}
