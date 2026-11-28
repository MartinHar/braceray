//! The navigation cache: checkpoint tables of containers (byte-budgeted LRU) and the last
//! child position read in each container (so sequential reads don't re-parse from the
//! checkpoint). Shared between the UI thread and workers through [`NavCache`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// Default byte budget of the checkpoint cache.
pub const DEFAULT_CACHE_BUDGET: usize = 256 << 20;

/// Containers whose last read position is remembered (cleared when full).
const MAX_CURSORS: usize = 4096;

/// Child offsets of one container.
#[derive(Debug)]
pub(crate) struct Checkpoints {
    /// Number of children found.
    pub(crate) count: u64,
    /// Offset of child `k × CHECKPOINT_EVERY` (before its leading whitespace is skipped).
    pub(crate) starts: Vec<usize>,
    /// `(start, end)` of the children that are containers the index doesn't store and
    /// span more than [`SYNC_SCAN_BYTES`](super::SYNC_SCAN_BYTES), in order: reads on the
    /// UI thread take their ends from here instead of scanning them (I-009).
    pub(crate) big_ends: Vec<(usize, usize)>,
}

impl Checkpoints {
    fn cost(&self) -> usize {
        self.starts.capacity() * std::mem::size_of::<usize>()
            + self.big_ends.capacity() * std::mem::size_of::<(usize, usize)>()
            + 64
    }
}

#[derive(Debug, Default)]
struct Inner {
    map: HashMap<u64, (Arc<Checkpoints>, u64)>,
    bytes: usize,
    clock: u64,
    /// Container start → (child index, offset where that child starts).
    cursors: HashMap<u64, (u64, usize)>,
}

/// Checkpoint and cursor cache of one document, safe to share between threads.
///
/// A [`Navigator`](super::Navigator) owns one by default; the app keeps one per open file
/// in an `Arc` and builds cheap borrowing navigators from it, so a worker can prepare a
/// big container's checkpoints while the UI thread reads the ones that are ready.
///
/// # Example
/// ```
/// use braceray_core::index::{build_bytes, IndexOptions};
/// use braceray_core::nav::{NavCache, Navigator};
/// use braceray_core::{CancelToken, Progress};
///
/// let bytes = b"[1, 2, 3]";
/// let index = build_bytes(bytes, &IndexOptions::default(), &CancelToken::new(), &Progress::default())?;
/// let cache = NavCache::default();
/// let root = Navigator::with_cache(bytes, &index, &cache).root_container().unwrap();
/// // Another navigator over the same cache sees the prepared table.
/// Navigator::with_cache(bytes, &index, &cache).prepare(&root, &CancelToken::new())?;
/// assert_eq!(Navigator::with_cache(bytes, &index, &cache).child_count_if_ready(&root), Some(3));
/// # Ok::<(), braceray_core::Error>(())
/// ```
#[derive(Debug)]
pub struct NavCache {
    inner: Mutex<Inner>,
    budget: usize,
}

impl Default for NavCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_BUDGET)
    }
}

impl NavCache {
    /// A cache whose checkpoint tables use at most about `budget` bytes.
    pub fn new(budget: usize) -> Self {
        NavCache {
            inner: Mutex::new(Inner::default()),
            budget,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while holding the lock leaves plain data behind; keep using it.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The table of the container starting at `start`, if cached (marks it used).
    pub(crate) fn get(&self, start: u64) -> Option<Arc<Checkpoints>> {
        let mut inner = self.lock();
        inner.clock += 1;
        let clock = inner.clock;
        let (cps, used) = inner.map.get_mut(&start)?;
        *used = clock;
        Some(Arc::clone(cps))
    }

    /// Stores a table, evicting the least recently used ones over the budget.
    pub(crate) fn insert(&self, start: u64, cps: Arc<Checkpoints>) {
        let size = cps.cost();
        let mut inner = self.lock();
        if let Some((old, _)) = inner.map.remove(&start) {
            inner.bytes -= old.cost();
        }
        while inner.bytes + size > self.budget && !inner.map.is_empty() {
            let oldest = inner
                .map
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| *k);
            if let Some(key) = oldest
                && let Some((old, _)) = inner.map.remove(&key)
            {
                inner.bytes -= old.cost();
            }
        }
        inner.clock += 1;
        let clock = inner.clock;
        inner.bytes += size;
        inner.map.insert(start, (cps, clock));
    }

    /// Last remembered `(child index, offset)` in the container starting at `start`.
    pub(crate) fn cursor(&self, start: u64) -> Option<(u64, usize)> {
        self.lock().cursors.get(&start).copied()
    }

    /// Remembers that child `idx` of the container at `start` begins at `pos`.
    pub(crate) fn set_cursor(&self, start: u64, idx: u64, pos: usize) {
        let mut inner = self.lock();
        if inner.cursors.len() >= MAX_CURSORS && !inner.cursors.contains_key(&start) {
            inner.cursors.clear();
        }
        inner.cursors.insert(start, (idx, pos));
    }

    /// Bytes used by cached checkpoint tables.
    pub fn memory_bytes(&self) -> usize {
        self.lock().bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(n: usize) -> Arc<Checkpoints> {
        Arc::new(Checkpoints {
            count: n as u64,
            starts: vec![0; n],
            big_ends: Vec::new(),
        })
    }

    #[test]
    fn evicts_least_recently_used() {
        let one = table(100).cost();
        let cache = NavCache::new(one * 2);
        cache.insert(1, table(100));
        cache.insert(2, table(100));
        assert!(cache.get(1).is_some()); // 2 is now the oldest
        cache.insert(3, table(100));
        assert!(cache.get(2).is_none());
        assert!(cache.get(1).is_some() && cache.get(3).is_some());
        assert_eq!(cache.memory_bytes(), one * 2);
        cache.insert(3, table(100)); // replacing doesn't double count
        assert_eq!(cache.memory_bytes(), one * 2);
    }

    #[test]
    fn cursors_are_bounded() {
        let cache = NavCache::default();
        for i in 0..MAX_CURSORS as u64 + 10 {
            cache.set_cursor(i, i, i as usize);
        }
        assert!(cache.lock().cursors.len() <= MAX_CURSORS);
        assert_eq!(cache.cursor(MAX_CURSORS as u64 + 9), Some((4105, 4105)));
    }
}
