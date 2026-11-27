//! UI tests of the search (step 10): the bar, the count, Enter / F3 navigation with
//! wrapping, the results list, toggles, Esc, history, the stored cap and search while
//! indexing.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use braceray_app::search_view::{WRAPPED_BOTTOM, WRAPPED_TOP};
use braceray_app::tree_view::TreeView;
use braceray_app::ui::search::{CASE, CLEAR, FIELD, HISTORY, WORD};
use braceray_app::{AppConfig, BracerayApp};
use eframe::egui::{self, Key, Modifiers, OutputCommand};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

const ORDERS: &str = r#"{
  "orders": [
    {"id": 1, "items": [{"sku": "A-1", "qty": 3}, {"sku": "B-2", "qty": 1}]},
    {"id": 2, "items": []},
    {"id": 3, "note": "Երևան 東京"}
  ],
  "count": 3
}
"#;

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        Env {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn app(&self, hold: Option<Arc<AtomicBool>>) -> BracerayApp {
        BracerayApp::new(AppConfig {
            settings_path: Some(self.dir.path().join("settings.json")),
            cache_dir: None,
            hold_indexing: hold,
        })
    }

    fn open(&self, path: PathBuf) -> Harness<'static, BracerayApp> {
        let mut app = self.app(None);
        app.open_path(path);
        let mut h = harness(app);
        step_until(&mut h, "index and raw view", ready);
        h
    }
}

fn harness(app: BracerayApp) -> Harness<'static, BracerayApp> {
    Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(
            |ui, app: &mut BracerayApp| {
                ui.ctx()
                    .set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
                app.show(ui)
            },
            app,
        )
}

fn ready(a: &BracerayApp) -> bool {
    a.tabs.first().is_some_and(|t| {
        t.tree
            .as_ref()
            .is_some_and(|v| !v.is_preview() && v.jobs_running() == 0)
            && t.panes.raw.as_ref().is_some_and(|r| !r.is_seeking())
    })
}

fn step_until(h: &mut Harness<'_, BracerayApp>, what: &str, done: impl Fn(&BracerayApp) -> bool) {
    let start = Instant::now();
    while !done(h.state()) {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}"
        );
        h.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    h.run_steps(2);
}

fn tree<'h>(h: &'h Harness<'_, BracerayApp>) -> &'h TreeView {
    h.state().tabs[0].tree.as_ref().unwrap()
}

fn selected_label(h: &Harness<'_, BracerayApp>) -> String {
    let t = tree(h);
    let row = t.row(t.selected.unwrap()).unwrap();
    braceray_app::ui::tree_row::row_text(
        t.document().bytes(),
        &row,
        &braceray_app::ui::tree_row::Palette::new(
            true,
            egui::Color32::WHITE,
            egui::Color32::WHITE,
            egui::Color32::GRAY,
        ),
        egui::FontId::monospace(13.0),
        None,
    )
    .1
}

/// Focuses the field (Mod+F), replaces its text and waits for the search to finish.
fn search(h: &mut Harness<'_, BracerayApp>, text: &str) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    h.state_mut().search.text.clear();
    assert!(h.get_by_label(FIELD).is_focused());
    h.get_by_label(FIELD).type_text(text);
    h.step();
    wait_search(h);
}

fn wait_search(h: &mut Harness<'_, BracerayApp>) {
    step_until(h, "search", |a| {
        let s = &a.tabs[0].search;
        a.search.settled == a.search.query() && s.query().text == a.search.text && s.is_done()
    });
}

/// Waits until a jump has selected its node in the tree.
fn wait_jump(h: &mut Harness<'_, BracerayApp>) {
    step_until(h, "jump", |a| {
        let t = &a.tabs[0];
        !t.search.is_stepping()
            && t.search.pending_jump.is_none()
            && !t.panes.is_locating()
            && t.tree.as_ref().is_some_and(|v| v.jobs_running() == 0)
    });
}

fn count_label(h: &Harness<'_, BracerayApp>) -> String {
    h.state().tabs[0].search.count_text()
}

fn current_offset(h: &Harness<'_, BracerayApp>) -> Option<u64> {
    h.state().tabs[0].search.current.map(|c| c.m.offset)
}

fn offsets_of(hay: &str, needle: &str) -> Vec<u64> {
    hay.match_indices(needle).map(|(i, _)| i as u64).collect()
}

#[test]
fn typing_shows_the_count_in_the_bar_and_status_bar() {
    let env = Env::new();
    let mut h = env.open(env.file("o.json", ORDERS.as_bytes()));
    search(&mut h, "sku");
    assert_eq!(count_label(&h), "2 matches");
    // In the bar and in the results header.
    assert_eq!(h.query_all_by_label("2 matches").count(), 2);
    h.get_by_label("Search: 2 matches");
    // The debounce: one more letter doesn't search at once.
    h.get_by_label(FIELD).type_text("x");
    h.step();
    assert_eq!(h.state().tabs[0].search.query().text, "sku");
    wait_search(&mut h);
    assert_eq!(count_label(&h), "No matches");
    assert_eq!(h.query_all_by_label("No matches").count(), 2);
}

#[test]
fn enter_and_f3_move_the_selection_and_wrap() {
    let env = Env::new();
    let mut h = env.open(env.file("o.json", ORDERS.as_bytes()));
    let qty = offsets_of(ORDERS, "qty");
    search(&mut h, "qty");
    h.key_press(Key::Enter);
    wait_jump(&mut h);
    assert_eq!(current_offset(&h), Some(qty[0]));
    assert_eq!(selected_label(&h), "qty : 3");
    assert_eq!(count_label(&h), "match 1 of 2");
    h.key_press(Key::Enter);
    wait_jump(&mut h);
    assert_eq!(selected_label(&h), "qty : 1");
    assert_eq!(count_label(&h), "match 2 of 2");
    // F3 past the last match wraps to the top.
    h.key_press(Key::F3);
    wait_jump(&mut h);
    assert_eq!(current_offset(&h), Some(qty[0]));
    assert_eq!(
        h.state().tabs[0]
            .search
            .toast
            .as_ref()
            .map(|t| t.0.as_str()),
        Some(WRAPPED_TOP)
    );
    h.get_by_label(WRAPPED_TOP);
    // Shift+F3 before the first wraps to the bottom; Shift+Enter goes back too.
    h.key_press_modifiers(Modifiers::SHIFT, Key::F3);
    wait_jump(&mut h);
    assert_eq!(current_offset(&h), Some(qty[1]));
    assert_eq!(
        h.state().tabs[0]
            .search
            .toast
            .as_ref()
            .map(|t| t.0.as_str()),
        Some(WRAPPED_BOTTOM)
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
    wait_jump(&mut h);
    assert_eq!(current_offset(&h), Some(qty[0]));
    // The raw view shows the match.
    let raw = h.state().tabs[0].panes.raw.as_ref().unwrap();
    assert!(raw.top_offset() <= qty[0]);
}

#[test]
fn next_starts_from_the_selected_node() {
    let env = Env::new();
    let mut h = env.open(env.file("o.json", ORDERS.as_bytes()));
    let id = offsets_of(ORDERS, "\"id\"");
    search(&mut h, "id");
    // Select the third order in the tree, then F3: the match after it is its "id".
    h.get_by_label("orders : [3]").click();
    h.run_steps(2);
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    h.get_by_label("[2] : {2}").click();
    h.run_steps(2);
    h.key_press(Key::F3);
    wait_jump(&mut h);
    assert_eq!(current_offset(&h), Some(id[2] + 1));
    assert_eq!(selected_label(&h), "id : 3");
}

#[test]
fn clicking_a_result_selects_its_node() {
    let env = Env::new();
    let mut h = env.open(env.file("o.json", ORDERS.as_bytes()));
    search(&mut h, "sku");
    // Wait for the paths of the rows on screen.
    step_until(&mut h, "row paths", |a| a.tabs[0].search.info.len() == 2);
    h.run_steps(2);
    let row = h
        .query_all_by_role(egui::accesskit::Role::Row)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("#2 $.orders[0].items[1].sku"))
        })
        .expect("result row #2");
    assert!(row.accesskit_node().label().unwrap().ends_with("line 3"));
    row.click();
    h.run_steps(1);
    wait_jump(&mut h);
    assert_eq!(selected_label(&h), "sku : \"B-2\"");
    assert_eq!(count_label(&h), "match 2 of 2");
    // ↑ in the list, then Enter jumps to row #1.
    h.key_press(Key::ArrowUp);
    h.run_steps(1);
    assert_eq!(h.state().tabs[0].search.list_selected, Some(0));
    h.key_press(Key::Enter);
    wait_jump(&mut h);
    assert_eq!(selected_label(&h), "sku : \"A-1\"");
}

#[test]
fn toggles_rerun_the_search_and_typing_replaces_it() {
    let env = Env::new();
    let mut h = env.open(env.file("o.json", ORDERS.as_bytes()));
    search(&mut h, "ID");
    assert_eq!(count_label(&h), "3 matches");
    let g = h.state().tabs[0].search.generation();
    h.get_by_label(CASE).click();
    h.run_steps(1);
    // Re-run right away (no debounce) and case-sensitive now.
    assert!(h.state().tabs[0].search.generation() > g);
    wait_search(&mut h);
    assert_eq!(count_label(&h), "No matches");
    h.get_by_label(CASE).click();
    wait_search(&mut h);
    assert_eq!(count_label(&h), "3 matches");
    // Whole word: "sk" isn't a word in "sku".
    search(&mut h, "sk");
    assert_eq!(count_label(&h), "2 matches");
    h.get_by_label(WORD).click();
    wait_search(&mut h);
    assert_eq!(count_label(&h), "No matches");
}

#[test]
fn esc_clears_then_closes_and_mod_shift_f_toggles_results() {
    let env = Env::new();
    let mut h = env.open(env.file("o.json", ORDERS.as_bytes()));
    search(&mut h, "sku");
    assert!(h.state().search.results_open);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(h.state().search.text, "");
    assert_eq!(h.state().tabs[0].search.query().text, "");
    assert_eq!(count_label(&h), "");
    // The field kept the focus; a second Esc closes the results panel.
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state().search.results_open);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
    h.run_steps(1);
    assert!(h.state().search.results_open);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
    h.run_steps(1);
    assert!(!h.state().search.results_open);
    // The clear button.
    search(&mut h, "qty");
    h.get_by_label(CLEAR).click();
    h.run_steps(2);
    assert_eq!(h.state().tabs[0].search.query().text, "");
}

#[test]
fn history_keeps_searches_used_to_navigate() {
    let env = Env::new();
    let mut h = env.open(env.file("o.json", ORDERS.as_bytes()));
    search(&mut h, "sku");
    h.key_press(Key::Enter);
    wait_jump(&mut h);
    search(&mut h, "qty");
    h.key_press(Key::Enter);
    wait_jump(&mut h);
    assert_eq!(h.state().settings.search_history, ["qty", "sku"]);
    // Pick "sku" from the dropdown: it searches at once.
    h.get_by_label(HISTORY).click();
    h.run_steps(2);
    h.get_by_label("sku").click();
    h.run_steps(1);
    assert_eq!(h.state().search.text, "sku");
    wait_search(&mut h);
    assert_eq!(count_label(&h), "2 matches");
    // Persisted.
    h.state().settings_store().flush();
    let saved = std::fs::read_to_string(env.dir.path().join("settings.json")).unwrap();
    assert!(saved.contains("\"qty\""), "{saved}");
}

#[test]
fn next_and_previous_beyond_the_stored_cap() {
    let env = Env::new();
    // 1,500 matches, 1,000 stored.
    let mut doc = String::from("[");
    for i in 0..1500 {
        doc.push_str(&format!("{{\"k\": \"hit {i}\"}}, "));
    }
    doc.push_str("0]");
    let hits = offsets_of(&doc, "hit");
    let mut app = env.app(None);
    app.settings.max_stored_results = 1000;
    app.open_path(env.file("cap.json", doc.as_bytes()));
    let mut h = harness(app);
    step_until(&mut h, "index", ready);
    search(&mut h, "hit");
    assert_eq!(h.state().tabs[0].search.count(), 1500);
    assert_eq!(h.state().tabs[0].search.stored(), 1000);
    h.get_by_label("· showing first 1,000");
    // Jump to the last stored match, then F3 twice: beyond the cap (on a worker).
    let repaint = braceray_core::job::no_repaint();
    h.state_mut().tabs[0].search_jump_index(999, &repaint);
    wait_jump(&mut h);
    assert_eq!(current_offset(&h), Some(hits[999]));
    for k in [1000, 1001] {
        h.key_press(Key::F3);
        h.run_steps(1);
        wait_jump(&mut h);
        assert_eq!(current_offset(&h), Some(hits[k]));
        assert_eq!(selected_label(&h), format!("k : \"hit {k}\""));
    }
    assert_eq!(count_label(&h), "match > 1,000 of 1,500");
    // Shift+F3 from the top wraps to the very last match (beyond the cap).
    h.state_mut().tabs[0].search_jump_index(0, &repaint);
    wait_jump(&mut h);
    h.key_press_modifiers(Modifiers::SHIFT, Key::F3);
    h.run_steps(1);
    wait_jump(&mut h);
    assert_eq!(current_offset(&h), Some(hits[1499]));
    assert_eq!(selected_label(&h), "k : \"hit 1499\"");
}

#[test]
fn search_runs_while_indexing_and_paths_fill_in_later() {
    let env = Env::new();
    let hold = Arc::new(AtomicBool::new(true));
    let mut app = env.app(Some(hold.clone()));
    app.open_path(env.file("o.json", ORDERS.as_bytes()));
    let mut h = harness(app);
    step_until(&mut h, "preview", |a| {
        a.tabs[0].tree.as_ref().is_some_and(TreeView::is_preview)
    });
    search(&mut h, "sku");
    assert_eq!(count_label(&h), "2 matches");
    // No paths over the preview; rows show placeholders.
    h.run_steps(3);
    assert!(h.state().tabs[0].search.info.is_empty());
    h.query_all_by_role(egui::accesskit::Role::Row)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("#1 …"))
        })
        .expect("placeholder row");
    // A jump inside the preview works now.
    h.key_press(Key::Enter);
    wait_jump(&mut h);
    assert_eq!(selected_label(&h), "sku : \"A-1\"");
    hold.store(false, Ordering::Relaxed);
    step_until(&mut h, "index", ready);
    step_until(&mut h, "row paths", |a| a.tabs[0].search.info.len() == 2);
    h.run_steps(2);
    h.query_all_by_role(egui::accesskit::Role::Row)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("#1 $.orders[0].items[0].sku"))
        })
        .expect("row with its path");
}

#[test]
fn copy_all_paths_to_the_clipboard() {
    let env = Env::new();
    let mut h = env.open(env.file("o.json", ORDERS.as_bytes()));
    search(&mut h, "qty");
    h.get_by_label(braceray_app::ui::results::COPY_ALL).click();
    let start = Instant::now();
    let mut copied = Vec::new();
    while copied.is_empty() {
        assert!(start.elapsed() < Duration::from_secs(20));
        h.step();
        copied.extend(
            h.output()
                .platform_output
                .commands
                .iter()
                .filter_map(|c| match c {
                    OutputCommand::CopyText(t) => Some(t.clone()),
                    _ => None,
                }),
        );
    }
    assert_eq!(
        copied,
        ["$.orders[0].items[0].qty\n$.orders[0].items[1].qty\n"]
    );
}

/// The modifiers egui-winit reports for Ctrl on Windows / Linux and Cmd on macOS.
fn platform_mod(os: egui::os::OperatingSystem) -> Modifiers {
    if os == egui::os::OperatingSystem::Mac {
        Modifiers::MAC_CMD | Modifiers::COMMAND
    } else {
        Modifiers::CTRL | Modifiers::COMMAND
    }
}

fn shortcuts_on(os: egui::os::OperatingSystem) {
    let env = Env::new();
    let mut h = env.open(env.file("o.json", ORDERS.as_bytes()));
    h.ctx.set_os(os);
    let m = platform_mod(os);
    let qty = offsets_of(ORDERS, "qty");
    // Mod+F focuses the field.
    h.key_press_modifiers(m, Key::F);
    h.run_steps(2);
    assert!(h.get_by_label(FIELD).is_focused(), "{os:?}");
    h.get_by_label(FIELD).type_text("qty");
    h.step();
    wait_search(&mut h);
    // Mod+Shift+F toggles the results panel.
    h.key_press_modifiers(m | Modifiers::SHIFT, Key::F);
    h.run_steps(1);
    assert!(!h.state().search.results_open, "{os:?}");
    h.key_press_modifiers(m | Modifiers::SHIFT, Key::F);
    h.run_steps(1);
    assert!(h.state().search.results_open, "{os:?}");
    // F3 / Shift+F3 everywhere.
    h.key_press(Key::F3);
    wait_jump(&mut h);
    assert_eq!(current_offset(&h), Some(qty[0]), "{os:?}");
    h.key_press(Key::F3);
    wait_jump(&mut h);
    assert_eq!(current_offset(&h), Some(qty[1]), "{os:?}");
    h.key_press_modifiers(Modifiers::SHIFT, Key::F3);
    wait_jump(&mut h);
    assert_eq!(current_offset(&h), Some(qty[0]), "{os:?}");
    // Cmd+G / Cmd+Shift+G on macOS only (Ctrl+G is not bound elsewhere).
    h.key_press_modifiers(m, Key::G);
    h.run_steps(1);
    wait_jump(&mut h);
    let mac = os == egui::os::OperatingSystem::Mac;
    assert_eq!(
        current_offset(&h),
        Some(if mac { qty[1] } else { qty[0] }),
        "{os:?}"
    );
    h.key_press_modifiers(m | Modifiers::SHIFT, Key::G);
    h.run_steps(1);
    wait_jump(&mut h);
    assert_eq!(current_offset(&h), Some(qty[0]), "{os:?}");
}

#[test]
fn shortcuts_on_windows_use_ctrl() {
    shortcuts_on(egui::os::OperatingSystem::Windows);
}

#[test]
fn shortcuts_on_macos_use_cmd_and_cmd_g() {
    shortcuts_on(egui::os::OperatingSystem::Mac);
}
