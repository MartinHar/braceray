//! UI tests of the tree view (step 07): expand / collapse by click and keyboard, group
//! rows, selection, context menu, deep documents and the preview → full index switch.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use braceray_app::tab::TabState;
use braceray_app::tree_view::{TreeKey, TreeView};
use braceray_app::{AppConfig, BracerayApp};
use eframe::egui::{self, Key, Modifiers, OutputCommand, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

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
            settings_path: Some(self.dir.path().join("settings.json")),
            cache_dir: None,
            hold_indexing: None,
        }
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

fn harness(app: BracerayApp) -> Harness<'static, BracerayApp> {
    Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        // Real frame pacing, so two clicks can be a double click (0.3 s window).
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, app: &mut BracerayApp| app.show(ui), app)
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

/// Opens `path` and waits until the full-index tree shows its rows.
fn open(env: &Env, path: PathBuf) -> Harness<'static, BracerayApp> {
    let mut h = harness(BracerayApp::new(env.config()));
    h.state_mut().open_path(path);
    step_until(&mut h, "tree", |a| {
        a.tabs.first().is_some_and(|t| {
            matches!(t.state, TabState::Ready { .. })
                && t.tree
                    .as_ref()
                    .is_some_and(|v| !v.is_preview() && v.jobs_running() == 0)
        })
    });
    h
}

fn tree<'h>(h: &'h Harness<'_, BracerayApp>) -> &'h TreeView {
    h.state().tabs[0].tree.as_ref().unwrap()
}

fn selected_label(h: &Harness<'_, BracerayApp>) -> String {
    let t = tree(h);
    let row = t.row(t.selected.unwrap()).unwrap();
    let doc = t.document().clone();
    braceray_app::ui::tree_row::row_text(
        doc.bytes(),
        &row,
        &palette(),
        egui::FontId::monospace(13.0),
        None,
    )
    .1
}

fn palette() -> braceray_app::ui::tree_row::Palette {
    braceray_app::ui::tree_row::Palette::new(
        true,
        egui::Color32::WHITE,
        egui::Color32::WHITE,
        egui::Color32::GRAY,
    )
}

fn key(h: &mut Harness<'_, BracerayApp>, key: Key) {
    h.key_press(key);
    h.run_steps(2);
}

/// Clicks at `pos` (press + release in one frame each).
fn click_at(h: &mut Harness<'_, BracerayApp>, pos: egui::Pos2) {
    h.event(egui::Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(2);
}

/// The disclosure triangle of the row labelled `label` at `depth`.
fn arrow_of(h: &Harness<'_, BracerayApp>, label: &str, depth: f32) -> egui::Pos2 {
    let r = h.get_by_label(label).rect();
    egui::pos2(r.left() + 12.0 + depth * 16.0, r.center().y)
}

fn copied(h: &Harness<'_, BracerayApp>) -> Vec<String> {
    h.output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn expand_and_collapse_by_click_and_keyboard() {
    let env = Env::new();
    let path = env.file("t.json", br#"{"a": {"b": [1, 2]}, "c": "x", "d": []}"#);
    let mut h = open(&env, path);
    // The root opens expanded and selected.
    h.get_by_label("root : {3}");
    h.get_by_label("a : {1}");
    h.get_by_label("c : \"x\"");
    h.get_by_label("d : []");
    assert!(h.query_by_label("b : [2]").is_none());

    // Click on the triangle expands; a click elsewhere only selects.
    let arrow = arrow_of(&h, "a : {1}", 1.0);
    click_at(&mut h, arrow);
    h.get_by_label("b : [2]");
    h.get_by_label("c : \"x\"").click();
    h.run_steps(2);
    assert_eq!(selected_label(&h), "c : \"x\"");
    assert!(h.query_by_label("[0] : 1").is_none());

    // Double click toggles (after the double-click window of the previous clicks).
    h.run_steps(30);
    let b = h.get_by_label("b : [2]").rect().center();
    click_at(&mut h, b);
    click_at(&mut h, b);
    h.get_by_label("[0] : 1");
    h.get_by_label("[1] : 2");

    // Keyboard: ↑/↓ move, ← collapses then goes to the parent, → expands then enters.
    key(&mut h, Key::ArrowDown);
    assert_eq!(selected_label(&h), "[0] : 1");
    key(&mut h, Key::ArrowLeft); // a scalar: go to the parent
    assert_eq!(selected_label(&h), "b : [2]");
    key(&mut h, Key::ArrowLeft); // expanded: collapse
    assert!(h.query_by_label("[0] : 1").is_none());
    assert_eq!(selected_label(&h), "b : [2]");
    key(&mut h, Key::ArrowRight); // expand
    h.get_by_label("[0] : 1");
    key(&mut h, Key::ArrowRight); // enter the first child
    assert_eq!(selected_label(&h), "[0] : 1");
    key(&mut h, Key::Home);
    assert_eq!(selected_label(&h), "root : {3}");
    key(&mut h, Key::End);
    assert_eq!(selected_label(&h), "d : []");
    key(&mut h, Key::PageUp);
    assert_eq!(selected_label(&h), "root : {3}");
    key(&mut h, Key::PageDown);
    assert_eq!(selected_label(&h), "d : []");
    // Enter toggles; on the root it collapses everything below.
    key(&mut h, Key::Home);
    key(&mut h, Key::Enter);
    assert_eq!(tree(&h).model.row_count(), 1);
    assert!(h.query_by_label("a : {1}").is_none());
    // `*` expands the selection and its children.
    h.event(egui::Event::Text("*".to_owned()));
    h.run_steps(2);
    h.get_by_label("a : {1}");
    h.get_by_label("b : [2]");
    assert!(h.query_by_label("[0] : 1").is_none());
    // The status bar shows the selected value's position.
    key(&mut h, Key::ArrowDown);
    h.get_by_label_contains("Ln 1, Col 7");
}

#[test]
fn group_rows_for_more_than_1000_children() {
    let env = Env::new();
    let items: Vec<String> = (0..2500).map(|i| i.to_string()).collect();
    let path = env.file("big.json", format!("[{}]", items.join(",")).as_bytes());
    let mut h = open(&env, path);
    h.get_by_label("root : [2,500]");
    h.get_by_label("[0 … 999]  1,000 items");
    h.get_by_label("[1,000 … 1,999]  1,000 items");
    h.get_by_label("[2,000 … 2,499]  500 items");
    assert!(h.query_by_label("[0] : 0").is_none());

    // Into the last group with the keyboard; the rows of a group are its children.
    key(&mut h, Key::End);
    key(&mut h, Key::ArrowRight);
    step_until(&mut h, "group children", |a| {
        a.tabs[0].tree.as_ref().unwrap().jobs_running() == 0
    });
    key(&mut h, Key::ArrowRight);
    assert_eq!(selected_label(&h), "[2,000] : 2000");
    key(&mut h, Key::End);
    assert_eq!(selected_label(&h), "[2,499] : 2499");
    h.get_by_label("[2,499] : 2499");
    assert_eq!(tree(&h).model.row_count(), 4 + 500);
    // ← twice: to the group, then collapse it.
    key(&mut h, Key::ArrowLeft);
    assert_eq!(selected_label(&h), "[2,000 … 2,499]  500 items");
    key(&mut h, Key::ArrowLeft);
    assert_eq!(tree(&h).model.row_count(), 4);
}

#[test]
fn context_menu_copies_and_expands() {
    let env = Env::new();
    let path = env.file(
        "m.json",
        br#"{"name": "Ann", "tags": ["x", {"deep": [true]}]}"#,
    );
    let mut h = open(&env, path);

    h.get_by_label("name : \"Ann\"").click_secondary();
    h.run_steps(2);
    h.get_by_label("Copy key").click();
    h.step();
    assert_eq!(copied(&h), ["name"]);
    h.run_steps(2);

    h.get_by_label("name : \"Ann\"").click_secondary();
    h.run_steps(2);
    h.get_by_label("Copy value").click();
    h.step();
    assert_eq!(copied(&h), ["\"Ann\""]);
    h.run_steps(2);

    // The hooks for later steps are there but disabled.
    h.get_by_label("tags : [2]").click_secondary();
    h.run_steps(2);
    assert!(h.get_by_label("Copy path").accesskit_node().is_disabled());
    assert!(
        h.get_by_label("Show in raw view")
            .accesskit_node()
            .is_disabled()
    );
    h.get_by_label("Expand all below").click();
    h.run_steps(2);
    h.get_by_label("[0] : \"x\"");
    h.get_by_label("deep : [1]");
    h.get_by_label("[0] : true");
    assert_eq!(tree(&h).model.row_count(), 7);

    h.get_by_label("tags : [2]").click_secondary();
    h.run_steps(2);
    h.get_by_label("Collapse").click();
    h.run_steps(2);
    assert_eq!(tree(&h).model.row_count(), 3);
}

#[test]
fn deep_100k_by_keyboard() {
    let env = Env::new();
    let depth = 100_000;
    let mut bytes = vec![b'['; depth];
    bytes.extend(std::iter::repeat_n(b']', depth));
    let path = env.file("deep.json", &bytes);
    let mut h = open(&env, path);
    // "→" 2 × 100k times: expand, then step into the child (the key handler the UI uses).
    let repaint = braceray_core::job::no_repaint();
    {
        let tab = &mut h.state_mut().tabs[0];
        let tree = tab.tree.as_mut().unwrap();
        for _ in 0..2 * depth {
            tree.key(TreeKey::Right, 10, &repaint);
        }
    }
    h.run_steps(2);
    let t = tree(&h);
    assert_eq!(t.model.row_count(), depth as u64);
    assert_eq!(t.selected, Some(depth as u64 - 1));
    assert_eq!(t.model.depth(depth as u64 - 1), Some(depth as u32 - 1));
    // The deepest rows are drawn (with a capped indent).
    h.get_by_label("[0] : []");
    // Real key presses at the bottom: ← goes to the parent; Home and ← collapse it all.
    key(&mut h, Key::ArrowLeft);
    assert_eq!(tree(&h).selected, Some(depth as u64 - 2));
    key(&mut h, Key::Home);
    key(&mut h, Key::ArrowLeft);
    assert_eq!(tree(&h).model.row_count(), 1);
}

#[test]
fn preview_switches_to_the_full_index_keeping_state() {
    let env = Env::new();
    let path = env.file("p.json", br#"{"a": [1, 2, {"b": "c"}], "z": null}"#);
    let hold = Arc::new(AtomicBool::new(true));
    let mut config = env.config();
    config.hold_indexing = Some(hold.clone());
    let mut h = harness(BracerayApp::new(config));
    h.state_mut().open_path(path);
    step_until(&mut h, "preview", |a| {
        a.tabs
            .first()
            .and_then(|t| t.tree.as_ref())
            .is_some_and(|t| t.is_preview())
    });
    h.get_by_label_contains("Preview: indexing…");
    // Expand a → [2] in the preview and select b.
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::ArrowRight);
    key(&mut h, Key::End);
    key(&mut h, Key::ArrowUp);
    key(&mut h, Key::ArrowRight);
    key(&mut h, Key::ArrowRight);
    assert_eq!(selected_label(&h), "b : \"c\"");
    let rows_before = tree(&h).model.row_count();

    hold.store(false, Ordering::Relaxed);
    step_until(&mut h, "full index", |a| {
        a.tabs[0].tree.as_ref().is_some_and(|t| !t.is_preview())
    });
    assert!(matches!(h.state().tabs[0].state, TabState::Ready { .. }));
    assert!(h.query_by_label_contains("Preview: indexing…").is_none());
    assert_eq!(tree(&h).model.row_count(), rows_before);
    assert_eq!(selected_label(&h), "b : \"c\"");
    h.get_by_label("b : \"c\"");
}

#[test]
fn group_size_setting_regroups_and_keeps_expansions() {
    let env = Env::new();
    let items: Vec<String> = (0..30).map(|i| format!("[{i}]")).collect();
    let path = env.file("g.json", format!("[{}]", items.join(",")).as_bytes());
    let mut h = open(&env, path);
    // Expand the last element, [29], with the keyboard.
    key(&mut h, Key::End);
    h.get_by_label("[29] : [1]");
    key(&mut h, Key::ArrowRight);
    h.get_by_label("[0] : 29");
    h.state_mut().settings.tree_group_size = 10;
    h.state_mut().tabs[0].set_group_size(10, &braceray_core::job::no_repaint());
    h.run_steps(2);
    h.get_by_label("[0 … 9]  10 items");
    h.get_by_label("[20 … 29]  10 items");
    // [29] stays expanded (inside its group) and selected.
    h.get_by_label("[0] : 29");
    assert_eq!(selected_label(&h), "[29] : [1]");
}
