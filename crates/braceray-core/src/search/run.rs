//! The running search: workers, the ordered publisher and on-demand navigation beyond
//! the stored cap.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use crate::job::{CancelToken, Progress, Repaint};

use super::Match;
use super::chain::{ChunkScan, Scanner};
use super::engine::Engine;

/// The searched bytes (a [`Document`](crate::Document), or any byte buffer in tests).
pub(crate) type Bytes = Arc<dyn AsRef<[u8]> + Send + Sync>;

/// Head entries reserved from the budget at a time.
const BLOCK: usize = 4096;

/// `matches_in` walks the exact chain from a chunk's chain start only when it is at most
/// this far before the range; otherwise it starts at the range (see `matches_in`).
const EXACT_WALK: u64 = 256 << 10;

/// The search's own thread pool (one thread per core), so a search never queues behind
/// an index build on the global rayon pool. `None` if it can't be created; searches then
/// use the global pool.
pub(crate) fn pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .thread_name(|i| format!("search-{i}"))
            .build()
            .map_err(|e| tracing::warn!("search pool: {e}; using the global pool"))
            .ok()
    })
    .as_ref()
}

/// Worker threads a search uses by default.
pub(crate) fn default_workers() -> usize {
    pool().map_or_else(rayon::current_num_threads, |p| p.current_num_threads())
}

/// Everything workers and the handle share.
pub(crate) struct Shared {
    pub hay: Bytes,
    pub engine: Engine,
    pub chunk_size: u64,
    pub n_chunks: usize,
    pub cap: usize,
    pub cancel: CancelToken,
    pub progress: Progress,
    pub repaint: Repaint,
    pub started: Instant,
    next_chunk: AtomicUsize,
    active: AtomicUsize,
    /// Published count + raw counts of finished, unpublished chunks.
    count: AtomicU64,
    /// Entries unpublished chunk heads may still hold beyond their free part.
    budget: AtomicIsize,
    capped: AtomicBool,
    done: AtomicBool,
    pending: Mutex<Pending>,
    wake: Condvar,
    store: RwLock<Store>,
}

#[derive(Default)]
struct Pending {
    slots: Vec<Option<ChunkScan>>,
    next: usize,
    carry: u64,
    unpublished_count: u64,
    finished_in: Option<Duration>,
}

/// The published, document-ordered prefix of the results.
#[derive(Default)]
pub(crate) struct Store {
    offsets: Vec<u64>,
    lens: Vec<u32>,
    /// Per published chunk: a chain position at or before its first match.
    chain_starts: Vec<u64>,
    /// Matches of the published chunks (stored or not).
    count: u64,
    /// Some published match isn't stored (the cap was reached).
    capped: bool,
}

impl Shared {
    pub(crate) fn new(
        hay: Bytes,
        engine: Engine,
        chunk_size: u64,
        cap: usize,
        repaint: Repaint,
    ) -> Shared {
        let len = (*hay).as_ref().len() as u64;
        let chunk_size = chunk_size.max(1);
        let n_chunks = len.div_ceil(chunk_size) as usize;
        let pending = Pending {
            slots: (0..n_chunks).map(|_| None).collect(),
            ..Pending::default()
        };
        Shared {
            hay,
            engine,
            chunk_size,
            n_chunks,
            cap,
            cancel: CancelToken::new(),
            progress: Progress::new(len),
            repaint,
            started: Instant::now(),
            next_chunk: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            count: AtomicU64::new(0),
            budget: AtomicIsize::new(cap.min(isize::MAX as usize) as isize),
            capped: AtomicBool::new(false),
            done: AtomicBool::new(n_chunks == 0),
            pending: Mutex::new(pending),
            wake: Condvar::new(),
            store: RwLock::new(Store::default()),
        }
    }

    fn bytes(&self) -> &[u8] {
        (*self.hay).as_ref()
    }

    fn scanner(&self) -> Scanner<'_> {
        Scanner {
            engine: &self.engine,
            bytes: self.bytes(),
            cancel: &self.cancel,
        }
    }

    fn bounds(&self, k: usize) -> (u64, u64) {
        let len = self.bytes().len() as u64;
        let s = k as u64 * self.chunk_size;
        (s, (s + self.chunk_size).min(len))
    }

    /// Starts `workers` tasks on the rayon pool.
    pub(crate) fn spawn(self: &Arc<Self>, workers: usize) {
        let workers = workers.clamp(1, self.n_chunks.max(1));
        if self.n_chunks == 0 {
            return;
        }
        self.active.store(workers, Ordering::SeqCst);
        let pool = pool();
        for _ in 0..workers {
            let me = self.clone();
            match pool {
                Some(p) => p.spawn(move || me.work()),
                None => rayon::spawn(move || me.work()),
            }
        }
    }

    fn work(&self) {
        while !self.cancel.is_cancelled() {
            let k = self.next_chunk.fetch_add(1, Ordering::Relaxed);
            if k >= self.n_chunks {
                break;
            }
            let (s, e) = self.bounds(k);
            let grow = || {
                if self.capped.load(Ordering::Relaxed) {
                    return false;
                }
                let block = BLOCK as isize;
                if self.budget.fetch_sub(block, Ordering::Relaxed) >= block {
                    true
                } else {
                    self.budget.fetch_add(block, Ordering::Relaxed);
                    false
                }
            };
            let Ok(scan) = self.scanner().scan_chunk(s, e, BLOCK, grow) else {
                break; // cancelled
            };
            self.progress.add(e - s);
            self.publish(k, scan);
        }
        let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        if self.active.fetch_sub(1, Ordering::SeqCst) == 1 {
            // The last worker: wake waiters (a cancelled search ends here).
            pending.finished_in.get_or_insert(self.started.elapsed());
            self.wake.notify_all();
            drop(pending);
            (self.repaint)();
        }
    }

    /// Files chunk `k` and publishes every chunk that is now next in document order.
    fn publish(&self, k: usize, scan: ChunkScan) {
        let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        pending.unpublished_count += scan.count;
        pending.slots[k] = Some(scan);
        let mut advanced = false;
        loop {
            let k = pending.next;
            let Some(scan) = pending.slots.get_mut(k).and_then(Option::take) else {
                break;
            };
            let (s, e) = self.bounds(k);
            let room = {
                let store = self.store.read().unwrap_or_else(|p| p.into_inner());
                if store.capped {
                    0
                } else {
                    self.cap - store.offsets.len()
                }
            };
            let Ok(st) = self.scanner().stitch(s, e, pending.carry, &scan, room) else {
                // Cancelled while rescanning: leave the rest unpublished.
                pending.slots[k] = Some(scan);
                break;
            };
            self.budget
                .fetch_add(scan.reserved as isize, Ordering::Relaxed);
            pending.unpublished_count -= scan.count;
            pending.carry = st.carry;
            pending.next += 1;
            let mut store = self.store.write().unwrap_or_else(|p| p.into_inner());
            if st.count > st.stored.len() as u64 {
                store.capped = true;
                self.capped.store(true, Ordering::Relaxed);
            }
            store.count += st.count;
            store.chain_starts.push(st.chain_start);
            store.offsets.extend(st.stored.iter().map(|m| m.offset));
            store.lens.extend(st.stored.iter().map(|m| m.len));
            advanced = true;
        }
        let published = self.store.read().map_or(0, |s| s.count);
        self.count
            .store(published + pending.unpublished_count, Ordering::Relaxed);
        if pending.next == self.n_chunks && !self.done.load(Ordering::Relaxed) {
            pending.finished_in = Some(self.started.elapsed());
            self.done.store(true, Ordering::Release);
        }
        self.wake.notify_all();
        drop(pending);
        if advanced {
            (self.repaint)();
        }
    }

    pub(crate) fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    pub(crate) fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    pub(crate) fn is_running(&self) -> bool {
        self.active.load(Ordering::SeqCst) > 0
    }

    pub(crate) fn duration(&self) -> Option<Duration> {
        self.pending.lock().ok().and_then(|p| p.finished_in)
    }

    /// Blocks until the search is done or, after a cancel, all workers have stopped.
    pub(crate) fn wait(&self) {
        let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        while !self.is_done() && self.is_running() {
            pending = self.wake.wait(pending).unwrap_or_else(|p| p.into_inner());
        }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Store> {
        self.store.read().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn stored(&self) -> usize {
        self.read().offsets.len()
    }

    pub(crate) fn is_capped(&self) -> bool {
        self.read().capped
    }

    pub(crate) fn get(&self, i: usize) -> Option<Match> {
        let store = self.read();
        Some(Match {
            offset: *store.offsets.get(i)?,
            len: store.lens[i],
        })
    }

    pub(crate) fn index_of_offset(&self, off: u64) -> Result<usize, usize> {
        self.read().offsets.binary_search(&off)
    }

    /// Chain position of published chunk `k`, if published.
    fn chain_start(&self, k: usize) -> Option<u64> {
        self.read().chain_starts.get(k).copied()
    }

    pub(crate) fn next_after(&self, off: u64) -> Option<Match> {
        {
            let store = self.read();
            let i = store.offsets.partition_point(|&o| o <= off);
            if i < store.offsets.len() {
                return Some(Match {
                    offset: store.offsets[i],
                    len: store.lens[i],
                });
            }
            if !store.capped {
                return None;
            }
        }
        // Beyond the cap: walk the chain chunk by chunk from the chunk holding `off + 1`.
        let mut k = (off.saturating_add(1) / self.chunk_size) as usize;
        while let Some(start) = self.chain_start(k) {
            let (_, e) = self.bounds(k);
            let mut found = None;
            self.scanner()
                .walk(start, e, |m| {
                    if m.offset > off {
                        found = Some(m);
                    }
                    found.is_none()
                })
                .ok()?;
            if found.is_some() {
                return found;
            }
            k += 1;
        }
        None
    }

    pub(crate) fn prev_before(&self, off: u64) -> Option<Match> {
        let (stored_prev, last_stored) = {
            let store = self.read();
            let i = store.offsets.partition_point(|&o| o < off);
            let prev = (i > 0).then(|| Match {
                offset: store.offsets[i - 1],
                len: store.lens[i - 1],
            });
            let last = if store.capped && i == store.offsets.len() {
                Some(store.offsets.last().copied().unwrap_or(0))
            } else {
                None
            };
            (prev, last)
        };
        if let Some(last) = last_stored {
            // Unstored matches may lie between the last stored one and `off`.
            let published = self.read().chain_starts.len();
            let first_k = (last / self.chunk_size) as usize;
            let mut k = ((off.saturating_sub(1) / self.chunk_size) as usize)
                .min(published.saturating_sub(1));
            while let Some(start) = self.chain_start(k) {
                let (_, e) = self.bounds(k);
                let mut best = None;
                self.scanner()
                    .walk(start, e, |m| {
                        if m.offset >= off {
                            return false;
                        }
                        best = Some(m);
                        true
                    })
                    .ok()?;
                if let Some(m) = best.filter(|m| stored_prev.is_none_or(|p| m.offset > p.offset)) {
                    return Some(m);
                }
                if k <= first_k || k == 0 {
                    break;
                }
                k -= 1;
            }
        }
        stored_prev
    }

    pub(crate) fn matches_in(&self, start: u64, end: u64, limit: usize) -> Vec<Match> {
        let mut out = Vec::new();
        let beyond = {
            let store = self.read();
            let i = store.offsets.partition_point(|&o| o < start);
            for j in i..store.offsets.len() {
                if store.offsets[j] >= end || out.len() >= limit {
                    return out;
                }
                out.push(Match {
                    offset: store.offsets[j],
                    len: store.lens[j],
                });
            }
            if !store.capped {
                return out;
            }
            // Unstored matches may follow the last stored one.
            store.offsets.last().map_or(0, |&o| o + 1).max(start)
        };
        if beyond >= end {
            return out;
        }
        let k = (beyond / self.chunk_size) as usize;
        let Some(chain) = self.chain_start(k) else {
            return out; // not published yet
        };
        // The exact chain when its start is near; otherwise start at the range (exact
        // for needles that can't overlap themselves, which is fine for highlighting).
        let back = (self.engine.max_len() as u64).saturating_sub(1);
        let from = if beyond.saturating_sub(chain) <= EXACT_WALK {
            chain.min(beyond)
        } else {
            beyond.saturating_sub(back)
        };
        let _ = self.scanner().walk(from, end, |m| {
            if m.offset >= beyond {
                out.push(m);
            }
            out.len() < limit
        });
        out
    }
}
