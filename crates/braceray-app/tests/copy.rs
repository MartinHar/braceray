//! UI tests of step 11: the breadcrumb bar (click selects an ancestor, `…` shows every
//! level, sibling dropdown, 1M-child array), copy path in each format (button, menu,
//! Ctrl/Cmd+Shift+C, last format remembered), copy value / formatted / minified / key /
//! string contents, Ctrl/Cmd+C in the tree, and the size guard.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use braceray_app::tab::TabState;
use braceray_app::tree_view::TreeView;
use braceray_app::ui::breadcrumb::{FORMAT_MENU, SHOW_ALL, copy_label};
use braceray_app::ui::copy_guard::{CANCEL, COPY_ANYWAY, message};
use braceray_app::{AppConfig, BracerayApp};
use braceray_core::path::PathFormat;
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;

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

fn harness(app: BracerayApp, width: f32) -> Harness<'static, BracerayApp> {
    Harness::builder()
        .with_size(egui::vec2(width, 700.0))
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

fn open_with(env: &Env, path: PathBuf, width: f32) -> Harness<'static, BracerayApp> {
    let mut h = harness(BracerayApp::new(env.config()), width);
    h.state_mut().record_copies();
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

fn open(env: &Env, path: PathBuf) -> Harness<'static, BracerayApp> {
    open_with(env, path, 1200.0)
}

fn tree<'h>(h: &'h Harness<'_, BracerayApp>) -> &'h TreeView {
    h.state().tabs[0].tree.as_ref().unwrap()
}

/// Texts put on the (mock) clipboard since the last call.
fn copied(h: &mut Harness<'_, BracerayApp>) -> Vec<String> {
    h.state_mut().take_copies()
}

/// Steps until something is copied; returns it.
fn wait_copied(h: &mut Harness<'_, BracerayApp>) -> String {
    let start = Instant::now();
    loop {
        if let Some(t) = copied(h).into_iter().next() {
            return t;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "nothing copied");
        h.step();
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Selects the value at `address` through the tree and waits for it.
fn reveal(h: &mut Harness<'_, BracerayApp>, address: Vec<u64>) {
    let repaint = braceray_core::job::no_repaint();
    let want = address.clone();
    h.state_mut().tabs[0]
        .tree
        .as_mut()
        .unwrap()
        .reveal_address(address, &repaint);
    step_until(h, "reveal", |a| {
        let t = a.tabs[0].tree.as_ref().unwrap();
        t.selected.and_then(|r| t.model.address(r)) == Some(want.clone())
    });
}

fn selected_address(h: &Harness<'_, BracerayApp>) -> Vec<u64> {
    let t = tree(h);
    t.model.address(t.selected.unwrap()).unwrap()
}

const DOC: &[u8] = br#"{"orders": [{"id": 1}, {"id": 2, "customer": {"email": "a@b.c", "full name": "A \"B\"", "class": [1, 2], "te\u0073t": "x\ny"}}], "n": 3}"#;

#[test]
fn breadcrumb_shows_the_chain_and_click_selects_the_ancestor() {
    let env = Env::new();
    let mut h = open(&env, env.file("d.json", DOC));
    reveal(&mut h, vec![0, 1, 1, 0]); // $.orders[1].customer.email
    for label in ["$", "orders", "[1]", "customer", "email"] {
        h.get_by_label(label);
    }
    h.get_by_label("customer").click();
    h.run_steps(2);
    assert_eq!(selected_address(&h), [0, 1, 1]);
    // The chain still shows the ancestors up to the new selection.
    h.get_by_label("[1]").click();
    h.run_steps(2);
    assert_eq!(selected_address(&h), [0, 1]);
    h.get_by_label("$").click();
    h.run_steps(2);
    assert_eq!(selected_address(&h), Vec::<u64>::new());
}

#[test]
fn copy_path_button_menu_and_shortcut_remember_the_format() {
    let env = Env::new();
    let mut h = open(&env, env.file("d.json", DOC));
    reveal(&mut h, vec![0, 1, 1, 1]); // "full name"
    h.get_by_label(&copy_label(PathFormat::JsonPath)).click();
    h.step();
    assert_eq!(copied(&mut h), [r#"$.orders[1].customer['full name']"#]);
    h.run_steps(2);
    h.get_by_label("Copied JSONPath");

    // The menu copies in another format and makes it the default.
    h.get_by_label(FORMAT_MENU).click();
    h.run_steps(2);
    h.get_by_label(&format!("    {}", copy_label(PathFormat::Jq)))
        .click();
    h.step();
    assert_eq!(copied(&mut h), [r#".orders[1].customer["full name"]"#]);
    h.run_steps(2);
    assert_eq!(h.state().settings.path_format, PathFormat::Jq);
    h.get_by_label(&copy_label(PathFormat::Jq));

    // Ctrl/Cmd+Shift+C uses the last format.
    reveal(&mut h, vec![0, 1, 1, 2, 1]); // class[1]
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::C);
    h.step();
    assert_eq!(copied(&mut h), [".orders[1].customer.class[1]"]);
    h.run_steps(2);

    // JavaScript with a custom root and JSON Pointer.
    h.state_mut().settings.js_root = "doc".to_owned();
    h.get_by_label(FORMAT_MENU).click();
    h.run_steps(2);
    h.get_by_label(&format!("    {}", copy_label(PathFormat::JavaScript)))
        .click();
    h.step();
    assert_eq!(copied(&mut h), [r#"doc.orders[1].customer["class"][1]"#]);
    h.run_steps(2);
    h.get_by_label(FORMAT_MENU).click();
    h.run_steps(2);
    h.get_by_label(&format!("    {}", copy_label(PathFormat::Pointer)))
        .click();
    h.step();
    assert_eq!(copied(&mut h), ["/orders/1/customer/class/1"]);
    h.run_steps(2);

    // The choice is saved.
    h.state().settings_store().flush();
    let saved = std::fs::read_to_string(env.dir.path().join("settings.json")).unwrap();
    assert!(saved.contains(r#""path_format": "pointer""#), "{saved}");
}

#[test]
fn copy_value_styles_key_and_string_contents() {
    let env = Env::new();
    let mut h = open(&env, env.file("d.json", DOC));
    let menu = |h: &mut Harness<'_, BracerayApp>, row: &str, item: &str| {
        h.get_by_label(row).click_secondary();
        h.run_steps(2);
        h.get_by_label(item).click();
        h.step();
        let c = copied(h);
        h.run_steps(2);
        c
    };
    reveal(&mut h, vec![0, 1, 1]); // customer
    let row = "customer : {4}";
    assert_eq!(
        menu(&mut h, row, "Copy minified"),
        [r#"{"email":"a@b.c","full name":"A \"B\"","class":[1,2],"te\u0073t":"x\ny"}"#]
    );
    let pretty = menu(&mut h, row, "Copy formatted");
    let want: serde_json::Value = serde_json::from_slice(DOC).unwrap();
    let got: serde_json::Value = serde_json::from_str(&pretty[0]).unwrap();
    assert_eq!(got, want["orders"][1]["customer"]);
    // Source order and escapes are kept (only whitespace moves).
    let lines = [
        "{",
        r#"  "email": "a@b.c","#,
        r#"  "full name": "A \"B\"","#,
        r#"  "class": ["#,
        "    1,",
        "    2",
        "  ],",
        r#"  "te\u0073t": "x\ny""#,
        "}",
    ];
    assert_eq!(pretty[0], lines.join("\n"));
    assert_eq!(menu(&mut h, row, "Copy key"), ["customer"]);

    // Keys are decoded; strings can be copied with or without quotes and escapes.
    h.get_by_label("customer : {4}").click();
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    reveal(&mut h, vec![0, 1, 1, 3]);
    let row = r#"te\u0073t : "x\ny""#;
    assert_eq!(menu(&mut h, row, "Copy key"), ["test"]);
    h.get_by_label("Copied key");
    assert_eq!(menu(&mut h, row, "Copy value"), [r#""x\ny""#]);
    assert_eq!(menu(&mut h, row, "Copy string contents"), ["x\ny"]);
    h.get_by_label("Copied 3 B");

    // Ctrl/Cmd+C with the tree focused copies the value.
    reveal(&mut h, vec![0, 1, 1, 2]);
    h.event(egui::Event::Copy);
    h.step();
    assert_eq!(copied(&mut h), ["[1, 2]"]);
    h.run_steps(2);
}

#[test]
fn size_guard_asks_above_the_limit() {
    let env = Env::new();
    // A 3 MB string: bigger than the sync limit too, so "Copy anyway" runs a job.
    let mut bytes = br#"{"small": [1], "big": ""#.to_vec();
    bytes.extend(std::iter::repeat_n(b'z', 3 << 20));
    bytes.extend_from_slice(br#""}"#);
    let mut h = open(&env, env.file("big.json", &bytes));
    h.state_mut().settings.copy_limit_bytes = 1 << 20;

    // Under the limit: no question.
    reveal(&mut h, vec![0]);
    h.event(egui::Event::Copy);
    h.step();
    assert_eq!(copied(&mut h), ["[1]"]);
    h.run_steps(2);

    reveal(&mut h, vec![1]);
    h.event(egui::Event::Copy);
    h.run_steps(2);
    let size = (3 << 20) + 2;
    h.run_steps(2); // the dialog area sizes itself on its first frame
    h.get_by_label(&message(size));
    assert!(copied(&mut h).is_empty());
    h.get_by_label(CANCEL).click();
    h.run_steps(2);
    assert!(h.query_by_label(&message(size)).is_none());
    assert!(h.state().tabs[0].copy.guard.is_none());

    h.event(egui::Event::Copy);
    h.run_steps(4);
    h.get_by_label(COPY_ANYWAY).click();
    let text = wait_copied(&mut h);
    assert_eq!(text.len(), size as usize);
    assert!(text.starts_with("\"zzz") && text.ends_with("zz\""));
    h.run_steps(2);
    h.get_by_label(&format!("Copied {}", braceray_app::format::bytes(size)));
}

#[test]
fn long_paths_collapse_in_the_middle_and_expand() {
    let env = Env::new();
    // 40 levels of {"level_NN": …}.
    let depth = 40;
    let mut text = String::new();
    for i in 0..depth {
        text.push_str(&format!("{{\"level_{i:02}\": "));
    }
    text.push('1');
    text.push_str(&"}".repeat(depth));
    let mut h = open_with(&env, env.file("deep.json", text.as_bytes()), 700.0);
    reveal(&mut h, vec![0; depth]);
    h.get_by_label("$");
    h.get_by_label("level_39");
    assert!(h.query_by_label("level_05").is_none(), "middle collapsed");
    h.get_by_label(SHOW_ALL).click();
    h.run_steps(2);
    for i in 0..depth {
        h.get_by_label(&format!("level_{i:02}"));
    }
    // A new selection collapses again (level_05 is on the first lines of the
    // scrolling area).
    h.get_by_label("level_05").click();
    h.run_steps(3);
    assert_eq!(selected_address(&h), vec![0; 6]);
    assert!(h.query_by_label(SHOW_ALL).is_some());
}

#[test]
fn million_child_array_navigation_and_siblings() {
    let env = Env::new();
    let n = 1_000_000;
    let mut text = String::from("{\"items\": [");
    for i in 0..n {
        if i > 0 {
            text.push(',');
        }
        text.push_str(&i.to_string());
    }
    text.push_str("]}");
    let mut h = open(&env, env.file("million.json", text.as_bytes()));
    reveal(&mut h, vec![0, 523_456]);
    step_until(&mut h, "breadcrumb label", |a| {
        let t = a.tabs[0].tree.as_ref().unwrap();
        t.crumb_rows().iter().all(|&r| t.value_row(r).is_some())
    });
    h.get_by_label("items");
    // Not open before the hover delay (kittest time: 1/60 s per step).
    h.get_by_label("[523456]").hover();
    h.run_steps(2);
    assert!(h.query_by_label("[7]").is_none());
    let start = Instant::now();
    while h.query_by_label("[7]").is_none() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "sibling list didn't open"
        );
        h.get_by_label("[523456]").hover();
        h.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    h.get_by_label("… 999,800 more");
    h.get_by_label("[7]").click();
    h.run_steps(3);
    assert_eq!(selected_address(&h), [0, 7]);
    h.get_by_label("[7]");
    // Copy path works there too.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::C);
    h.step();
    assert_eq!(copied(&mut h), ["$.items[7]"]);
    h.run_steps(2);
    h.get_by_label("items").click();
    h.run_steps(2);
    assert_eq!(selected_address(&h), [0]);
}

/// Reveal and copy path of a value deeper than `DEEP_LEVELS` run on workers (I-012,
/// I-016) and give the same result as the synchronous code.
#[test]
fn deep_reveal_and_copy_path_run_on_workers() {
    let env = Env::new();
    let depth = braceray_app::tree_view::DEEP_LEVELS + 3_000;
    let text = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    let mut h = open(&env, env.file("deep.json", text.as_bytes()));
    let want = vec![0; depth];
    let repaint = braceray_core::job::no_repaint();
    h.state_mut().tabs[0]
        .tree
        .as_mut()
        .unwrap()
        .reveal_address(want.clone(), &repaint);
    assert!(tree(&h).is_revealing(), "deep reveal goes to a worker");
    step_until(&mut h, "deep reveal", |a| {
        let t = a.tabs[0].tree.as_ref().unwrap();
        !t.is_revealing() && t.selected.and_then(|r| t.model.address(r)) == Some(want.clone())
    });
    assert!(tree(&h).deep_path_job().is_some());
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::C);
    let start = Instant::now();
    let text = loop {
        h.step();
        if let Some(t) = copied(&mut h).pop() {
            break t;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "path copy timed out"
        );
        std::thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(text, format!("${}", "[0]".repeat(depth)));
    h.run_steps(2);
    h.get_by_label("Copied JSONPath");
}
