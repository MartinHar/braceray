//! Debug overlay (`Ctrl+Shift+D`): frame times and tree numbers, for checking the 16 ms
//! frame budget while scrolling.

use std::collections::VecDeque;

use eframe::egui;

/// Frames kept for the statistics.
const KEEP: usize = 600;
/// Frames between two log lines while the overlay is open.
const LOG_EVERY: u64 = 300;

/// Title of the overlay window.
pub const TITLE: &str = "Debug";

/// Recent frame times.
#[derive(Debug, Default)]
pub struct FrameStats {
    /// The overlay is shown.
    pub open: bool,
    /// Time spent in the app's UI code per frame (ms).
    ui_ms: VecDeque<f32>,
    /// Full CPU time of the previous frame as eframe reports it (ms; layout,
    /// tessellation included).
    cpu_ms: VecDeque<f32>,
    frames: u64,
    /// Tree rows drawn in the last frame.
    pub rows_drawn: usize,
}

/// p50 / p99 / max of a sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    /// Median.
    pub p50: f32,
    /// 99th percentile.
    pub p99: f32,
    /// Maximum.
    pub max: f32,
}

impl FrameStats {
    /// Records one frame.
    pub fn record(&mut self, ui_ms: f32, cpu_ms: Option<f32>) {
        push(&mut self.ui_ms, ui_ms);
        if let Some(c) = cpu_ms {
            push(&mut self.cpu_ms, c);
        }
        self.frames += 1;
        if self.open && self.frames.is_multiple_of(LOG_EVERY) {
            tracing::info!(
                "frame times over {} frames: ui {:?} ms, cpu {:?} ms, rows drawn {}",
                self.ui_ms.len(),
                summary(&self.ui_ms),
                summary(&self.cpu_ms),
                self.rows_drawn
            );
        }
    }

    /// UI-code time of the recent frames.
    pub fn ui_summary(&self) -> Option<Summary> {
        summary(&self.ui_ms)
    }

    /// Full CPU frame time of the recent frames.
    pub fn cpu_summary(&self) -> Option<Summary> {
        summary(&self.cpu_ms)
    }

    /// Forgets the recorded frames.
    pub fn reset(&mut self) {
        self.ui_ms.clear();
        self.cpu_ms.clear();
    }
}

fn push(q: &mut VecDeque<f32>, v: f32) {
    if q.len() == KEEP {
        q.pop_front();
    }
    q.push_back(v);
}

fn summary(q: &VecDeque<f32>) -> Option<Summary> {
    if q.is_empty() {
        return None;
    }
    let mut v: Vec<f32> = q.iter().copied().collect();
    v.sort_by(f32::total_cmp);
    let at = |p: f32| v[((v.len() - 1) as f32 * p).round() as usize];
    Some(Summary {
        p50: at(0.5),
        p99: at(0.99),
        max: v[v.len() - 1],
    })
}

fn line(name: &str, s: Option<Summary>) -> String {
    match s {
        Some(s) => format!(
            "{name} p50 {:.2} · p99 {:.2} · max {:.2} ms",
            s.p50, s.p99, s.max
        ),
        None => format!("{name}: no frames yet"),
    }
}

/// Label of the "expand everything" button.
pub const EXPAND_EVERYTHING: &str = "Expand everything (no limit)";

/// Draws the overlay with `extra` lines (tree numbers). Returns `true` when "Expand
/// everything" was clicked.
pub fn show(ctx: &egui::Context, stats: &mut FrameStats, extra: &[String]) -> bool {
    if !stats.open {
        return false;
    }
    let mut open = true;
    let mut expand = false;
    egui::Window::new(TITLE)
        .open(&mut open)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 48.0))
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(format!("Frames: {}", stats.ui_ms.len()));
            ui.label(line("UI", stats.ui_summary()));
            ui.label(line("CPU frame", stats.cpu_summary()));
            ui.label(format!("Rows drawn: {}", stats.rows_drawn));
            for l in extra {
                ui.label(l);
            }
            ui.horizontal(|ui| {
                if ui.button("Reset").clicked() {
                    stats.reset();
                }
                expand = ui
                    .button(EXPAND_EVERYTHING)
                    .on_hover_text("Expands every row of the active tree on the UI thread (for frame-time tests).")
                    .clicked();
            });
        });
    stats.open = open;
    expand
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles() {
        let mut s = FrameStats::default();
        assert_eq!(s.ui_summary(), None);
        for i in 1..=100 {
            s.record(i as f32, None);
        }
        let sum = s.ui_summary().unwrap();
        assert_eq!((sum.p50, sum.p99, sum.max), (51.0, 99.0, 100.0));
        for _ in 0..KEEP {
            s.record(1.0, Some(2.0));
        }
        assert_eq!(s.ui_summary().unwrap().max, 1.0);
        assert_eq!(s.cpu_summary().unwrap().p50, 2.0);
    }
}
