use braceray_app::BracerayApp;
use egui_kittest::{Harness, kittest::Queryable as _};

#[test]
fn empty_window_shows_drop_hint() {
    let mut harness = Harness::new_ui_state(
        |ui, app: &mut BracerayApp| app.show(ui),
        BracerayApp::default(),
    );
    harness.run();
    harness.get_by_label("Drop a JSON file here");
}
