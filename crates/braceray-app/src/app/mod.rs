use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use braceray_core::cache::IndexCache;
use braceray_core::job::{JobEvent, JobHandle, Repaint};
use eframe::egui;

use crate::settings::{Settings, SettingsStore};
use crate::tab::{OpenDocument, OpenEnv, canonical_key};
use crate::theme;

mod actions;
mod copy;
mod input;
mod stats;
use crate::ui::debug_overlay::{self, FrameStats};
use crate::ui::goto::GoToDialog;
use crate::ui::search::SearchBar;
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
    /// The Go to dialog, when open.
    pub goto: Option<GoToDialog>,
    /// The search bar (shared by every tab).
    pub search: SearchBar,
    cache_info: CacheInfo,
    cache_job: Option<JobHandle<(), u64>>,
    missing_recent: HashSet<PathBuf>,
    recent_job: Option<JobHandle<(), HashSet<PathBuf>>>,
    memory: Option<u64>,
    memory_at: Option<Instant>,
    repaint: Option<Repaint>,
    theme_applied: Option<crate::settings::ThemeChoice>,
    focused: bool,
    /// Frame times for the debug overlay.
    pub frame_stats: FrameStats,
    /// Test hook ("mock clipboard"): every text copied, when enabled.
    copies: Option<Vec<String>>,
    cpu_usage: Option<f32>,
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
            goto: None,
            search: SearchBar::default(),
            cache_info: CacheInfo::Unavailable,
            cache_job: None,
            missing_recent: HashSet::new(),
            recent_job: None,
            memory: None,
            memory_at: None,
            repaint: None,
            theme_applied: None,
            focused: true,
            frame_stats: FrameStats::default(),
            cpu_usage: None,
            copies: None,
        }
    }

    /// Opens `path` (or focuses its tab) on the next frame. Used for the command line,
    /// drops, the dialog, recent files and tests.
    pub fn open_path(&mut self, path: impl Into<PathBuf>) {
        self.pending_opens.push(path.into());
    }

    /// Test hook: from now on, keep every text put on the clipboard (see
    /// [`copies`](Self::copies)). Real clipboards can't be read back in UI tests, and
    /// one test step may run several frames.
    #[doc(hidden)]
    pub fn record_copies(&mut self) {
        self.copies.get_or_insert_with(Vec::new);
    }

    /// Test hook: texts copied since [`record_copies`](Self::record_copies), oldest first;
    /// empties the log.
    #[doc(hidden)]
    pub fn take_copies(&mut self) -> Vec<String> {
        self.copies.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// Puts `text` on the clipboard (through eframe's `arboard` clipboard).
    fn copy_text(&mut self, ctx: &egui::Context, text: String) {
        if let Some(log) = &mut self.copies {
            log.push(text.clone());
        }
        ctx.copy_text(text);
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
        let started = Instant::now();
        let ctx = ui.ctx().clone();
        self.frame_start(&ctx);
        let repaint = self.repaint();
        let mut actions = Vec::new();

        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 6)),
            )
            .show(ui, |ui| {
                ui::toolbar::show(ui, &self.tabs, self.active, &mut actions)
            });
        if !self.tabs.is_empty() {
            let active = self.active;
            egui::Panel::top("search_bar")
                .frame(
                    egui::Frame::side_top_panel(ui.style())
                        .inner_margin(egui::Margin::symmetric(8, 4)),
                )
                .show(ui, |ui| {
                    let search = self.tabs.get(active).map(|t| &t.search);
                    ui::search::show(
                        ui,
                        &mut self.search,
                        search,
                        &self.settings.search_history,
                        &mut actions,
                    )
                });
        }
        self.run_search(&ctx, &repaint);
        self.breadcrumb_bar(ui, &mut actions);
        egui::Panel::bottom("status_bar")
            .frame(
                egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show(ui, |ui| {
                ui::status::show(ui, self.active_tab(), self.memory)
            });
        let active = self.active;
        self.results_panel(ui, &repaint, &mut actions);
        let rows_drawn = egui::CentralPanel::default()
            .show(ui, |ui| match self.tabs.get_mut(active) {
                Some(tab) => ui::body::show(ui, active, tab, &repaint, &mut actions),
                None => {
                    ui::empty::show(
                        ui,
                        &self.settings.recent_files,
                        &self.missing_recent,
                        &mut actions,
                    );
                    0
                }
            })
            .inner;
        self.frame_stats.rows_drawn = rows_drawn;
        let extra = self.debug_lines();
        if debug_overlay::show(&ctx, &mut self.frame_stats, &extra)
            && let Some(tree) = self.tabs.get_mut(active).and_then(|t| t.tree.as_mut())
        {
            let started = Instant::now();
            tree.expand_all_with(0, u32::MAX, u64::MAX, &repaint);
            tracing::info!("expand everything: {:?}", started.elapsed());
        }

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
        self.goto_frame(&ctx);
        self.copy_guard(&ctx, &mut actions);

        self.keyboard(&ctx, &mut actions);
        self.drag_and_drop(&ctx, &mut actions);
        for action in actions {
            self.apply(&ctx, action);
        }
        if self.is_busy() || self.dialog.is_some() {
            ctx.request_repaint_after(PROGRESS_REPAINT);
        }
        let cpu_ms = self.cpu_usage.take().map(|s| s * 1000.0);
        self.frame_stats
            .record(started.elapsed().as_secs_f32() * 1000.0, cpu_ms);
        if self.frame_stats.open {
            // Keep measuring while the overlay is open.
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    /// The full CPU time of the previous frame, from eframe (shown in the debug overlay).
    pub fn set_cpu_usage(&mut self, seconds: Option<f32>) {
        self.cpu_usage = seconds;
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
            tab.poll(&repaint);
            tab.maybe_check_changed(gained_focus, repaint.clone());
        }
        // Wake up for the next change check even when nothing else happens.
        if !self.tabs.is_empty() {
            ctx.request_repaint_after(crate::tab::CHANGE_CHECK_EVERY);
        }
        self.poll_copies(ctx);
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
            group_size: self.settings.tree_group_size,
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

    /// Settles the typed text after the debounce and makes the active tab run the
    /// settled query (a new search cancels the tab's previous one).
    fn run_search(&mut self, ctx: &egui::Context, repaint: &Repaint) {
        if let Some(wait) = self.search.tick() {
            ctx.request_repaint_after(wait);
        }
        let max = usize::try_from(self.settings.max_stored_results).unwrap_or(usize::MAX);
        if let Some(tab) = self.tabs.get_mut(self.active)
            && let Some(doc) = tab.document().cloned()
        {
            let typed = self.search.last_edit;
            tab.search
                .ensure(&doc, &self.search.settled, max, typed, repaint);
        }
    }

    /// The results panel of the active tab (above the status bar).
    fn results_panel(&mut self, ui: &mut egui::Ui, repaint: &Repaint, actions: &mut Vec<Action>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        if tab.search.query().text.is_empty() || !self.search.results_open {
            return;
        }
        let tab_id = tab.id;
        let out = egui::Panel::bottom(("results_panel", tab_id))
            .resizable(true)
            .default_size(220.0)
            .size_range(90.0..=(ui.available_height() * 0.8).max(120.0))
            .frame(
                egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ui, |ui| {
                let cx = ui::results::ResultsContext {
                    tab_id,
                    repaint,
                    focused: tab.panes.focus == crate::panes::Pane::Results,
                };
                ui::results::show(ui, &mut tab.search, tab.tree.as_ref(), cx, actions)
            })
            .inner;
        if out.focus {
            tab.panes.focus = crate::panes::Pane::Results;
        }
        if let Some(i) = out.jump {
            actions.push(Action::SearchJump(i));
        }
    }
}

impl eframe::App for BracerayApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.set_cpu_usage(frame.info().cpu_usage);
        self.show(ui);
    }

    fn on_exit(&mut self) {
        self.store.flush();
    }
}
