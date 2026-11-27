//! Parallel text search over the mapped bytes (architecture.md §4).
//!
//! A search runs on the rayon pool in chunks (8 MiB by default). It gives an exact match
//! count, streams matches in document order (the finished prefix of chunks is published
//! while later chunks still run), and is cancellable. It needs only the bytes, so it
//! works before the index is built; paths for results come from the index later.
//!
//! **Semantics.** The search runs on the raw bytes (a string written with escapes, such as
//! `"café"`, doesn't match `café`). Matches are **non-overlapping, leftmost-first**,
//! like editors: `aa` occurs twice in `aaaaa` (at 0 and 2). Case-insensitive search
//! folds ASCII letters for ASCII queries and uses Unicode simple case folding otherwise
//! (`ЁЛКА` finds `ёлка`), plus the Armenian ligature `և` ≡ `եվ` / `եւ`. Whole word
//! requires that a match whose first (last) byte is a word byte (ASCII letter, digit,
//! `_`, or any byte of a non-ASCII character) has no word byte right before (after) it.

mod chain;
mod engine;
mod run;

use std::sync::Arc;
use std::time::Duration;

use crate::error::Error;
use crate::job::Repaint;

use run::Shared;

/// Where a match may be (step 13 adds `Keys` and `Values`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    /// Anywhere in the file.
    #[default]
    All,
    /// Only inside object keys (step 13).
    Keys,
    /// Only inside values (step 13).
    Values,
}

/// What to search for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    /// The text (UTF-8). An empty text gives no search and no matches.
    pub text: String,
    /// Match case exactly (default: off).
    pub case_sensitive: bool,
    /// Only whole words (default: off).
    pub whole_word: bool,
    /// Treat `text` as a regular expression (step 13; not supported yet).
    pub regex: bool,
    /// Where matches may be (only [`Scope::All`] for now).
    pub scope: Scope,
}

impl SearchQuery {
    /// A plain, case-insensitive query for `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

/// Tuning knobs (the defaults suit the app; tests use tiny chunks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOptions {
    /// Bytes per work unit (default 8 MiB, minimum 1).
    pub chunk_size: usize,
    /// How many matches are kept for [`SearchHandle::get`] (default 5,000,000, ≈ 60 MB).
    /// Beyond it the count stays exact and next / previous scan on demand.
    pub max_stored_results: usize,
    /// Worker tasks on the rayon pool (default: one per pool thread).
    pub threads: Option<usize>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            chunk_size: 8 << 20,
            max_stored_results: 5_000_000,
            threads: None,
        }
    }
}

/// One match: `len` bytes at byte `offset` of the searched content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Match {
    /// Byte offset of the first matched byte.
    pub offset: u64,
    /// Length in bytes (can differ from the query's for case-insensitive Unicode).
    pub len: u32,
}

impl Match {
    /// Offset just past the match.
    pub fn end(&self) -> u64 {
        self.offset + u64::from(self.len)
    }
}

/// Starts a search over `bytes` (usually an `Arc<Document>`) on the rayon pool.
///
/// `repaint` is called whenever more results are published and when the search ends.
/// Dropping the handle cancels the search. Errors: [`Error::SearchUnsupported`] for regex
/// or key/value scope (step 13), [`Error::SearchQuery`] if the query can't be compiled.
///
/// # Example
/// ```
/// use std::sync::Arc;
/// use braceray_core::job::no_repaint;
/// use braceray_core::search::{start_search, Match, SearchOptions, SearchQuery};
///
/// let bytes = Arc::new(br#"{"name": "Needle", "tags": ["needle", "hay"]}"#.to_vec());
/// let search = start_search(bytes, &SearchQuery::new("needle"), SearchOptions::default(),
///                           no_repaint())?;
/// search.wait();
/// assert_eq!(search.count(), 2);
/// assert_eq!(search.get(0), Some(Match { offset: 10, len: 6 }));
/// assert_eq!(search.next_after(10).map(|m| m.offset), Some(29));
/// # Ok::<(), braceray_core::Error>(())
/// ```
pub fn start_search<B>(
    bytes: Arc<B>,
    query: &SearchQuery,
    options: SearchOptions,
    repaint: Repaint,
) -> Result<SearchHandle, Error>
where
    B: AsRef<[u8]> + Send + Sync + 'static,
{
    if query.regex {
        return Err(Error::SearchUnsupported { feature: "Regex" });
    }
    if query.scope != Scope::All {
        return Err(Error::SearchUnsupported {
            feature: "Keys-only and values-only",
        });
    }
    let hay: run::Bytes = bytes;
    let engine = engine::Engine::new(&query.text, query.case_sensitive, query.whole_word)?;
    let Some(engine) = engine else {
        return Ok(SearchHandle { shared: None });
    };
    let shared = Arc::new(Shared::new(
        hay,
        engine,
        options.chunk_size as u64,
        options.max_stored_results,
        repaint,
    ));
    let workers = options.threads.unwrap_or_else(rayon::current_num_threads);
    shared.spawn(workers);
    Ok(SearchHandle {
        shared: Some(shared),
    })
}

/// A running (or finished) search. Every method is cheap and safe to call from the UI
/// thread, except [`next_after`](Self::next_after) / [`prev_before`](Self::prev_before)
/// beyond the stored cap and [`wait`](Self::wait). Dropping it cancels the search.
pub struct SearchHandle {
    /// `None` for an empty query.
    shared: Option<Arc<Shared>>,
}

impl SearchHandle {
    /// Matches found so far; exact once [`is_done`](Self::is_done).
    pub fn count(&self) -> u64 {
        self.shared.as_ref().map_or(0, |s| s.count())
    }

    /// `true` once the whole file was searched (never after a cancel).
    pub fn is_done(&self) -> bool {
        self.shared.as_ref().is_none_or(|s| s.is_done())
    }

    /// `true` while workers are still running.
    pub fn is_running(&self) -> bool {
        self.shared.as_ref().is_some_and(|s| s.is_running())
    }

    /// Searched bytes / total, `0.0..=1.0`.
    pub fn progress(&self) -> f32 {
        self.shared.as_ref().map_or(1.0, |s| s.progress.fraction())
    }

    /// How long the search took, once it ended (done or cancelled).
    pub fn duration(&self) -> Option<Duration> {
        match &self.shared {
            None => Some(Duration::ZERO),
            Some(s) => s.duration(),
        }
    }

    /// Number of matches available through [`get`](Self::get): the document-ordered
    /// prefix published so far, at most `max_stored_results`.
    pub fn stored(&self) -> usize {
        self.shared.as_ref().map_or(0, |s| s.stored())
    }

    /// `true` if some found matches aren't stored (the cap was reached).
    pub fn is_capped(&self) -> bool {
        self.shared.as_ref().is_some_and(|s| s.is_capped())
    }

    /// Stored match `i` (document order).
    pub fn get(&self, i: usize) -> Option<Match> {
        self.shared.as_ref()?.get(i)
    }

    /// Binary search of the stored matches by offset: `Ok(i)` if a stored match starts at
    /// `off`, else `Err(i)` with the index where it would be.
    pub fn index_of_offset(&self, off: u64) -> Result<usize, usize> {
        self.shared
            .as_ref()
            .map_or(Err(0), |s| s.index_of_offset(off))
    }

    /// The first match starting after `off`, among the published part of the file. Beyond
    /// the stored cap this scans forward (up to the end of the file in the worst case),
    /// so call it on a worker when [`is_capped`](Self::is_capped).
    pub fn next_after(&self, off: u64) -> Option<Match> {
        self.shared.as_ref()?.next_after(off)
    }

    /// The last match starting before `off`, among the published part of the file. Beyond
    /// the stored cap this scans the chunks between the last stored match and `off`.
    pub fn prev_before(&self, off: u64) -> Option<Match> {
        self.shared.as_ref()?.prev_before(off)
    }

    /// Asks the workers to stop (each stops within one chunk or 1024 matches).
    pub fn cancel(&self) {
        if let Some(s) = &self.shared {
            s.cancel.cancel();
        }
    }

    /// Blocks until done or, after [`cancel`](Self::cancel), until every worker stopped.
    /// For tests and tools; never call it on the UI thread.
    pub fn wait(&self) {
        if let Some(s) = &self.shared {
            s.wait();
        }
    }
}

impl Drop for SearchHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests;
