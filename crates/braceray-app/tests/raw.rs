//! UI tests of the raw view (step 08): modes, Go to, tree ↔ raw sync, selection and copy.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use braceray_app::raw_view::{RawMode, RawView};
use braceray_app::tree_view::TreeView;
use braceray_app::ui::raw::{ORIGINAL, PRETTY};
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

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn open(&self, path: PathBuf) -> Harness<'static, BracerayApp> {
        let mut app = BracerayApp::new(AppConfig {
            settings_path: Some(self.dir.path().join("settings.json")),
            cache_dir: None,
            hold_indexing: None,
        });
        app.open_path(path);
        let mut h = Harness::builder()
            .with_size(egui::vec2(1200.0, 700.0))
            .with_step_dt(1.0 / 60.0)
            .build_ui_state(
                |ui, app: &mut BracerayApp| {
                    // Tests have no system fonts for the CJK text in some fixtures.
                    ui.ctx()
                        .set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
                    app.show(ui)
                },
                app,
            );
        step_until(&mut h, "index and raw view", |a| {
            a.tabs.first().is_some_and(|t| {
                t.tree
                    .as_ref()
                    .is_some_and(|v| !v.is_preview() && v.jobs_running() == 0)
                    && t.panes.raw.as_ref().is_some_and(|r| !r.is_seeking())
            })
        });
        h
    }
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

fn raw<'h>(h: &'h Harness<'_, BracerayApp>) -> &'h RawView {
    h.state().tabs[0].panes.raw.as_ref().unwrap()
}

/// Labels of the raw rows on screen (their text).
fn raw_rows(h: &Harness<'_, BracerayApp>) -> Vec<String> {
    let mut rows: Vec<(f32, String)> = h
        .query_all_by_role(egui::accesskit::Role::Row)
        .filter(|n| n.rect().left() > 400.0) // the raw pane is on the right
        .map(|n| {
            (
                n.rect().top(),
                n.accesskit_node().label().unwrap_or_default(),
            )
        })
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.into_iter().map(|(_, l)| l).collect()
}

/// The screen position of the first character of `needle` in the raw row containing it.
fn raw_pos(h: &Harness<'_, BracerayApp>, needle: &str) -> egui::Pos2 {
    let node = h
        .query_all_by_role(egui::accesskit::Role::Row)
        .filter(|n| n.rect().left() > 400.0)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.contains(needle))
        })
        .unwrap_or_else(|| panic!("no raw row contains {needle:?}: {:?}", raw_rows(h)));
    let label = node.accesskit_node().label().unwrap();
    let chars = label[..label.find(needle).unwrap()].chars().count() as f32;
    let r = node.rect();
    // Monospace 13 pt: measure the glyph width from egui itself.
    let w = h
        .ctx
        .fonts_mut(|f| f.glyph_width(&egui::FontId::monospace(13.0), '0'));
    egui::pos2(r.left() + 4.0 + (chars + 0.8) * w, r.center().y)
}

fn press(h: &mut Harness<'_, BracerayApp>, pos: egui::Pos2, pressed: bool, modifiers: Modifiers) {
    h.event(egui::Event::PointerMoved(pos));
    // `event_modifiers` also sets the input's modifier state (Shift+click).
    h.event_modifiers(
        egui::Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        },
        modifiers,
    );
    h.step();
}

fn click_at(h: &mut Harness<'_, BracerayApp>, pos: egui::Pos2) {
    press(h, pos, true, Modifiers::NONE);
    press(h, pos, false, Modifiers::NONE);
    h.run_steps(2);
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

const ORDERS: &str = r#"{
  "orders": [
    {"id": 1, "items": [{"sku": "A-1", "qty": 3}, {"sku": "B-2", "qty": 1}]},
    {"id": 2, "items": []},
    {"id": 3, "note": "Երևան 東京"}
  ],
  "count": 3
}
"#;

#[test]
fn raw_click_selects_the_value_in_the_tree() {
    let env = Env::new();
    let mut h = env.open(env.file("o.json", ORDERS.as_bytes()));
    assert_eq!(raw(&h).mode, RawMode::Original);
    let p = raw_pos(&h, "\"qty\": 1");
    click_at(&mut h, p);
    step_until(&mut h, "tree reveal", |a| {
        let t = a.tabs[0].tree.as_ref().unwrap();
        t.jobs_running() == 0 && !a.tabs[0].panes.is_locating()
    });
    assert_eq!(selected_label(&h), "qty : 1");
    // The raw view stayed where it was (the click came from it).
    assert_eq!(raw(&h).top_offset(), 0);
}

#[test]
fn mode_toggle_formats_and_keeps_the_position() {
    let env = Env::new();
    let mut h = env.open(env.file("o.json", ORDERS.as_bytes()));
    assert!(raw_rows(&h)[2].contains(r#"{"id": 1, "items": [{"sku": "A-1""#));
    h.get_by_label(PRETTY).click();
    h.run_steps(3);
    assert_eq!(raw(&h).mode, RawMode::Pretty);
    let rows = raw_rows(&h);
    assert_eq!(&rows[..4], ["{", "\"orders\": [", "{", "\"id\": 1,"]);
    // Rows are indented by position, not by spaces: the depth is in the model.
    h.get_by_label(ORIGINAL).click();
    h.run_steps(3);
    assert_eq!(raw(&h).mode, RawMode::Original);
    assert_eq!(raw_rows(&h)[0], "{");
}

#[test]
fn minified_files_open_in_pretty_mode() {
    let env = Env::new();
    // One line, longer than 2,000 bytes on average.
    let mut doc = String::from("[");
    for i in 0..200 {
        doc.push_str(&format!(r#"{{"id":{i},"name":"item {i}"}},"#));
    }
    doc.push_str("null]");
    let mut h = env.open(env.file("min.json", doc.as_bytes()));
    assert_eq!(raw(&h).mode, RawMode::Pretty);
    let rows = raw_rows(&h);
    assert_eq!(&rows[..4], ["[", "{", "\"id\": 0,", "\"name\": \"item 0\""]);
    // Scroll with the keyboard after a click in the view; the top moves by rows.
    let p = raw_pos(&h, "\"id\": 0");
    click_at(&mut h, p);
    h.key_press(Key::PageDown);
    h.run_steps(2);
    assert!(raw(&h).top_offset() > 100, "{}", raw(&h).top_offset());
    h.key_press(Key::Home);
    h.run_steps(2);
    assert_eq!(raw(&h).top_offset(), 0);
}

fn go_to(h: &mut Harness<'_, BracerayApp>, text: &str) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::L);
    h.run_steps(2);
    // The Go to field (not the search field, which has a label).
    h.query_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| n.accesskit_node().label().is_none())
        .expect("the Go to field")
        .type_text(text);
    h.run_steps(1);
    h.key_press(Key::Enter);
    // Done: the dialog closed (success) or shows an error.
    step_until(h, "go to", |a| {
        a.goto
            .as_ref()
            .is_none_or(|d| !d.is_busy() && d.error.is_some())
            && !a.tabs[0].panes.is_locating()
            && a.tabs[0].tree.as_ref().unwrap().jobs_running() == 0
    });
}

#[test]
fn go_to_line_offset_and_path() {
    let env = Env::new();
    // 3,000 lines: "go to line" must scroll.
    let mut doc = String::from("[\n");
    for i in 0..3000 {
        doc.push_str(&format!("  {{\"n\": {i}}},\n"));
    }
    doc.push_str("  null\n]\n");
    let mut h = env.open(env.file("lines.json", doc.as_bytes()));
    assert_eq!(raw(&h).mode, RawMode::Original);

    go_to(&mut h, "2,345");
    assert!(h.state().goto.is_none(), "dialog closes on success");
    let rows = raw_rows(&h);
    // Two rows of context above the target line.
    assert_eq!(rows[2], "  {\"n\": 2343},");
    let line_start = doc.match_indices('\n').nth(2343).unwrap().0 as u64 + 1;
    assert_eq!(raw(&h).flash.unwrap().0.start, line_start);
    assert_eq!(selected_label(&h), "[2,343] : {1}");

    go_to(&mut h, &format!("@{}", doc.find("{\"n\": 77}").unwrap()));
    assert_eq!(raw_rows(&h)[2], "  {\"n\": 77},");
    assert_eq!(selected_label(&h), "[77] : {1}");

    go_to(&mut h, "$[1500].n");
    assert_eq!(raw_rows(&h)[2], "  {\"n\": 1500},");
    assert_eq!(selected_label(&h), "n : 1500");

    // Errors stay in the dialog.
    go_to(&mut h, "$[5000]");
    let err = h.state().goto.as_ref().unwrap().error.clone().unwrap();
    assert_eq!(err, "$ has no element [5000].");
    h.get_by_label_contains("$ has no element").accesskit_node();
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().goto.is_none());
    go_to(&mut h, "99999");
    assert!(
        h.state()
            .goto
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .starts_with("Line 99,999 is past the end")
    );
}

#[test]
fn tree_selection_scrolls_the_raw_view_and_flashes() {
    let env = Env::new();
    let mut doc = String::from("{\"list\": [\n");
    for i in 0..500 {
        doc.push_str(&format!(
            "  {{\"n\": {i}, \"deep\": [[{{\"x\": {i}}}]]}},\n"
        ));
    }
    doc.push_str("  null\n]}\n");
    let mut h = env.open(env.file("t.json", doc.as_bytes()));
    // Reveal a deep value through the tree API (as a click or a key would).
    let target = doc.find("\"x\": 321").unwrap() as u64;
    let address = vec![0, 321, 1, 0, 0, 0];
    let repaint = braceray_core::job::no_repaint();
    h.state_mut().tabs[0]
        .tree
        .as_mut()
        .unwrap()
        .reveal_address(address, &repaint);
    step_until(&mut h, "raw follows", |a| {
        a.tabs[0].panes.raw.as_ref().unwrap().flash.is_some()
    });
    assert_eq!(selected_label(&h), "x : 321");
    let flash = raw(&h).flash.unwrap().0;
    assert_eq!(flash.start, target);
    assert_eq!(raw_rows(&h)[2], r#"  {"n": 321, "deep": [[{"x": 321}]]},"#);
    // A click on another tree row scrolls the raw view there and flashes it.
    h.get_by_label("n : 321").click();
    h.run_steps(3);
    assert_eq!(selected_label(&h), "n : 321");
    let key = doc.find("\"n\": 321").unwrap() as u64;
    assert_eq!(raw(&h).flash.unwrap().0.start, key);
    // Arrow keys in the tree move the raw view along.
    h.key_press(Key::ArrowUp);
    h.run_steps(3);
    assert_eq!(selected_label(&h), "[321] : {2}");
    let record = doc[..key as usize].rfind('{').unwrap() as u64;
    assert_eq!(raw(&h).flash.unwrap().0.start, record);
    assert_eq!(raw_rows(&h)[2], r#"  {"n": 321, "deep": [[{"x": 321}]]},"#);
}

#[test]
fn drag_selection_copies_the_exact_source_bytes() {
    let env = Env::new();
    let mut h = env.open(env.file("o.json", ORDERS.as_bytes()));
    let a = raw_pos(&h, "{\"sku\": \"A-1\"");
    let b = raw_pos(&h, "\"count\"");
    press(&mut h, a, true, Modifiers::NONE);
    for k in 1..=5 {
        let p = a + (b - a) * (k as f32 / 5.0);
        h.event(egui::Event::PointerMoved(p));
        h.step();
    }
    press(&mut h, b, false, Modifiers::NONE);
    h.run_steps(2);
    let span = raw(&h).selected_span().expect("a selection");
    // The caret positions are character boundaries; compare with the file bytes.
    let start = ORDERS.find("{\"sku\": \"A-1\"").unwrap() as u64;
    assert!(span.start.abs_diff(start) <= 1, "{span:?} vs {start}");
    h.event(egui::Event::Copy);
    h.run_steps(1);
    let text = copied(&h);
    assert_eq!(text.len(), 1);
    assert_eq!(
        text[0].as_bytes(),
        &ORDERS.as_bytes()[span.start as usize..span.end as usize]
    );
    assert!(text[0].contains("Երևան 東京"));

    // Shift+click extends the selection.
    let w = h
        .ctx
        .fonts_mut(|f| f.glyph_width(&egui::FontId::monospace(13.0), '0'));
    let c = raw_pos(&h, "\"count\": 3") + egui::vec2(10.0 * w, 0.0); // on the 3
    press(&mut h, c, true, Modifiers::SHIFT);
    press(&mut h, c, false, Modifiers::SHIFT);
    h.run_steps(1);
    assert!(raw(&h).selected_span().unwrap().end > span.end);
}

#[test]
fn copy_over_the_limit_shows_a_message() {
    let env = Env::new();
    // 51 MB: a big array of small numbers.
    let mut doc = Vec::with_capacity(52 << 20);
    doc.push(b'[');
    while doc.len() < 51 << 20 {
        doc.extend_from_slice(b"12345678,");
    }
    doc.extend_from_slice(b"0]");
    let mut h = env.open(env.file("big.json", &doc));
    let p = raw_pos(&h, "12345678");
    click_at(&mut h, p);
    h.state_mut().tabs[0].panes.raw.as_mut().unwrap().selection = Some((0, doc.len() as u64));
    h.event(egui::Event::Copy);
    h.run_steps(2);
    assert!(copied(&h).is_empty());
    let notice = raw(&h).notice.clone().unwrap().0;
    assert!(notice.contains("the copy limit is"), "{notice}");
    h.get_by_label_contains("the copy limit is");
}

#[test]
fn mouse_wheel_scrolls_by_rows_both_ways() {
    let env = Env::new();
    let mut doc = String::from("[");
    for i in 0..300 {
        doc.push_str(&format!(r#"{{"id":{i},"tags":["a","b"]}},"#));
    }
    doc.push_str("null]");
    let mut h = env.open(env.file("w.json", doc.as_bytes()));
    assert_eq!(raw(&h).mode, RawMode::Pretty);
    let p = raw_pos(&h, "\"id\": 0");
    h.event(egui::Event::PointerMoved(p));
    for _ in 0..10 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -3.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
            source: egui::MouseWheelSource::Wheel,
        });
        h.step();
    }
    h.run_steps(30); // smooth scrolling settles
    let down = raw(&h).top_offset();
    assert!(down > 200, "scrolled down to {down}");
    for _ in 0..10 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 3.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
            source: egui::MouseWheelSource::Wheel,
        });
        h.step();
    }
    h.run_steps(30);
    assert_eq!(raw(&h).top_offset(), 0);
    // Scrolling up doesn't keep laying out the rows it left below the screen.
    let repaint = braceray_core::job::no_repaint();
    let tab = &mut h.state_mut().tabs[0];
    let (tree, raw_view) = (tab.tree.as_ref().unwrap(), tab.panes.raw.as_mut().unwrap());
    raw_view.scroll_by(tree, 600.0, 30, &repaint);
    let far = raw_view.top_offset();
    raw_view.scroll_by(tree, -600.0, 30, &repaint);
    assert!(far > 1000 && raw_view.top_offset() == 0, "{far}");
    h.run_steps(2);
    assert!(raw(&h).cached_rows() < 200, "{}", raw(&h).cached_rows());
    assert!(h.state().frame_stats.rows_drawn < 200);
}

#[test]
fn mouse_wheel_still_scrolls_the_tree_pane() {
    let env = Env::new();
    let mut doc = String::from("[");
    for i in 0..500 {
        doc.push_str(&format!("{i},"));
    }
    doc.push_str("0]");
    let mut h = env.open(env.file("t.json", doc.as_bytes()));
    let p = h.get_by_label("[10] : 10").rect().center();
    h.event(egui::Event::PointerMoved(p));
    for _ in 0..5 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -3.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
            source: egui::MouseWheelSource::Wheel,
        });
        h.step();
    }
    h.run_steps(30);
    assert!(tree(&h).scroll > 5.0, "tree scroll {}", tree(&h).scroll);
    assert_eq!(raw(&h).top_offset(), 0, "the raw view didn't move");
}
