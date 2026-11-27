//! [`load_or_build`]: the one call the app makes to get an index.

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{CacheError, IndexCache, StoreOutcome};
use crate::document::Document;
use crate::error::Error;
use crate::index::{Index, IndexOptions, build_index};
use crate::job::{CancelToken, Progress};

/// Where an index came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexSource {
    /// Loaded from the index cache.
    Cache,
    /// Built from the file.
    Built,
}

/// The result of [`load_or_build`].
#[derive(Debug)]
pub struct LoadedIndex {
    /// The index.
    pub index: Arc<Index>,
    /// Cache hit or build.
    pub source: IndexSource,
    /// Time to get the index (cache lookup included).
    pub elapsed: Duration,
    /// The background cache write started after a build, if any. Dropping it detaches the
    /// thread; tests join it.
    pub pending_write: Option<JoinHandle<Result<StoreOutcome, CacheError>>>,
}

impl LoadedIndex {
    /// Status bar text: `"Index: from cache (0.21 s)"` or `"Index: built (3.8 s)"`.
    pub fn status_text(&self) -> String {
        let secs = self.elapsed.as_secs_f64();
        let time = if secs < 1.0 {
            format!("{secs:.2} s")
        } else {
            format!("{secs:.1} s")
        };
        match self.source {
            IndexSource::Cache => format!("Index: from cache ({time})"),
            IndexSource::Built => format!("Index: built ({time})"),
        }
    }
}

/// Loads the index of `doc` from `cache`, or builds it (and writes it to the cache on a
/// background thread). Run it on a worker thread; it may take seconds.
///
/// Errors as [`build_index`]. See [`IndexCache`] for an example.
pub fn load_or_build(
    doc: &Document,
    opts: &IndexOptions,
    cache: Option<&IndexCache>,
    cancel: &CancelToken,
    progress: &Progress,
) -> Result<LoadedIndex, Error> {
    let start = Instant::now();
    if let Some(index) = cache.and_then(|c| c.load(doc, opts, cancel)) {
        return Ok(LoadedIndex {
            index: Arc::new(index),
            source: IndexSource::Cache,
            elapsed: start.elapsed(),
            pending_write: None,
        });
    }
    cancel.check()?;
    let index = Arc::new(build_index(doc, opts, cancel, progress)?);
    let elapsed = start.elapsed();
    let pending_write = cache
        .filter(|c| c.skip_reason(doc.identity().size).is_none())
        .and_then(|c| match c.spawn_store(doc, index.clone(), opts) {
            Ok(handle) => Some(handle),
            Err(e) => {
                tracing::warn!("index cache: can't start the writer thread: {e}");
                None
            }
        });
    Ok(LoadedIndex {
        index,
        source: IndexSource::Built,
        elapsed,
        pending_write,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_text() {
        let mut l = LoadedIndex {
            index: Arc::new(
                crate::index::scalar::build(b"[]", None, &CancelToken::new(), &Progress::default())
                    .unwrap(),
            ),
            source: IndexSource::Cache,
            elapsed: Duration::from_millis(210),
            pending_write: None,
        };
        assert_eq!(l.status_text(), "Index: from cache (0.21 s)");
        l.source = IndexSource::Built;
        l.elapsed = Duration::from_millis(3800);
        assert_eq!(l.status_text(), "Index: built (3.8 s)");
    }
}
