//! Search state of one tab (no egui; `ui::search` and `ui::results` draw it): the running
//! search, the current match, next / previous (on a worker beyond the stored cap), the
//! lazily computed path + line of the result rows on screen, and "Copy all paths".

use std::sync::Arc;
use std::time::{Duration, Instant};

use braceray_core::Document;
use braceray_core::job::{JobEvent, JobHandle, Repaint};
use braceray_core::search::{Match, SearchHandle, SearchNav, SearchOptions, SearchQuery};

use crate::format;

mod copy;
mod info;

pub use copy::{COPY_TO_FILE_ABOVE, CopyOutcome, CopyPaths};
pub use info::{InfoCache, RowInfo};

/// How long a toast ("Wrapped to top") stays.
pub const TOAST: Duration = Duration::from_millis(1800);

/// Text of the "wrapped" toasts.
pub const WRAPPED_TOP: &str = "Wrapped to top";
/// See [`WRAPPED_TOP`].
pub const WRAPPED_BOTTOM: &str = "Wrapped to bottom";

/// The selected match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Current {
    /// The match.
    pub m: Match,
    /// Its position among the stored matches (`None` beyond the stored cap, or while it
    /// isn't published yet).
    pub index: Option<usize>,
}

/// Where next / previous starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    /// After / before this match.
    Match(Match),
    /// From this offset (the start of the selected value, or of the file).
    Offset(u64),
}

/// The result of next / previous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// A match in the asked direction.
    Found(Match),
    /// None ahead; this is the first match.
    WrappedTop(Match),
    /// None behind; this is the last match.
    WrappedBottom(Match),
    /// None found in the part searched so far (the search still runs); retried when
    /// more results arrive.
    NotYet {
        /// The asked direction.
        forward: bool,
    },
    /// No match at all.
    Nothing,
}

/// Next (`forward`) or previous match from `cursor`. Cheap while every match is stored;
/// beyond the cap it may scan, so the caller runs it on a worker then.
pub fn step(nav: &SearchNav, cursor: Cursor, forward: bool, done: bool) -> Step {
    let found = match (cursor, forward) {
        (Cursor::Match(m), true) => nav.next_after(m.offset),
        (Cursor::Match(m), false) => nav.prev_before(m.offset),
        (Cursor::Offset(0), true) => nav.get(0).or_else(|| nav.next_after(0)),
        (Cursor::Offset(o), true) => nav.next_after(o - 1),
        (Cursor::Offset(o), false) => nav.prev_before(o),
    };
    match found {
        Some(m) => Step::Found(m),
        None if !done => Step::NotYet { forward },
        None => {
            let wrapped = if forward {
                nav.get(0).map(Step::WrappedTop)
            } else {
                nav.prev_before(u64::MAX).map(Step::WrappedBottom)
            };
            wrapped.unwrap_or(Step::Nothing)
        }
    }
}

/// Search state of one tab.
pub struct TabSearch {
    handle: Option<SearchHandle>,
    query: SearchQuery,
    /// Why the query can't run (shown instead of the count).
    pub error: Option<String>,
    started: Option<Instant>,
    /// Time from starting the search to the first frame with a stored result.
    pub first_result: Option<Duration>,
    /// Time from the keystroke that led to the search to that frame.
    pub first_result_typed: Option<Duration>,
    typed_at: Option<Instant>,
    /// The selected match.
    pub current: Option<Current>,
    nav_job: Option<(JobHandle<(), Step>, bool)>,
    /// A match to show once the full index is ready (it lies beyond the preview).
    pub pending_jump: Option<Match>,
    /// Path and line of result rows.
    pub info: InfoCache,
    /// Scroll position of the results list in rows.
    pub scroll: f64,
    /// Bring this result row into view on the next frame.
    pub scroll_to: Option<usize>,
    /// The results row the keyboard moved to (Enter jumps to it).
    pub list_selected: Option<usize>,
    /// "Copy all paths".
    pub copy: CopyPaths,
    /// Next (`true`) / previous to retry once more results are published.
    pub retry_step: Option<bool>,
    /// A short message and when it disappears.
    pub toast: Option<(String, Instant)>,
    generation: u64,
}

impl Default for TabSearch {
    fn default() -> Self {
        TabSearch {
            handle: None,
            query: SearchQuery::default(),
            error: None,
            started: None,
            first_result: None,
            first_result_typed: None,
            typed_at: None,
            current: None,
            nav_job: None,
            pending_jump: None,
            info: InfoCache::default(),
            scroll: 0.0,
            scroll_to: None,
            list_selected: None,
            copy: CopyPaths::default(),
            retry_step: None,
            toast: None,
            generation: 0,
        }
    }
}

impl std::fmt::Debug for TabSearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabSearch")
            .field("query", &self.query.text)
            .field("count", &self.count())
            .field("current", &self.current)
            .finish_non_exhaustive()
    }
}

impl TabSearch {
    /// Runs `query` over `doc` unless it already does. Starting a search cancels the
    /// previous one (its handle is dropped). `typed_at` is the keystroke that led to it
    /// (for the debug overlay). Returns `true` if a new search started.
    pub fn ensure(
        &mut self,
        doc: &Arc<Document>,
        query: &SearchQuery,
        max_stored: usize,
        typed_at: Option<Instant>,
        repaint: &Repaint,
    ) -> bool {
        if *query == self.query {
            return false;
        }
        self.typed_at = typed_at;
        self.first_result_typed = None;
        self.query = query.clone();
        self.handle = None;
        self.error = None;
        self.current = None;
        self.nav_job = None;
        self.pending_jump = None;
        self.info.clear();
        self.copy = CopyPaths::default();
        self.retry_step = None;
        self.scroll = 0.0;
        self.scroll_to = None;
        self.list_selected = None;
        self.first_result = None;
        self.started = None;
        self.generation += 1;
        if query.text.is_empty() {
            return true;
        }
        let options = SearchOptions {
            max_stored_results: max_stored,
            ..SearchOptions::default()
        };
        match braceray_core::search::start_search(Arc::clone(doc), query, options, repaint.clone())
        {
            Ok(h) => {
                self.handle = Some(h);
                self.started = Some(Instant::now());
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        true
    }

    /// Forgets the search (the tab's document was reloaded).
    pub fn reset(&mut self) {
        *self = TabSearch {
            generation: self.generation + 1,
            ..TabSearch::default()
        };
    }

    /// The query this tab runs.
    pub fn query(&self) -> &SearchQuery {
        &self.query
    }

    /// Bumped by every new search.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The running (or finished) search.
    pub fn handle(&self) -> Option<&SearchHandle> {
        self.handle.as_ref()
    }

    /// Matches found so far.
    pub fn count(&self) -> u64 {
        self.handle.as_ref().map_or(0, SearchHandle::count)
    }

    /// Stored matches (rows of the results list).
    pub fn stored(&self) -> usize {
        self.handle.as_ref().map_or(0, SearchHandle::stored)
    }

    /// `true` once the whole file was searched.
    pub fn is_done(&self) -> bool {
        self.handle.as_ref().is_none_or(SearchHandle::is_done)
    }

    /// `true` while the search or a next / previous job runs.
    pub fn is_busy(&self) -> bool {
        self.handle.as_ref().is_some_and(|h| !h.is_done()) || self.nav_job.is_some()
    }

    /// Stored match `i`.
    pub fn get(&self, i: usize) -> Option<Match> {
        self.handle.as_ref()?.get(i)
    }

    /// `true` if a stored match starts in `start..end` (tree markers; never scans).
    pub fn has_stored_in(&self, start: u64, end: u64) -> bool {
        let Some(h) = &self.handle else {
            return false;
        };
        let i = h.index_of_offset(start).unwrap_or_else(|i| i);
        h.get(i).is_some_and(|m| m.offset < end)
    }

    /// Matches starting in `start..end` (raw view highlights), at most `limit`.
    pub fn matches_in(&self, start: u64, end: u64, limit: usize) -> Vec<Match> {
        self.handle
            .as_ref()
            .map_or_else(Vec::new, |h| h.matches_in(start, end, limit))
    }

    /// "Searching… 1,204 matches (37 %)", "18,442 matches", "match 312 of 18,442",
    /// "No matches", or the query error; empty without a query.
    pub fn count_text(&self) -> String {
        if let Some(e) = &self.error {
            return e.clone();
        }
        let Some(h) = &self.handle else {
            return String::new();
        };
        let n = h.count();
        if !h.is_done() {
            return format!("Searching… {} ({:.0} %)", matches(n), h.progress() * 100.0);
        }
        match (n, self.current) {
            (0, _) => "No matches".to_owned(),
            (_, Some(Current { index: Some(i), .. })) => format!(
                "match {} of {}",
                format::group_digits(i as u64 + 1),
                format::group_digits(n)
            ),
            (_, Some(Current { index: None, .. })) => format!(
                "match > {} of {}",
                format::group_digits(h.stored() as u64),
                format::group_digits(n)
            ),
            _ => matches(n),
        }
    }

    /// Shows `text` for [`TOAST`].
    pub fn toast(&mut self, text: &str) {
        self.toast = Some((text.to_owned(), Instant::now() + TOAST));
    }

    /// Every frame: timings, the current match's index, finished jobs. Returns a finished
    /// next / previous to apply.
    pub fn poll(&mut self) -> Option<Step> {
        if let (Some(h), Some(started)) = (&self.handle, self.started)
            && self.first_result.is_none()
            && h.stored() > 0
        {
            let took = started.elapsed();
            tracing::info!("search {:?}: first result after {took:?}", self.query.text);
            self.first_result = Some(took);
            self.first_result_typed = self.typed_at.map(|t| t.elapsed());
        }
        if let (Some(h), Some(c)) = (&self.handle, &mut self.current)
            && c.index.is_none()
            && !h.is_capped()
        {
            c.index = h.index_of_offset(c.m.offset).ok();
        }
        self.info.poll();
        self.copy.poll();
        if self
            .toast
            .as_ref()
            .is_some_and(|(_, t)| Instant::now() >= *t)
        {
            self.toast = None;
        }
        let (job, _) = self.nav_job.as_mut()?;
        let event = job.try_recv()?;
        self.nav_job = None;
        match event {
            JobEvent::Done(step) => Some(step),
            _ => None,
        }
    }

    /// Next / previous from `cursor`: answered now while every match is stored,
    /// otherwise on a worker (the answer comes from [`poll`](Self::poll)).
    pub fn start_step(&mut self, cursor: Cursor, forward: bool, repaint: &Repaint) -> Option<Step> {
        let h = self.handle.as_ref()?;
        let nav = h.nav();
        let done = h.is_done();
        if !h.is_capped() {
            return Some(step(&nav, cursor, forward, done));
        }
        let job = JobHandle::spawn("search-step", 0, repaint.clone(), move |_| {
            Ok(step(&nav, cursor, forward, done))
        });
        self.nav_job = Some((job, forward));
        None
    }

    /// `true` while a next / previous job runs.
    pub fn is_stepping(&self) -> bool {
        self.nav_job.is_some()
    }

    /// Makes `m` the current match.
    pub fn set_current(&mut self, m: Match) {
        let index = self
            .handle
            .as_ref()
            .and_then(|h| h.index_of_offset(m.offset).ok());
        self.current = Some(Current { m, index });
        self.scroll_to = index;
        self.list_selected = index;
    }

    /// A navigation handle for workers.
    pub fn nav(&self) -> Option<SearchNav> {
        self.handle.as_ref().map(SearchHandle::nav)
    }
}

fn matches(n: u64) -> String {
    if n == 1 {
        "1 match".to_owned()
    } else {
        format!("{} matches", format::group_digits(n))
    }
}
