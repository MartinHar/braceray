//! Work that is O(depth) on very deep documents runs on a worker (I-012, I-016).
//!
//! Revealing a value tens of thousands of levels deep (raw click, Go to, a search jump)
//! expands every level, and the path of such a value reads every level: tens of ms on
//! the UI thread at depth 50,000+. Above [`DEEP_LEVELS`] the work runs on a clone of the
//! row model on a worker; the result is applied on a later frame if the tree didn't
//! change meanwhile (otherwise the reveal starts again from the current tree).

use std::sync::Arc;

use braceray_core::Document;
use braceray_core::index::ContainerKind;
use braceray_core::job::{JobEvent, JobHandle, Repaint};
use braceray_core::nav::{NavCache, Navigator, PathSegment};
use braceray_core::tree::{Reveal, RowItem, TreeModel, ValueRow};

use super::{Source, Then, TreeView, navigator};

/// Reveals and paths deeper than this many levels run on a worker.
pub const DEEP_LEVELS: usize = 2_000;

/// A reveal running on a worker.
pub(super) struct RevealJob {
    /// Model version it started from.
    version: u64,
    address: Vec<u64>,
    job: JobHandle<(), Revealed>,
}

/// What the worker hands back: the model with the value revealed.
pub(super) struct Revealed {
    model: TreeModel,
    result: Result<u64, Reveal>,
    /// The old selection, found again if rows were inserted.
    selected: Option<u64>,
    added: bool,
}

/// The path of a deep selection, built on a worker with [`PathJob::run`].
pub struct PathJob {
    model: TreeModel,
    row: u64,
    doc: Arc<Document>,
    source: Source,
    cache: Arc<NavCache>,
}

impl PathJob {
    /// The document the path is read from (for formatting the path).
    pub fn document(&self) -> &Arc<Document> {
        &self.doc
    }

    /// Reads every level from the root to the selected row. O(depth).
    pub fn run(&self) -> Option<Vec<PathSegment>> {
        let nav = navigator(&self.doc, &self.source, &self.cache);
        path_of(&self.model, &nav, self.row)
    }
}

/// Path of value row `row` from the root (empty for the root itself).
pub(super) fn path_of(
    model: &TreeModel,
    nav: &Navigator<'_>,
    row: u64,
) -> Option<Vec<PathSegment>> {
    let rows = model.ancestors(row);
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let v: ValueRow = match model.rows(nav, r..r + 1).rows.pop()?.item {
            RowItem::Value(v) => v,
            _ => return None,
        };
        match (v.parent, v.child.key) {
            (None, _) => {}
            (Some(ContainerKind::Object), Some(k)) => out.push(PathSegment::Key(k)),
            _ => out.push(PathSegment::Index(v.index)),
        }
    }
    Some(out)
}

impl TreeView {
    /// Expands what is needed to show the value at `address` (child indices from the
    /// top-level value), selects it and scrolls to it; waits for prepare jobs on the way.
    pub fn reveal_address(&mut self, address: Vec<u64>, repaint: &Repaint) {
        if address.len() > DEEP_LEVELS {
            self.reveal_on_worker(address, repaint);
            return;
        }
        self.reveal_job = None; // a newer reveal wins
        let nav = navigator(&self.doc, &self.source, &self.cache);
        let before = self.model.row_count();
        let selected = self.selected.and_then(|s| self.model.address(s));
        let result = self.model.reveal(&nav, &address);
        if self.model.row_count() != before {
            // Rows were inserted: find the old selection again (it may have moved).
            self.selected = selected.and_then(|a| self.model.reveal(&nav, &a).ok());
            self.changed();
        }
        self.finish_reveal(address, result, repaint);
    }

    /// Selects a revealed value, or waits for the container a reveal needs.
    pub(super) fn finish_reveal(
        &mut self,
        address: Vec<u64>,
        result: Result<u64, Reveal>,
        repaint: &Repaint,
    ) {
        match result {
            Ok(row) => self.select(row),
            Err(Reveal::Pending(c)) => {
                self.then = Some((c.start, self.version, Then::Reveal(address)));
                self.request_prepare(c, repaint);
            }
            Err(Reveal::NotFound) => {}
        }
    }

    /// Starts revealing `address` on a worker (replaces a running reveal).
    pub(super) fn reveal_on_worker(&mut self, address: Vec<u64>, repaint: &Repaint) {
        let (doc, source, cache) = self.shared();
        let mut model = self.model.clone();
        let selected = self.selected;
        let target = address.clone();
        let job = JobHandle::spawn("tree-reveal", 0, repaint.clone(), move |_| {
            let nav = navigator(&doc, &source, &cache);
            let before = model.row_count();
            let old = selected.and_then(|s| model.address(s));
            let result = model.reveal(&nav, &target);
            let added = model.row_count() != before;
            let selected = if added {
                old.and_then(|a| model.reveal(&nav, &a).ok())
            } else {
                selected
            };
            Ok(Revealed {
                model,
                result,
                selected,
                added,
            })
        });
        self.reveal_job = Some(RevealJob {
            version: self.version,
            address,
            job,
        });
    }

    /// Every frame: applies a finished worker reveal.
    pub(super) fn poll_reveal(&mut self, repaint: &Repaint) {
        let Some(running) = self.reveal_job.as_mut() else {
            return;
        };
        let event = match running.job.try_recv() {
            Some(e) => e,
            None if running.job.is_finished() => {
                self.reveal_job = None;
                return;
            }
            None => return,
        };
        let Some(running) = self.reveal_job.take() else {
            return;
        };
        let JobEvent::Done(done) = event else {
            return;
        };
        if running.version != self.version {
            // The tree changed while the worker ran: start again from the current one.
            self.reveal_address(running.address, repaint);
            return;
        }
        self.model = done.model;
        if done.added {
            self.selected = done.selected;
            self.changed();
        }
        self.finish_reveal(running.address, done.result, repaint);
    }

    /// `true` while a reveal runs on a worker.
    pub fn is_revealing(&self) -> bool {
        self.reveal_job.is_some()
    }

    /// For a selection deeper than [`DEEP_LEVELS`]: a job that builds its path off the UI
    /// thread. `None` when nothing is selected or the selection is shallow (use
    /// [`TreeView::selected_path`]).
    pub fn deep_path_job(&self) -> Option<PathJob> {
        let row = self.selected?;
        if self.crumb_rows().len() <= DEEP_LEVELS {
            return None;
        }
        let (doc, source, cache) = self.shared();
        Some(PathJob {
            model: self.model.clone(),
            row,
            doc,
            source,
            cache,
        })
    }
}
