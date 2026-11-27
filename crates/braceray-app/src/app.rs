use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use braceray_core::cache::IndexCache;
use braceray_core::job::{JobEvent, JobHandle, Repaint};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

use crate::settings::{Settings, SettingsStore};
use crate::tab::{OpenDocument, OpenEnv, canonical_key};
use crate::theme;
use crate::ui::settings_window::CacheInfo;
use crate::ui::{self, Action};

/// Text shown in the empty window before a file is opened.
pub const DROP_HINT: &str = ui::empty::DROP_HINT;

/// How often the memory readout in the status bar refreshes.
const MEMORY_EVERY: Duration = Duration::from_secs(2);
/// Repaint interval while a job runs, so progress moves smoothly.
const PROGRESS_REPAINT: Duration = Duration::from_millis(50);

/// Where the app keeps its files. Tests use temp dirs; `None` disables that part.
#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    /// The settings file.
    pub settings_path: Option<PathBuf>,
    /// The index cache dir.
    pub cache_dir: Option<PathBuf>,
    /// Test hook: while `true`, open jobs pause after mapping the file.
    #[doc(hidden)]
    pub hold_indexing: Option<Arc<AtomicBool>>,
}

impl AppConfig {
    /// The OS locations (config dir and cache dir).
    pub fn from_env() -> Self {
        AppConfig {
            settings_path: crate::settings::default_path(),
            cache_dir: IndexCache::default_dir(),
            hold_indexing: None,
        }
    }
}

/// Top-level application state.
pub struct BracerayApp {
    /// Current settings (persisted on change).
    pub settings: Settings,
    store: SettingsStore,
    cache_dir: Option<PathBuf>,
    hold_indexing: Option<Arc<AtomicBool>>,
    /// Open files.
    pub tabs: Vec<OpenDocument>,
    /// Index of the visible tab.
    pub active: usize,
    next_id: u64,
    pending_opens: Vec<PathBuf>,
    dialog: Option<Receiver<Vec<PathBuf>>>,
    /// The settings window is open.
    pub show_settings: bool,
    /// The keyboard shortcuts dialog is open.
    pub show_shortcuts: bool,
    cache_info: CacheInfo,
    cache_job: Option<JobHandle<(), u64>>,
    missing_recent: HashSet<PathBuf>,
    recent_job: Option<JobHandle<(), HashSet<PathBuf>>>,
    memory: Option<u64>,
    memory_at: Option<Instant>,
    repaint: Option<Repaint>,
    theme_applied: Option<crate::settings::ThemeChoice>,
    focused: bool,
}

impl Default for BracerayApp {
    /// An app that keeps settings in memory only and has no index cache.
    fn default() -> Self {
        Self::new(AppConfig::default())
    }
}

impl std::fmt::Debug for BracerayApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BracerayApp")
            .field("tabs", &self.tabs.len())
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

impl BracerayApp {
    /// Loads the settings from `config` (a small file, read once).
    pub fn new(config: AppConfig) -> Self {
        let store = SettingsStore::new(config.settings_path);
        let settings = store.load();
        BracerayApp {
            settings,
            store,
            cache_dir: config.cache_dir,
            hold_indexing: config.hold_indexing,
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            pending_opens: Vec::new(),
            dialog: None,
            show_settings: false,
            show_shortcuts: false,
            cache_info: CacheInfo::Unavailable,
            cache_job: None,
            missing_recent: HashSet::new(),
            recent_job: None,
            memory: None,
            memory_at: None,
            repaint: None,
            theme_applied: None,
            focused: true,
        }
    }

    /// Opens `path` (or focuses its tab) on the next frame. Used for the command line,
    /// drops, the dialog, recent files and tests.
    pub fn open_path(&mut self, path: impl Into<PathBuf>) {
        self.pending_opens.push(path.into());
    }

    /// The settings store (tests flush it).
    pub fn settings_store(&self) -> &SettingsStore {
        &self.store
    }

    /// The visible tab.
    pub fn active_tab(&self) -> Option<&OpenDocument> {
        self.tabs.get(self.active)
    }

    /// `true` while any tab is opening or indexing.
    pub fn is_busy(&self) -> bool {
        self.tabs.iter().any(OpenDocument::is_busy)
    }

    /// The index cache with the current settings, if there is a cache dir.
    pub fn index_cache(&self) -> Option<IndexCache> {
        self.cache_dir
            .clone()
            .map(|dir| IndexCache::new(dir, self.settings.cache_settings()))
    }

    /// Draws the whole app into `ui`. Shared by the real window and UI tests.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.frame_start(&ctx);
        let mut actions = Vec::new();

        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 6)),
            )
            .show(ui, |ui| {
                ui::toolbar::show(ui, &self.tabs, self.active, &mut actions)
            });
        egui::Panel::bottom("status_bar")
            .frame(
                egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show(ui, |ui| {
                ui::status::show(ui, self.active_tab(), self.memory)
            });
        egui::CentralPanel::default().show(ui, |ui| match self.tabs.get(self.active) {
            Some(tab) => ui::body::show(ui, self.active, tab, &mut actions),
            None => ui::empty::show(
                ui,
                &self.settings.recent_files,
                &self.missing_recent,
                &mut actions,
            ),
        });

        let mut open = self.show_settings;
        ui::settings_window::show(
            &ctx,
            &mut open,
            &mut self.settings,
            self.cache_info,
            &mut actions,
        );
        if open != self.show_settings {
            actions.push(Action::ToggleSettings);
        }
        let mut open = self.show_shortcuts;
        ui::shortcuts::show(&ctx, &mut open);
        self.show_shortcuts = open;

        self.keyboard(&ctx, &mut actions);
        self.drag_and_drop(&ctx, &mut actions);
        for action in actions {
            self.apply(&ctx, action);
        }
        if self.is_busy() || self.dialog.is_some() {
            ctx.request_repaint_after(PROGRESS_REPAINT);
        }
    }

    fn frame_start(&mut self, ctx: &egui::Context) {
        if self.repaint.is_none() {
            theme::install(ctx);
            let c = ctx.clone();
            self.repaint = Some(Arc::new(move || c.request_repaint()));
            self.refresh_recent();
        }
        if self.theme_applied != Some(self.settings.theme) {
            theme::apply(ctx, self.settings.theme);
            self.theme_applied = Some(self.settings.theme);
        }

        for path in std::mem::take(&mut self.pending_opens) {
            self.open_now(&path);
        }
        if let Some(rx) = &self.dialog {
            match rx.try_recv() {
                Ok(paths) => {
                    self.dialog = None;
                    for p in paths {
                        self.open_now(&p);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.dialog = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        let gained_focus = focused && !self.focused;
        self.focused = focused;
        let repaint = self.repaint();
        for tab in &mut self.tabs {
            tab.poll();
            tab.maybe_check_changed(gained_focus, repaint.clone());
        }
        // Wake up for the next change check even when nothing else happens.
        if !self.tabs.is_empty() {
            ctx.request_repaint_after(crate::tab::CHANGE_CHECK_EVERY);
        }
        self.poll_background();

        if self.memory_at.is_none_or(|t| t.elapsed() >= MEMORY_EVERY) {
            self.memory = memory_stats::memory_stats().map(|m| m.physical_mem as u64);
            self.memory_at = Some(Instant::now());
            ctx.request_repaint_after(MEMORY_EVERY);
        }
    }

    fn poll_background(&mut self) {
        if let Some(event) = self.cache_job.as_mut().and_then(|j| j.try_recv()) {
            self.cache_job = None;
            self.cache_info = match event {
                JobEvent::Done(bytes) => CacheInfo::Size(bytes),
                _ => CacheInfo::Size(0),
            };
        }
        if let Some(event) = self.recent_job.as_mut().and_then(|j| j.try_recv()) {
            self.recent_job = None;
            if let JobEvent::Done(missing) = event {
                self.missing_recent = missing;
            }
        }
    }

    fn open_env(&self) -> OpenEnv {
        OpenEnv {
            cache: self.index_cache(),
            repaint: self.repaint(),
            hold: self.hold_indexing.clone(),
        }
    }

    fn repaint(&self) -> Repaint {
        self.repaint
            .clone()
            .unwrap_or_else(braceray_core::job::no_repaint)
    }

    fn open_now(&mut self, path: &Path) {
        let key = canonical_key(path);
        if let Some(i) = self.tabs.iter().position(|t| t.key == key) {
            self.active = i;
            return;
        }
        let tab = OpenDocument::open(self.next_id, path, self.open_env());
        self.next_id += 1;
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        // The canonical path, so `./a.json` and `/abs/a.json` share one entry and a
        // relative command-line path still works when the app starts elsewhere.
        self.settings.push_recent(&key);
        self.store.save(&self.settings);
        self.refresh_recent();
    }

    /// Re-checks in the background which recent files are missing.
    fn refresh_recent(&mut self) {
        let paths = self.settings.recent_files.clone();
        self.recent_job = Some(JobHandle::spawn(
            "recent-check",
            0,
            self.repaint(),
            move |_| Ok(paths.into_iter().filter(|p| !p.is_file()).collect()),
        ));
    }

    fn refresh_cache_size(&mut self) {
        let Some(cache) = self.index_cache() else {
            self.cache_info = CacheInfo::Unavailable;
            return;
        };
        self.cache_info = CacheInfo::Busy;
        self.cache_job = Some(JobHandle::spawn(
            "cache-size",
            0,
            self.repaint(),
            move |_| Ok(cache.total_bytes()),
        ));
    }

    fn keyboard(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let open = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
        let close = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
        ctx.input_mut(|i| {
            if i.consume_shortcut(&open) {
                actions.push(Action::OpenDialog);
            }
            if i.consume_shortcut(&close) && !self.tabs.is_empty() {
                actions.push(Action::Close(self.active));
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

    fn drag_and_drop(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
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

    fn apply(&mut self, ctx: &egui::Context, action: Action) {
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

impl eframe::App for BracerayApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn on_exit(&mut self) {
        self.store.flush();
    }
}
