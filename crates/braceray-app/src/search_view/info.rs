//! Path and line of the result rows on screen, computed on a worker (finding a match's
//! path may build a big container's checkpoint table) and kept in a small LRU cache.

use std::collections::HashMap;

use braceray_core::job::{JobEvent, JobHandle, Repaint};
use braceray_core::path::jsonpath;

use crate::tree_view::{TreeView, navigator};

/// Rows kept; the least recently used half is dropped when it overflows.
const CAPACITY: usize = 4096;

/// Path and line of one match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowInfo {
    /// JSONPath of the innermost value holding the match.
    pub path: String,
    /// 1-based line of the match.
    pub line: u64,
}

type InfoJob = JobHandle<(), Vec<(u64, RowInfo)>>;

/// LRU cache of [`RowInfo`] by match offset, plus the job filling it.
#[derive(Default)]
pub struct InfoCache {
    map: HashMap<u64, (RowInfo, u64)>,
    tick: u64,
    job: Option<InfoJob>,
    /// Source generation of the tree the cached paths come from.
    source: u64,
}

impl std::fmt::Debug for InfoCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InfoCache")
            .field("rows", &self.map.len())
            .field("busy", &self.job.is_some())
            .finish()
    }
}

impl InfoCache {
    /// Forgets everything (a new search); a running job is cancelled.
    pub fn clear(&mut self) {
        self.map.clear();
        self.job = None;
    }

    /// Cached info for the match at `offset`.
    pub fn get(&mut self, offset: u64) -> Option<&RowInfo> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(&offset).map(|(info, used)| {
            *used = tick;
            &*info
        })
    }

    /// Number of cached rows.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// `true` when nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// `true` while a job computes rows.
    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }

    /// Starts a job for the `offsets` not cached yet, unless one runs. Needs the full
    /// index (paths and lines over the preview would be wrong past its end).
    pub fn request(&mut self, offsets: &[u64], tree: &TreeView, repaint: &Repaint) {
        if tree.is_preview() {
            return;
        }
        if self.source != tree.source_generation() {
            self.source = tree.source_generation();
            self.clear();
        }
        if self.job.is_some() {
            return;
        }
        let missing: Vec<u64> = offsets
            .iter()
            .copied()
            .filter(|o| !self.map.contains_key(o))
            .collect();
        if missing.is_empty() {
            return;
        }
        let (doc, source, cache) = tree.shared();
        self.job = Some(JobHandle::spawn(
            "result-paths",
            0,
            repaint.clone(),
            move |ctx| {
                let nav = navigator(&doc, &source, &cache);
                let mut out = Vec::with_capacity(missing.len());
                for off in missing {
                    ctx.cancel().check()?;
                    let path = nav.path_of(&nav.node_at_offset(off));
                    let line = nav.index().lines().line_of(nav.bytes(), off);
                    out.push((
                        off,
                        RowInfo {
                            path: jsonpath(nav.bytes(), &path),
                            line,
                        },
                    ));
                }
                Ok(out)
            },
        ));
    }

    /// Stores a finished job's rows.
    pub fn poll(&mut self) {
        let Some(event) = self.job.as_mut().and_then(JobHandle::try_recv) else {
            return;
        };
        self.job = None;
        if let JobEvent::Done(rows) = event {
            for (off, info) in rows {
                self.tick += 1;
                self.map.insert(off, (info, self.tick));
            }
            if self.map.len() > CAPACITY {
                let mut ticks: Vec<u64> = self.map.values().map(|(_, t)| *t).collect();
                ticks.sort_unstable();
                let keep_from = ticks[ticks.len() - CAPACITY / 2];
                self.map.retain(|_, (_, t)| *t >= keep_from);
            }
        }
    }
}
