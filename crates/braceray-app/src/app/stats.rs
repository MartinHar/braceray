//! Numbers for the debug overlay.

use std::time::Duration;

use super::BracerayApp;

impl BracerayApp {
    /// Tree numbers for the debug overlay (only computed while it is open).
    pub(super) fn debug_lines(&self) -> Vec<String> {
        if !self.frame_stats.open {
            return Vec::new();
        }
        let Some(tab) = self.active_tab() else {
            return vec!["No tree".to_owned()];
        };
        let Some(tree) = tab.tree.as_ref() else {
            return vec!["No tree".to_owned()];
        };
        let ms = |d: Option<Duration>| {
            d.map_or("–".to_owned(), |d| {
                format!("{:.2} ms", d.as_secs_f64() * 1000.0)
            })
        };
        vec![
            format!("First rows after open: {}", ms(tab.first_rows)),
            format!(
                "Last expand: {} · last prepare: {}",
                ms(tree.timings.expand),
                ms(tree.timings.prepare)
            ),
            format!(
                "Tree rows: {} · segments: {}",
                crate::format::group_digits(tree.model.row_count()),
                tree.model.segment_count()
            ),
            format!(
                "Checkpoint cache: {} · jobs: {}{}",
                crate::format::bytes(tree.cache_bytes() as u64),
                tree.jobs_running(),
                if tree.is_preview() { " · preview" } else { "" }
            ),
            format!(
                "Search: first result {} ({} after typing) · finished in {} · {} stored",
                ms(tab.search.first_result),
                ms(tab.search.first_result_typed),
                ms(tab.search.handle().and_then(|h| h.duration())),
                crate::format::group_digits(tab.search.stored() as u64)
            ),
        ]
    }
}
