//! UI tests of the app shell (step 06): opening, indexing states, errors, tabs, theme,
//! settings, recent files, drag & drop and the index cache.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use braceray_app::settings::{Settings, SettingsStore, ThemeChoice};
use braceray_app::tab::TabState;
use braceray_app::ui::body::PREVIEW_LABEL;
use braceray_app::{AppConfig, BracerayApp, DROP_HINT};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../braceray-core/tests/fixtures")
        .join(rel)
}

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        Env {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn config(&self) -> AppConfig {
        AppConfig {
            settings_path: Some(self.settings_path()),
            cache_dir: Some(self.cache_dir()),
            hold_indexing: None,
        }
    }

    fn settings_path(&self) -> PathBuf {
        self.dir.path().join("config/settings.json")
    }

    fn cache_dir(&self) -> PathBuf {
        self.dir.path().join("cache")
    }

    /// Writes settings that cache every file, however small.
    fn cache_small_files(&self, enabled: bool) {
        let store = SettingsStore::new(Some(self.settings_path()));
        store.save(&Settings {
            index_cache_enabled: enabled,
            index_cache_min_file_bytes: 0,
            ..Settings::default()
        });
        store.flush();
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

fn harness(app: BracerayApp) -> Harness<'static, BracerayApp> {
    Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, app: &mut BracerayApp| app.show(ui), app)
}

/// Steps the UI until `done` holds (jobs run on real threads), failing after 20 s.
fn step_until(h: &mut Harness<'_, BracerayApp>, what: &str, done: impl Fn(&BracerayApp) -> bool) {
    let start = Instant::now();
    while !done(h.state()) {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
    h.step();
}

fn all_ready(app: &BracerayApp) -> bool {
    !app.tabs.is_empty() && !app.is_busy()
}

#[test]
fn open_shows_progress_then_done_and_cancel_works() {
    let env = Env::new();
    let hold = Arc::new(AtomicBool::new(true));
    let mut config = env.config();
    config.hold_indexing = Some(hold.clone());
    let mut h = harness(BracerayApp::new(config));

    h.state_mut()
        .open_path(fixture("generated/orders_16k.json"));
    step_until(&mut h, "indexing state", |a| {
        a.tabs
            .first()
            .is_some_and(|t| matches!(t.state, TabState::Indexing { .. }))
    });
    assert!(h.query_all_by_label_contains("orders_16k.json").count() >= 1);
    // While indexing, the preview tree shows the first rows.
    step_until(&mut h, "preview", |a| {
        a.tabs[0].tree.as_ref().is_some_and(|t| t.is_preview())
    });
    h.run_steps(2);
    h.get_by_label_contains(PREVIEW_LABEL);
    h.get_by_label_contains("root : [");
    hold.store(false, Ordering::Relaxed);
    step_until(&mut h, "index", |a| {
        all_ready(a) && a.tabs[0].tree.as_ref().is_some_and(|t| !t.is_preview())
    });
    h.run_steps(2);
    assert!(h.query_by_label_contains(PREVIEW_LABEL).is_none());
    h.get_by_label_contains("Index: built");

    // A second file, held in the indexing state, then cancelled.
    hold.store(true, Ordering::Relaxed);
    h.state_mut()
        .open_path(fixture("generated/orders_min_16k.json"));
    step_until(&mut h, "indexing state", |a| {
        a.tabs.len() == 2 && matches!(a.tabs[1].state, TabState::Indexing { .. })
    });
    h.get_by_label("Cancel").click();
    step_until(&mut h, "cancel", |a| {
        matches!(a.tabs[1].state, TabState::Cancelled)
    });
    h.get_by_label("Indexing cancelled.");
    hold.store(false, Ordering::Relaxed);
    h.get_by_label("Retry").click();
    step_until(&mut h, "retry", |a| {
        matches!(a.tabs[1].state, TabState::Ready { .. })
    });
}

#[test]
fn invalid_json_shows_banner_with_position() {
    let env = Env::new();
    let mut h = harness(BracerayApp::new(env.config()));
    h.state_mut()
        .open_path(fixture("invalid/invalid_missing_comma.json"));
    step_until(&mut h, "index", all_ready);
    h.get_by_label_contains("Invalid JSON at line ");
    h.get_by_label_contains(", column ");
    h.get_by_label("Open anyway").click();
    h.run_steps(2);
    assert!(h.state().tabs[0].browse_invalid);
    assert!(h.query_by_label("Open anyway").is_none());
    h.get_by_label("Copy details").click();
    h.run_steps(1);
}

#[test]
fn friendly_messages_for_unreadable_files() {
    let env = Env::new();
    let cases = [
        (
            "encoding/utf16le.json",
            "This file is UTF-16 LE. Braceray supports UTF-8 JSON.",
        ),
        ("encoding/empty.json", "This file is empty."),
        ("encoding/binary.bin", "This file looks like binary data"),
    ];
    let mut h = harness(BracerayApp::new(env.config()));
    for (n, (file, message)) in cases.iter().enumerate() {
        h.state_mut().open_path(fixture(file));
        step_until(&mut h, file, |a| a.tabs.len() == n + 1 && !a.is_busy());
        assert!(matches!(h.state().tabs[n].state, TabState::Failed { .. }));
        assert!(
            h.query_all_by_label_contains(message).count() >= 1,
            "{message}"
        );
    }
    let missing = env.dir.path().join("nope.json");
    h.state_mut().open_path(&missing);
    step_until(&mut h, "missing file", |a| {
        a.tabs.len() == 4 && !a.is_busy()
    });
    h.get_by_label_contains(&format!("Can't open {}", missing.display()));
}

#[test]
fn tabs_open_switch_dedup_and_close() {
    let env = Env::new();
    let a = env.file("a.json", br#"{"a": 1}"#);
    let b = env.file("b.json", b"[1, 2, 3]");
    let mut h = harness(BracerayApp::new(env.config()));
    h.state_mut().open_path(&a);
    h.state_mut().open_path(&b);
    step_until(&mut h, "both", |app| app.tabs.len() == 2 && !app.is_busy());
    assert_eq!(h.state().active, 1);

    // Re-opening a file focuses its tab (also through another spelling of the path).
    let other_spelling = env.dir.path().join(".").join("a.json");
    h.state_mut().open_path(other_spelling);
    h.run_steps(2);
    assert_eq!(h.state().tabs.len(), 2);
    assert_eq!(h.state().active, 0);

    h.get_by_label("b.json").click();
    h.run_steps(2);
    assert_eq!(h.state().active, 1);

    h.get_by_label("Close a.json").click();
    h.run_steps(2);
    assert_eq!(h.state().tabs.len(), 1);
    assert_eq!(h.state().active_tab().unwrap().name, "b.json");

    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::W);
    h.run_steps(2);
    assert!(h.state().tabs.is_empty());
    h.get_by_label(DROP_HINT);
}

#[test]
fn theme_switch_is_instant_and_persisted() {
    let env = Env::new();
    let mut h = harness(BracerayApp::new(env.config()));
    h.get_by_label("Settings").click();
    h.run();
    h.get_by_label("Light").click();
    h.run_steps(2);
    assert_eq!(h.ctx.theme(), egui::Theme::Light);
    assert_eq!(h.state().settings.theme, ThemeChoice::Light);
    h.get_by_label("Dark").click();
    h.run_steps(2);
    assert_eq!(h.ctx.theme(), egui::Theme::Dark);
    h.state().settings_store().flush();

    // A new app (a restart) starts dark.
    let mut h = harness(BracerayApp::new(env.config()));
    h.run_steps(2);
    assert_eq!(h.state().settings.theme, ThemeChoice::Dark);
    assert_eq!(h.ctx.theme(), egui::Theme::Dark);
}

#[test]
fn settings_round_trip_through_the_window() {
    let env = Env::new();
    let mut h = harness(BracerayApp::new(env.config()));
    h.get_by_label("Settings").click();
    h.run();
    h.get_by_label("Cache indexes on disk").click();
    h.run_steps(2);
    assert!(!h.state().settings.index_cache_enabled);
    h.state().settings_store().flush();
    let saved = SettingsStore::new(Some(env.settings_path())).load();
    assert_eq!(saved, h.state().settings);
    assert!(!saved.index_cache_enabled);

    // Broken values in the file are reset to defaults on load.
    std::fs::write(
        env.settings_path(),
        r#"{"theme": "light", "tree_group_size": -4, "index_cache_max_bytes": "lots"}"#,
    )
    .unwrap();
    let app = BracerayApp::new(env.config());
    let d = Settings::default();
    assert_eq!(app.settings.theme, ThemeChoice::Light);
    assert_eq!(app.settings.tree_group_size, d.tree_group_size);
    assert_eq!(app.settings.index_cache_max_bytes, d.index_cache_max_bytes);
}

#[test]
fn reopen_shows_from_cache_and_disabled_cache_writes_nothing() {
    let env = Env::new();
    env.cache_small_files(true);
    let path = fixture("generated/orders_16k.json");
    let mut h = harness(BracerayApp::new(env.config()));
    h.state_mut().open_path(&path);
    step_until(&mut h, "index", all_ready);
    h.get_by_label_contains("Index: built");
    h.state_mut().tabs[0].wait_for_cache_write();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::W);
    h.run_steps(2);
    h.state_mut().open_path(&path);
    step_until(&mut h, "cached index", all_ready);
    h.get_by_label_contains("Index: from cache");

    let env = Env::new();
    env.cache_small_files(false);
    let mut h = harness(BracerayApp::new(env.config()));
    h.state_mut().open_path(&path);
    step_until(&mut h, "index", all_ready);
    h.state_mut().tabs[0].wait_for_cache_write();
    assert!(
        !env.cache_dir().exists() || std::fs::read_dir(env.cache_dir()).unwrap().count() == 0,
        "disabled cache wrote files"
    );
}

#[test]
fn recent_files_open_and_missing_ones_can_be_removed() {
    let env = Env::new();
    let a = env.file("a.json", b"{}");
    let gone = env.file("gone.json", b"[]");
    {
        let mut h = harness(BracerayApp::new(env.config()));
        h.state_mut().open_path(&gone);
        h.state_mut().open_path(&a);
        step_until(&mut h, "both", |app| app.tabs.len() == 2 && !app.is_busy());
        h.state().settings_store().flush();
    }
    std::fs::remove_file(&gone).unwrap();

    let mut h = harness(BracerayApp::new(env.config()));
    h.run_steps(2);
    std::thread::sleep(Duration::from_millis(50)); // background "missing" check
    h.run_steps(2);
    h.get_by_label("(missing)");
    h.get_by_label("Remove").click();
    h.run_steps(2);
    assert_eq!(h.state().settings.recent_files, vec![canonical(&a)]);

    h.get_by_label("a.json").click();
    step_until(&mut h, "open from recent", all_ready);
    assert_eq!(h.state().tabs[0].path, canonical(&a));

    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::W);
    h.run_steps(2);
    h.get_by_label("Clear recent").click();
    h.run_steps(2);
    assert!(h.state().settings.recent_files.is_empty());
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap()
}

#[test]
fn recent_files_store_one_canonical_path_per_file() {
    let env = Env::new();
    let a = env.file("a.json", b"{}");
    std::fs::create_dir(env.dir.path().join("sub")).unwrap();
    let roundabout = env.dir.path().join("sub/../a.json");

    let mut h = harness(BracerayApp::new(env.config()));
    h.state_mut().open_path(&roundabout);
    step_until(&mut h, "open", all_ready);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::W);
    h.run_steps(2);
    h.state_mut().open_path(&a);
    step_until(&mut h, "reopen", all_ready);

    assert_eq!(h.state().settings.recent_files, vec![canonical(&a)]);
    assert!(h.state().settings.recent_files[0].is_absolute());
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

#[test]
fn drag_and_drop_opens_files() {
    let env = Env::new();
    let a = env.file("dropped.json", br#"{"x": [true]}"#);
    let mut h = harness(BracerayApp::new(env.config()));
    h.input_mut()
        .dropped_files
        .push(Arc::new(Dropped(a.clone())));
    h.run_steps(1);
    step_until(&mut h, "dropped file", all_ready);
    assert_eq!(h.state().tabs[0].path, a);
    h.run_steps(2);
    h.get_by_label("root : {1}");
}

#[test]
fn shortcuts_dialog_opens_with_question_mark() {
    let mut h = harness(BracerayApp::default());
    h.event(egui::Event::Text("?".to_owned()));
    h.run();
    assert!(h.state().show_shortcuts);
    assert!(
        h.query_all_by_label("Keyboard shortcuts").count() >= 2,
        "button + window"
    );
    h.get_by_label("Open file");
}
