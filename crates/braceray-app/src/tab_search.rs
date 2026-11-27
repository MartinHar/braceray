//! Search actions of a tab that touch the tree and the raw view: jump to a match,
//! next / previous from the current selection, and jumps that wait for the full index.

use braceray_core::job::Repaint;
use braceray_core::nav::Span;
use braceray_core::search::Match;

use crate::search_view::{Cursor, Step, WRAPPED_BOTTOM, WRAPPED_TOP};
use crate::tab::OpenDocument;

/// Toast while a jump waits for the index.
pub const WAITING_FOR_INDEX: &str = "Indexing… the match is shown when the index is ready";

impl OpenDocument {
    /// Every frame (after the tree polled): applies a finished next / previous and a jump
    /// that waited for the full index.
    pub fn search_poll(&mut self, repaint: &Repaint) {
        if let Some(step) = self.search.poll() {
            self.apply_step(step, repaint);
        }
        if let Some(forward) = self.search.retry_step
            && (self.search.stored() > 0 || self.search.is_done())
        {
            self.search.retry_step = None;
            self.search_step(forward, repaint);
        }
        if let Some(m) = self.search.pending_jump
            && self.tree.as_ref().is_some_and(|t| !t.is_preview())
        {
            self.search.pending_jump = None;
            self.search_jump(m, repaint);
        }
    }

    /// Selects match `m`: the tree reveals and selects the value holding it, the raw view
    /// scrolls to it. A match past the preview waits for the full index.
    pub fn search_jump(&mut self, m: Match, repaint: &Repaint) {
        self.search.set_current(m);
        let Some(tree) = self.tree.as_mut() else {
            self.search.pending_jump = Some(m);
            return;
        };
        if tree.is_preview() && m.end() > tree.browsed_len() {
            self.search.pending_jump = Some(m);
            self.search.toast(WAITING_FOR_INDEX);
            return;
        }
        self.search.pending_jump = None;
        if let Some(raw) = self.panes.raw.as_mut() {
            raw.reveal(
                tree,
                Span {
                    start: m.offset,
                    end: m.end(),
                },
                repaint,
            );
        }
        self.panes.locate(tree, m.offset, repaint);
    }

    /// Selects stored match `i` (a results row).
    pub fn search_jump_index(&mut self, i: usize, repaint: &Repaint) {
        if let Some(m) = self.search.get(i) {
            self.search_jump(m, repaint);
        }
    }

    /// Next (`forward`) or previous match from the current selection, wrapping around.
    pub fn search_step(&mut self, forward: bool, repaint: &Repaint) {
        if self.search.is_stepping() {
            return;
        }
        let cursor = self.search_cursor(forward);
        if let Some(step) = self.search.start_step(cursor, forward, repaint) {
            self.apply_step(step, repaint);
        }
    }

    /// The current match while the selection still holds it (or the tree is still
    /// revealing it); otherwise the start of the selected value; otherwise the file's
    /// start (forward) or end (backward).
    fn search_cursor(&self, forward: bool) -> Cursor {
        let selected = self.tree.as_ref().and_then(|t| t.selected_spans());
        let selected = selected.map(|(key, value)| Span {
            start: key.map_or(value.start, |k| k.start),
            end: value.end,
        });
        if let Some(c) = self.search.current {
            let holds = selected.is_none_or(|s| s.start <= c.m.offset && c.m.offset < s.end);
            if holds || self.panes.is_locating() || self.search.pending_jump.is_some() {
                return Cursor::Match(c.m);
            }
        }
        match selected {
            Some(s) => Cursor::Offset(s.start),
            None if forward => Cursor::Offset(0),
            None => Cursor::Offset(u64::MAX),
        }
    }

    fn apply_step(&mut self, step: Step, repaint: &Repaint) {
        match step {
            Step::Found(m) => self.search_jump(m, repaint),
            Step::WrappedTop(m) => {
                self.search_jump(m, repaint);
                self.search.toast(WRAPPED_TOP);
            }
            Step::WrappedBottom(m) => {
                self.search_jump(m, repaint);
                self.search.toast(WRAPPED_BOTTOM);
            }
            Step::NotYet { forward } => self.search.retry_step = Some(forward),
            Step::Nothing => {}
        }
    }
}
