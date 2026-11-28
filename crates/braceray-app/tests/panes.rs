//! UI tests of the pane toggles (I-013): hide the tree or the raw pane from the toolbar or
//! with Ctrl/Cmd+1 / Ctrl/Cmd+2, never both, persisted in the settings.

use std::path::Path;
use std::time::{Duration, Instant};

use braceray_app::settings::Settings;
use braceray_app::ui::raw::PRETTY;
use braceray_app::{AppConfig, BracerayApp};
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;

const ROOT_ROW: &str = "root : {2}";

fn open(dir: &Path) -> Harness<'static, BracerayApp> {
    let file = dir.join("a.json");
    std::fs::write(&file, br#"{"a": 1, "b": [true, null]}"#).unwrap();
    let mut app = BracerayApp::new(AppConfig {
        settings_path: Some(dir.join("settings.json")),
        cache_dir: None,
        hold_indexing: None,
    });
    app.open_path(file);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(
            |ui, app: &mut BracerayApp| {
                // Tests have no system fonts for some symbols.
                ui.ctx()
                    .set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
                app.show(ui)
            },
            app,
        );
    let start = Instant::now();
    while !h.state().tabs[0]
        .tree
        .as_ref()
        .is_some_and(|v| !v.is_preview() && v.jobs_running() == 0)
    {
        assert!(start.elapsed() < Duration::from_secs(20), "index timed out");
        h.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    h.run_steps(3);
    h
}

/// `(tree row visible, raw pane visible)`.
fn visible(h: &Harness<'_, BracerayApp>) -> (bool, bool) {
    (
        h.query_by_label(ROOT_ROW).is_some(),
        h.query_by_label(PRETTY).is_some(),
    )
}

fn saved(dir: &Path) -> (bool, bool) {
    let s = Settings::from_json(&std::fs::read_to_string(dir.join("settings.json")).unwrap());
    (s.show_tree, s.show_raw)
}

#[test]
fn panes_toggle_from_toolbar_and_keys_and_persist() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    assert_eq!(visible(&h), (true, true));

    // Toolbar: hide the raw pane; the tree fills the area.
    h.get_by_label("Raw pane").click();
    h.run_steps(3);
    assert_eq!(visible(&h), (true, false));
    assert_eq!(saved(dir.path()), (true, false));

    // Hiding the tree too would leave nothing: the raw pane comes back instead.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num1);
    h.run_steps(3);
    assert_eq!(visible(&h), (false, true));
    assert_eq!(saved(dir.path()), (false, true));
    assert_eq!(
        h.state().tabs[0].panes.focus,
        braceray_app::panes::Pane::Raw,
        "focus leaves the hidden tree"
    );

    // Mod+1 again: both shown.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num1);
    h.run_steps(3);
    assert_eq!(visible(&h), (true, true));

    // Mod+2 hides the raw pane; a new app starts with the saved layout.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num2);
    h.run_steps(3);
    assert_eq!(visible(&h), (true, false));
    drop(h);
    let h = open(dir.path());
    assert_eq!(visible(&h), (true, false));
}
