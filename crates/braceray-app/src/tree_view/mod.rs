//! State of one tab's tree: the row model over the preview or the full index, the
//! selection, scrolling, and the background jobs that prepare big containers.

use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use braceray_core::job::{JobEvent, JobHandle, Repaint};
use braceray_core::nav::{Container, NavCache, Navigator, Span};
use braceray_core::tree::preview::Preview;
use braceray_core::tree::{Expand, Reveal, Row, RowItem, Rows, TreeModel};
use braceray_core::{Document, Index};

mod keys;

pub use keys::TreeKey;

/// Rows that `*` and "Expand all below" add before asking to continue.
pub const EXPAND_ALL_BUDGET: u64 = 10_000;

/// Prepare jobs running at once (more are started on later frames).
const MAX_JOBS: usize = 4;

/// What the tree browses.
#[derive(Debug, Clone)]
pub enum Source {
    /// An index of the first few MB, while the full index builds.
    Preview(Arc<Preview>),
    /// The full index.
    Full(Arc<Index>),
}

/// Runs after a container is prepared, if nothing changed meanwhile.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Then {
    Expand(u64),
    ExpandAll { row: u64, levels: u32, budget: u64 },
    Reveal(Vec<u64>),
}

/// An "expand all" that stopped at the row budget and waits for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AskContinue {
    /// The row being expanded.
    pub row: u64,
    /// Levels below it.
    pub levels: u32,
    /// Rows added so far.
    pub added: u64,
}

/// Timings for the debug overlay and the log.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TreeTimings {
    /// The last expand (model update only).
    pub expand: Option<Duration>,
    /// The last finished prepare job (checkpoint table build on a worker).
    pub prepare: Option<Duration>,
}

/// Tree state of one tab.
pub struct TreeView {
    /// Recent timings.
    pub timings: TreeTimings,
    doc: Arc<Document>,
    source: Source,
    cache: Arc<NavCache>,
    /// The visible rows.
    pub model: TreeModel,
    /// Selected row.
    pub selected: Option<u64>,
    /// Scroll position in rows (fractional: the top row is `scroll.floor()`).
    pub scroll: f64,
    /// Bring this row into view on the next frame.
    pub scroll_to: Option<u64>,
    /// An "expand all" waiting for "Continue".
    pub ask_continue: Option<AskContinue>,
    jobs: Vec<(u64, JobHandle<(), Duration>)>,
    then: Option<(u64, u64, Then)>,
    /// Bumped by every model change (a pending "expand after prepare" is dropped then).
    version: u64,
    /// Bumped whenever rows may read differently (model change, prepared container).
    generation: u64,
    rows_cache: Option<(Range<u64>, u64, Rows)>,
    source_generation: u64,
    /// (selected row, generation, its spans).
    selected_cache: RefCell<Option<SelectedSpans>>,
}

type SelectedSpans = (u64, u64, Option<(Option<Span>, Span)>);

impl std::fmt::Debug for TreeView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeView")
            .field("rows", &self.model.row_count())
            .field("selected", &self.selected)
            .field("preview", &self.is_preview())
            .finish_non_exhaustive()
    }
}

/// A navigator over `source` (a free function so callers can borrow other fields mutably).
pub fn navigator<'a>(doc: &'a Document, source: &'a Source, cache: &'a NavCache) -> Navigator<'a> {
    match source {
        Source::Preview(p) => p.navigator(doc.bytes(), cache),
        Source::Full(index) => Navigator::with_cache(doc.bytes(), index, cache),
    }
}

impl TreeView {
    /// A tree over `source` with the root expanded (when that is quick).
    pub fn new(doc: Arc<Document>, source: Source, group_size: u64, repaint: &Repaint) -> Self {
        let cache = Arc::new(NavCache::default());
        let model = TreeModel::new(&navigator(&doc, &source, &cache), group_size);
        let mut view = TreeView {
            timings: TreeTimings::default(),
            doc,
            source,
            cache,
            model,
            selected: None,
            scroll: 0.0,
            scroll_to: None,
            ask_continue: None,
            jobs: Vec::new(),
            then: None,
            version: 0,
            generation: 0,
            rows_cache: None,
            source_generation: 0,
            selected_cache: RefCell::new(None),
        };
        if view.model.row_count() > 0 {
            view.selected = Some(0);
            view.expand(0, repaint);
        }
        view.prefetch_root(repaint);
        view
    }

    /// Builds the root's checkpoint table in the background when it is big, so opening a
    /// group deep inside a huge top-level array doesn't wait for it.
    fn prefetch_root(&mut self, repaint: &Repaint) {
        let root = self.nav().root_container();
        if let Some(root) = root
            && !self.nav().is_ready(&root)
        {
            self.request_prepare(root, repaint);
        }
    }

    /// A navigator over the current source.
    pub fn nav(&self) -> Navigator<'_> {
        navigator(&self.doc, &self.source, &self.cache)
    }

    /// What a worker needs to build its own navigator (see [`navigator`]).
    pub fn shared(&self) -> (Arc<Document>, Source, Arc<NavCache>) {
        (
            Arc::clone(&self.doc),
            self.source.clone(),
            Arc::clone(&self.cache),
        )
    }

    /// Bumped whenever the tree switches to another source (preview → full index).
    pub fn source_generation(&self) -> u64 {
        self.source_generation
    }

    /// The document.
    pub fn document(&self) -> &Arc<Document> {
        &self.doc
    }

    /// `true` while browsing the preview.
    pub fn is_preview(&self) -> bool {
        matches!(self.source, Source::Preview(_))
    }

    /// Length of the browsed bytes (the preview prefix or the whole document).
    pub fn browsed_len(&self) -> u64 {
        match &self.source {
            Source::Preview(p) => p.len() as u64,
            Source::Full(_) => self.doc.len(),
        }
    }

    /// Bytes used by cached checkpoint tables.
    pub fn cache_bytes(&self) -> usize {
        self.cache.memory_bytes()
    }

    /// Prepare jobs still running.
    pub fn jobs_running(&self) -> usize {
        self.jobs.len()
    }

    /// Switches to the full index, keeping the expanded rows, the selection and the top
    /// visible row.
    pub fn switch_to_full(&mut self, index: Arc<Index>, repaint: &Repaint) {
        let group = self.model.group_size();
        self.rebuild(Source::Full(index), group, repaint);
    }

    /// Re-groups the rows with another group size, keeping what is expanded.
    pub fn set_group_size(&mut self, group_size: u64, repaint: &Repaint) {
        if group_size != self.model.group_size() {
            let source = self.source.clone();
            self.rebuild(source, group_size, repaint);
        }
    }

    fn rebuild(&mut self, source: Source, group_size: u64, repaint: &Repaint) {
        let expansions = self.model.expansions();
        let selected = self.selected.and_then(|r| self.model.address(r));
        let top = self.model.address(self.scroll.floor() as u64);
        if !matches!((&self.source, &source), (Source::Full(a), Source::Full(b)) if Arc::ptr_eq(a, b))
        {
            self.cache = Arc::new(NavCache::default());
            self.source_generation += 1;
        }
        self.source = source;
        self.jobs.clear(); // dropping a handle cancels its job
        self.then = None;
        self.ask_continue = None;
        let nav = navigator(&self.doc, &self.source, &self.cache);
        let mut model = TreeModel::new(&nav, group_size);
        model.restore(&nav, &expansions);
        let reveal = |model: &mut TreeModel, addr: Option<Vec<u64>>| {
            addr.and_then(|a| model.reveal(&nav, &a).ok())
        };
        let top_row = reveal(&mut model, top);
        self.selected = reveal(&mut model, selected).or((model.row_count() > 0).then_some(0));
        self.model = model;
        self.scroll = top_row.unwrap_or(0) as f64;
        self.scroll_to = self.selected;
        self.changed();
        if self.model.row_count() > 0 && !self.model.is_expanded(0) {
            self.expand(0, repaint);
        }
        self.prefetch_root(repaint);
    }

    /// The model changed.
    fn changed(&mut self) {
        self.version += 1;
        self.invalidate_rows();
    }

    /// Rows may read differently (a container became ready).
    fn invalidate_rows(&mut self) {
        self.generation += 1;
        self.rows_cache = None;
    }

    /// Polls the prepare jobs (every frame).
    pub fn poll(&mut self, repaint: &Repaint) {
        let mut done = Vec::new();
        let mut prepared = None;
        self.jobs.retain_mut(|(start, job)| match job.try_recv() {
            Some(JobEvent::Done(took)) => {
                done.push(*start);
                tracing::debug!("tree: prepared container at {start} in {took:?}");
                prepared = Some(took);
                false
            }
            Some(JobEvent::Failed(e)) => {
                tracing::warn!("tree: preparing a container failed: {e}");
                false
            }
            Some(JobEvent::Cancelled) => false,
            _ => !job.is_finished(),
        });
        if done.is_empty() {
            return;
        }
        self.timings.prepare = prepared.or(self.timings.prepare);
        self.invalidate_rows();
        if let Some((start, version, _)) = &self.then
            && done.contains(start)
        {
            let version = *version;
            let then = self.then.take().map(|(_, _, t)| t);
            if version == self.version {
                match then {
                    Some(Then::Expand(row)) => self.expand(row, repaint),
                    Some(Then::ExpandAll {
                        row,
                        levels,
                        budget,
                    }) => self.expand_all_with(row, levels, budget, repaint),
                    Some(Then::Reveal(address)) => self.reveal_address(address, repaint),
                    None => {}
                }
            }
        }
    }

    /// Starts preparing `c` on a worker (no-op if already running or too many run).
    pub fn request_prepare(&mut self, c: Container, repaint: &Repaint) {
        if self.jobs.iter().any(|(s, _)| *s == c.start) || self.jobs.len() >= MAX_JOBS {
            return;
        }
        let (doc, source, cache) = (
            Arc::clone(&self.doc),
            self.source.clone(),
            Arc::clone(&self.cache),
        );
        let job = JobHandle::spawn("tree-prepare", 0, repaint.clone(), move |ctx| {
            let started = Instant::now();
            navigator(&doc, &source, &cache).prepare(&c, ctx.cancel())?;
            Ok(started.elapsed())
        });
        self.jobs.push((c.start, job));
    }

    /// Rows `range` (cached while nothing changes).
    pub fn rows(&mut self, range: Range<u64>) -> &Rows {
        let fresh =
            matches!(&self.rows_cache, Some((r, g, _)) if *r == range && *g == self.generation);
        if !fresh {
            let rows = self.model.rows(&self.nav(), range.clone());
            self.rows_cache = Some((range, self.generation, rows));
        }
        match &self.rows_cache {
            Some((_, _, rows)) => rows,
            None => unreachable!("filled above"),
        }
    }

    /// One row (not cached).
    pub fn row(&self, row: u64) -> Option<Row> {
        self.model.rows(&self.nav(), row..row + 1).rows.pop()
    }

    /// Expands `row`; starts a prepare job and expands afterwards if needed.
    pub fn expand(&mut self, row: u64, repaint: &Repaint) {
        let nav = navigator(&self.doc, &self.source, &self.cache);
        let started = Instant::now();
        let result = self.model.expand(&nav, row);
        self.timings.expand = Some(started.elapsed());
        match result {
            Expand::Added(n) => {
                self.shift_selection(row, n as i64);
                self.changed();
            }
            Expand::Pending(c) => {
                self.then = Some((c.start, self.version, Then::Expand(row)));
                self.request_prepare(c, repaint);
            }
            Expand::Already | Expand::NotExpandable => {}
        }
    }

    /// Collapses `row`.
    pub fn collapse(&mut self, row: u64) {
        let removed = self.model.collapse(row);
        if removed > 0 {
            if let Some(s) = self.selected
                && s > row
            {
                self.selected = Some(if s <= row + removed { row } else { s - removed });
            }
            self.changed();
        }
    }

    /// Expands or collapses `row`.
    pub fn toggle(&mut self, row: u64, repaint: &Repaint) {
        if self.model.is_expanded(row) {
            self.collapse(row);
        } else {
            self.expand(row, repaint);
        }
    }

    /// Expands `row` and `levels - 1` levels below it, up to [`EXPAND_ALL_BUDGET`] rows;
    /// sets [`ask_continue`](Self::ask_continue) when it stops at the budget.
    pub fn expand_all(&mut self, row: u64, levels: u32, repaint: &Repaint) {
        self.expand_all_with(row, levels, EXPAND_ALL_BUDGET, repaint);
    }

    /// [`expand_all`](Self::expand_all) with another row budget (the debug overlay's
    /// "Expand everything" uses `u64::MAX`; it runs on the UI thread).
    pub fn expand_all_with(&mut self, row: u64, levels: u32, budget: u64, repaint: &Repaint) {
        self.ask_continue = None;
        let nav = navigator(&self.doc, &self.source, &self.cache);
        let before = self.model.row_count();
        // Rows are inserted above and below a selection inside the subtree: keep it by
        // address (its ancestors are expanded already, so revealing expands nothing).
        let selected = self
            .selected
            .filter(|&s| s > row)
            .and_then(|s| self.model.address(s));
        let result = self.model.expand_all(&nav, row, levels, budget);
        if self.model.row_count() != before {
            if let Some(addr) = selected {
                self.selected = self.model.reveal(&nav, &addr).ok().or(Some(row));
            }
            self.changed();
        }
        if let Some(c) = result.pending {
            let then = Then::ExpandAll {
                row,
                levels,
                budget,
            };
            self.then = Some((c.start, self.version, then));
            self.request_prepare(c, repaint);
        } else if !result.complete {
            self.ask_continue = Some(AskContinue {
                row,
                levels,
                added: result.added,
            });
        }
    }

    /// Expands what is needed to show the value at `address` (child indices from the
    /// top-level value), selects it and scrolls to it; waits for prepare jobs on the way.
    pub fn reveal_address(&mut self, address: Vec<u64>, repaint: &Repaint) {
        let nav = navigator(&self.doc, &self.source, &self.cache);
        let before = self.model.row_count();
        let selected = self.selected.and_then(|s| self.model.address(s));
        let result = self.model.reveal(&nav, &address);
        if self.model.row_count() != before {
            // Rows were inserted: find the old selection again (it may have moved).
            self.selected = selected.and_then(|a| self.model.reveal(&nav, &a).ok());
            self.changed();
        }
        match result {
            Ok(row) => self.select(row),
            Err(Reveal::Pending(c)) => {
                self.then = Some((c.start, self.version, Then::Reveal(address)));
                self.request_prepare(c, repaint);
            }
            Err(Reveal::NotFound) => {}
        }
    }

    /// Rows inserted below `row` move a selection that is below it.
    fn shift_selection(&mut self, row: u64, added: i64) {
        if let Some(s) = self.selected
            && s > row
        {
            self.selected = Some(s.saturating_add_signed(added));
        }
    }

    /// Selects `row` (clamped) and scrolls it into view.
    pub fn select(&mut self, row: u64) {
        let count = self.model.row_count();
        if count == 0 {
            self.selected = None;
            return;
        }
        let row = row.min(count - 1);
        self.selected = Some(row);
        self.scroll_to = Some(row);
    }

    /// Key and value spans of the selected value (group rows: their container). Cached
    /// per selected row and model generation: reading a row finds its value's end, which
    /// for a 200 MB string is a 200 MB scan, and callers ask every frame.
    pub fn selected_spans(&self) -> Option<(Option<Span>, Span)> {
        let row = self.selected?;
        if let Some((r, g, spans)) = *self.selected_cache.borrow()
            && r == row
            && g == self.generation
        {
            return spans;
        }
        let spans = match self.row(row)?.item {
            RowItem::Value(v) => Some((v.child.key, v.child.value)),
            RowItem::Group { container, .. } | RowItem::Loading { container, .. } => Some((
                None,
                Span {
                    start: container.start,
                    end: container.end + 1,
                },
            )),
        };
        *self.selected_cache.borrow_mut() = Some((row, self.generation, spans));
        spans
    }
}
