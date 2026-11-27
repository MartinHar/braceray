//! Applying the [`Action`]s the UI components return, and the native file dialog.

use std::sync::mpsc;

use braceray_core::job::JobHandle;
use eframe::egui;

use super::BracerayApp;
use crate::ui::Action;
use crate::ui::settings_window::CacheInfo;

impl BracerayApp {
    pub(super) fn apply(&mut self, ctx: &egui::Context, action: Action) {
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
            Action::Copy(text) => ctx.copy_text(text),
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
            Action::Focus(_) | Action::Close(_) => {}
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
