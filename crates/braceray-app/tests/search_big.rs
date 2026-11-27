//! Step 10 acceptance on the 2 GB fixture (ignored by default; needs
//! `fixtures-data/orders_2g.json` from `gen-fixtures orders --size 2g`). Run optimized:
//!
//! ```text
//! CARGO_PROFILE_DEV_OPT_LEVEL=3 cargo test -p braceray-app --target-dir target/opt3 \
//!     --test search_big -- --ignored --nocapture
//! ```
//!
//! The harness is stepped at 60 frames per second of wall-clock time. The target ("first
//! result shown < 200 ms") is checked from the start of the search, i.e. after the 150 ms
//! typing debounce, to the frame that shows the result (`TabSearch::first_result`); the
//! time from the keystroke is printed too (debounce + one or two frames).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use braceray_app::ui::search::FIELD;
use braceray_app::{AppConfig, BracerayApp};
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

const FRAME: Duration = Duration::from_millis(16);

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-data/orders_2g.json")
}

fn manifest_count(needle: &str) -> u64 {
    let text = std::fs::read_to_string(fixture().with_extension("json.manifest.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["needles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["needle"] == needle)
        .unwrap()["count_case_insensitive"]
        .as_u64()
        .unwrap()
}

/// One frame, then sleep to the next 16 ms tick.
fn frame(h: &mut Harness<'_, BracerayApp>) {
    let t = Instant::now();
    h.step();
    if let Some(rest) = FRAME.checked_sub(t.elapsed()) {
        std::thread::sleep(rest);
    }
}

fn until(
    h: &mut Harness<'_, BracerayApp>,
    what: &str,
    secs: u64,
    done: impl Fn(&BracerayApp) -> bool,
) {
    let start = Instant::now();
    while !done(h.state()) {
        assert!(
            start.elapsed() < Duration::from_secs(secs),
            "timed out: {what}"
        );
        frame(h);
    }
}

/// Types `text` and returns the time from the keystroke to the first frame that shows a
/// result row.
fn type_and_time(h: &mut Harness<'_, BracerayApp>, text: &str) -> Duration {
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    frame(h);
    frame(h);
    h.state_mut().search.text.clear();
    h.get_by_label(FIELD).type_text(text);
    let typed = Instant::now();
    loop {
        frame(h);
        let shown = h.query_all_by_role(egui::accesskit::Role::Row).any(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("#1 "))
        });
        if shown && h.state().tabs[0].search.query().text == text {
            return typed.elapsed();
        }
        assert!(
            typed.elapsed() < Duration::from_secs(20),
            "no results for {text}"
        );
    }
}

#[test]
#[ignore = "needs fixtures-data/orders_2g.json"]
fn first_results_under_200_ms_on_2g_while_indexing_and_after() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = BracerayApp::new(AppConfig {
        settings_path: Some(dir.path().join("settings.json")),
        cache_dir: None, // build the index, so the first search runs during indexing
        hold_indexing: None,
    });
    app.open_path(fixture());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, app: &mut BracerayApp| {
                // Symbols come from system fonts in the app; tests have none.
                ui.ctx()
                    .set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
                app.show(ui)
            },
            app,
        );
    until(&mut h, "mapped", 30, |a| {
        a.tabs.first().is_some_and(|t| t.document().is_some())
    });
    let want = manifest_count("needle_common");

    let mut starts = Vec::new();
    let during = type_and_time(&mut h, "needle_common");
    starts.extend(
        h.state().tabs[0]
            .search
            .first_result
            .map(|d| ("while indexing", d)),
    );
    let indexing = h.state().tabs[0]
        .tree
        .as_ref()
        .is_none_or(|t| t.is_preview());
    until(&mut h, "count", 60, |a| a.tabs[0].search.is_done());
    let count = h.state().tabs[0].search.count();
    println!(
        "while indexing ({indexing}): first results {during:?} after typing; engine {:?}; count {count}",
        h.state().tabs[0].search.first_result
    );
    assert_eq!(count, want);

    until(&mut h, "index", 120, |a| {
        a.tabs[0].tree.as_ref().is_some_and(|t| !t.is_preview())
    });
    let mut after = Vec::new();
    for q in ["email", "needle_common", "NEEDLE_RARE", "needle_common"] {
        let t = type_and_time(&mut h, q);
        println!(
            "indexed: {q}: first results {t:?} after typing; engine {:?}",
            h.state().tabs[0].search.first_result
        );
        // A rare needle's first match may be far into the file; not part of the target.
        if q != "NEEDLE_RARE" {
            after.push(t);
            starts.extend(
                h.state().tabs[0]
                    .search
                    .first_result
                    .map(|d| ("indexed", d)),
            );
        }
        until(&mut h, "search", 60, |a| a.tabs[0].search.is_done());
    }
    assert_eq!(h.state().tabs[0].search.count(), want);
    // Paths fill in now that the index is ready.
    until(&mut h, "paths", 30, |a| !a.tabs[0].search.info.is_empty());
    let row = h.query_all_by_role(egui::accesskit::Role::Row).find(|n| {
        n.accesskit_node()
            .label()
            .is_some_and(|l| l.starts_with("#1 $"))
    });
    assert!(row.is_some(), "a result row with its path");

    // From the search start to the frame showing the result; and a sanity bound from the
    // keystroke (debounce + frames).
    for (what, from_start) in starts {
        assert!(
            from_start < Duration::from_millis(200),
            "{what}: {from_start:?}"
        );
    }
    for t in std::iter::once(during).chain(after) {
        assert!(t < Duration::from_millis(300), "from the keystroke: {t:?}");
    }
}
