use eframe::egui;

/// Text shown in the empty window before a file is opened.
pub const DROP_HINT: &str = "Drop a JSON file here";

/// Top-level application state.
#[derive(Debug, Default)]
pub struct BracerayApp {}

impl BracerayApp {
    /// Draws the whole app into `ui`. Shared by the real window and UI tests.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.centered_and_justified(|ui| {
                ui.heading(DROP_HINT);
            });
        });
    }
}

impl eframe::App for BracerayApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}
