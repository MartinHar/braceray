//! Reading the children of a container: blocking calls, non-blocking `*_if_ready` calls
//! for the UI thread, and [`Navigator::prepare`] for workers.

use std::ops::Range;
use std::sync::Arc;

use super::cache::Checkpoints;
use super::{
    CHECKPOINT_EVERY, Child, Container, Navigator, SYNC_SCAN_BYTES, Span, ValueKind, scan,
};
use crate::error::Error;
use crate::index::{ContainerKind, Node, UNKNOWN_COUNT};
use crate::job::CancelToken;

/// How many children are parsed between two cancel checks.
const CANCEL_EVERY: u64 = 1 << 16;

impl Navigator<'_> {
    /// Number of direct children. Builds the checkpoint table when the index doesn't know
    /// the count (may scan the whole container).
    pub fn child_count(&self, c: &Container) -> u64 {
        match self.stored_count(c) {
            Some(n) => n,
            None => self.checkpoints(c).count,
        }
    }

    /// Number of direct children without slow work: known to the index, checkpoints
    /// cached, or a container of at most [`SYNC_SCAN_BYTES`]. `None`: call
    /// [`prepare`](Self::prepare) on a worker first.
    pub fn child_count_if_ready(&self, c: &Container) -> Option<u64> {
        self.stored_count(c)
            .or_else(|| self.ready_checkpoints(c).map(|cps| cps.count))
    }

    /// `true` when any range of children can be read without slow work.
    pub fn is_ready(&self, c: &Container) -> bool {
        self.ready_checkpoints(c).is_some()
    }

    /// Children `range` of `c` (clamped to the child count). Builds the checkpoint table
    /// first when the range starts beyond the first [`CHECKPOINT_EVERY`] children.
    pub fn children(&self, c: &Container, range: Range<u64>) -> Vec<Child> {
        let cps = if self.reachable_without_table(c, range.start) {
            self.cache().get(c.start)
        } else {
            Some(self.checkpoints(c))
        };
        self.read_children(c, cps.as_deref(), range, None)
            .unwrap_or_default()
    }

    /// Like [`children`](Self::children), but `None` instead of building a big checkpoint
    /// table or scanning a child container the index doesn't store that is bigger than
    /// [`SYNC_SCAN_BYTES`] (a 100 MB nested array in adaptive mode). Ranges within the
    /// first [`CHECKPOINT_EVERY`] children, ranges near the last read position and
    /// containers of at most [`SYNC_SCAN_BYTES`] are ready unless they contain such a
    /// child; [`prepare`](Self::prepare) records those and makes them ready.
    pub fn children_if_ready(&self, c: &Container, range: Range<u64>) -> Option<Vec<Child>> {
        let cps = if self.reachable_without_table(c, range.start) {
            self.cache().get(c.start)
        } else {
            Some(self.ready_checkpoints(c)?)
        };
        self.read_children(c, cps.as_deref(), range, Some(SYNC_SCAN_BYTES))
    }

    /// Builds and caches the checkpoint table of `c` (for workers). After this,
    /// [`children_if_ready`](Self::children_if_ready) and
    /// [`child_count_if_ready`](Self::child_count_if_ready) answer at once while the table
    /// stays in the cache.
    ///
    /// # Errors
    /// [`Error::Cancelled`] when `cancel` fires (checked every 65,536 children).
    pub fn prepare(&self, c: &Container, cancel: &CancelToken) -> Result<(), Error> {
        // A table is built with every big child end recorded, so once it is cached the
        // `*_if_ready` reads of `c` never need more than their scan budget.
        if self.cache().get(c.start).is_none() {
            let cps = Arc::new(self.build_checkpoints(c, Some(cancel))?);
            self.cache().insert(c.start, cps);
        }
        Ok(())
    }

    /// Offset of the latest cached checkpoint (a child start, before its whitespace) at
    /// or before `offset`, if `c`'s checkpoint table is cached. Never builds one.
    pub(crate) fn checkpoint_before_if_ready(&self, c: &Container, offset: u64) -> Option<u64> {
        let cps = self.cache().get(c.start)?;
        let k = cps
            .starts
            .partition_point(|&s| s as u64 <= offset)
            .checked_sub(1)?;
        Some(cps.starts[k] as u64)
    }

    fn stored_count(&self, c: &Container) -> Option<u64> {
        let n = c.node?;
        let count = self.node(n).child_count();
        (count != UNKNOWN_COUNT).then_some(u64::from(count))
    }

    fn reachable_without_table(&self, c: &Container, from: u64) -> bool {
        from < CHECKPOINT_EVERY
            || self
                .cache()
                .cursor(c.start)
                .is_some_and(|(idx, _)| idx <= from && from - idx < CHECKPOINT_EVERY)
    }

    /// Cached table, or a freshly built one for a small container.
    fn ready_checkpoints(&self, c: &Container) -> Option<Arc<Checkpoints>> {
        if let Some(cps) = self.cache().get(c.start) {
            return Some(cps);
        }
        (c.end.saturating_sub(c.start) <= SYNC_SCAN_BYTES).then(|| self.checkpoints(c))
    }

    pub(super) fn checkpoints(&self, c: &Container) -> Arc<Checkpoints> {
        if let Some(cps) = self.cache().get(c.start) {
            return cps;
        }
        let built = match self.build_checkpoints(c, None) {
            Ok(cps) => cps,
            // Without a cancel token the build can't fail.
            Err(_) => Checkpoints {
                count: 0,
                starts: vec![c.start as usize + 1],
                big_ends: Vec::new(),
            },
        };
        let cps = Arc::new(built);
        self.cache().insert(c.start, Arc::clone(&cps));
        cps
    }

    fn build_checkpoints(
        &self,
        c: &Container,
        cancel: Option<&CancelToken>,
    ) -> Result<Checkpoints, Error> {
        let mut starts = Vec::new();
        let mut count = 0u64;
        let mut pos = c.start as usize + 1;
        let mut hint = self.first_node_after(pos);
        let mut ends = scan::Ends::unlimited(&[]);
        loop {
            if count.is_multiple_of(CHECKPOINT_EVERY) {
                starts.push(pos);
            }
            if count.is_multiple_of(CANCEL_EVERY)
                && let Some(token) = cancel
            {
                token.check()?;
            }
            let Some((_, next)) = self.child_at(c, pos, Some(&mut hint), &mut ends) else {
                break;
            };
            count += 1;
            pos = next;
        }
        starts.shrink_to_fit();
        let mut big_ends = ends.found;
        big_ends.shrink_to_fit();
        Ok(Checkpoints {
            count,
            starts,
            big_ends,
        })
    }

    /// Reads children `range` starting from the best known position: the cursor, the
    /// checkpoint before `range.start`, or the container start. With a `cap`, `None` when
    /// an unindexed child container longer than that has no recorded end.
    fn read_children(
        &self,
        c: &Container,
        cps: Option<&Checkpoints>,
        range: Range<u64>,
        cap: Option<u64>,
    ) -> Option<Vec<Child>> {
        let end = cps.map_or(range.end, |t| range.end.min(t.count));
        if range.start >= end {
            return Some(Vec::new());
        }
        let known = cps.map_or(&[][..], |t| &t.big_ends[..]);
        let mut ends = match cap {
            Some(b) => scan::Ends::capped(known, b),
            None => scan::Ends::unlimited(known),
        };
        let (mut idx, mut pos) = (0u64, c.start as usize + 1);
        if let Some(t) = cps {
            let k = ((range.start / CHECKPOINT_EVERY) as usize).min(t.starts.len() - 1);
            (idx, pos) = (k as u64 * CHECKPOINT_EVERY, t.starts[k]);
        }
        if let Some((i, p)) = self.cache().cursor(c.start)
            && i <= range.start
            && i > idx
        {
            (idx, pos) = (i, p);
        }
        let mut hint = self.first_node_after(pos);
        let mut out = Vec::with_capacity((end - range.start).min(4096) as usize);
        while idx < end {
            let Some((child, next)) = self.child_at(c, pos, Some(&mut hint), &mut ends) else {
                break;
            };
            if idx >= range.start {
                out.push(child);
            }
            idx += 1;
            pos = next;
        }
        if ends.exceeded {
            return None;
        }
        if idx > range.start {
            self.cache().set_cursor(c.start, idx, pos);
        }
        Some(out)
    }

    /// Child of `c` whose key or value contains `offset`, with its index.
    pub(super) fn child_containing(&self, c: &Container, offset: u64) -> Option<(u64, Child)> {
        if offset <= c.start || offset >= c.end {
            return None;
        }
        let cps = self.checkpoints(c);
        let k = cps
            .starts
            .partition_point(|&s| s as u64 <= offset)
            .checked_sub(1)?;
        let mut pos = cps.starts[k];
        let mut idx = k as u64 * CHECKPOINT_EVERY;
        let mut hint = self.first_node_after(pos);
        let mut ends = scan::Ends::unlimited(&cps.big_ends);
        while idx < cps.count {
            let (child, next) = self.child_at(c, pos, Some(&mut hint), &mut ends)?;
            let first = child.key.map_or(child.value.start, |k| k.start);
            if offset < first {
                return None; // between children
            }
            if offset < child.value.end {
                return Some((idx, child));
            }
            idx += 1;
            pos = next;
        }
        None
    }

    /// Index of the first stored node starting at or after `pos`.
    fn first_node_after(&self, pos: usize) -> usize {
        self.index
            .nodes()
            .partition_point(|n| n.start() < pos as u64)
    }

    /// Parses the child starting at or after `pos` inside `c`; returns it and the offset
    /// after its trailing comma (or the container end). `hint`: see [`Self::value_at`].
    fn child_at(
        &self,
        c: &Container,
        pos: usize,
        hint: Option<&mut usize>,
        ends: &mut scan::Ends<'_>,
    ) -> Option<(Child, usize)> {
        let bytes = self.bytes;
        let end = (c.end as usize).min(bytes.len());
        let p = scan::skip_ws(bytes, pos, end);
        if p >= end {
            return None;
        }
        let (key, vpos) = match c.kind {
            ContainerKind::Object => {
                if bytes[p] != b'"' {
                    return None;
                }
                let key_end = scan::string_end(bytes, p, end)?;
                let colon = scan::skip_ws(bytes, key_end, end);
                if colon >= end || bytes[colon] != b':' {
                    return None;
                }
                (
                    Some(Span::new(p, key_end)),
                    scan::skip_ws(bytes, colon + 1, end),
                )
            }
            ContainerKind::Array => (None, p),
        };
        let child = self.value_at(vpos, end, key, hint, ends)?;
        let after = scan::skip_ws(bytes, child.value.end as usize, end);
        let next = if after < end && bytes[after] == b',' {
            after + 1
        } else {
            end
        };
        Some((child, next))
    }

    /// The value starting at `pos` (bounded by `end`).
    ///
    /// `hint` is a node index with no stored node in `hint_start..pos` for an increasing
    /// sequence of calls; it turns the node lookup of a container value into a short
    /// galloping search instead of a binary search over the whole table.
    pub(super) fn value_at(
        &self,
        pos: usize,
        end: usize,
        key: Option<Span>,
        hint: Option<&mut usize>,
        ends: &mut scan::Ends<'_>,
    ) -> Option<Child> {
        let bytes = self.bytes;
        let first = *bytes.get(pos).filter(|_| pos < end)?;
        let (kind, value_end) = match first {
            b'{' | b'[' => {
                let kind = if first == b'{' {
                    ValueKind::Object
                } else {
                    ValueKind::Array
                };
                let nodes = self.index.nodes();
                let i = match hint {
                    Some(h) => {
                        *h = gallop(nodes, *h, pos as u64);
                        *h
                    }
                    None => nodes.partition_point(|n| n.start() < pos as u64),
                };
                let stop = match nodes.get(i).filter(|n| n.start() == pos as u64) {
                    Some(n) => (n.end() as usize + 1).min(end),
                    None => ends.container_end(bytes, pos, end)?,
                };
                (kind, stop)
            }
            b'"' => (ValueKind::String, scan::string_end(bytes, pos, end)?),
            b't' | b'f' => (ValueKind::Bool, scan::scalar_end(bytes, pos, end)),
            b'n' => (ValueKind::Null, scan::scalar_end(bytes, pos, end)),
            _ => (ValueKind::Number, scan::scalar_end(bytes, pos, end)),
        };
        Some(Child {
            key,
            value: Span::new(pos, value_end),
            kind,
        })
    }
}

/// First `i ≥ from` with `nodes[i].start() ≥ offset`, assuming every node before `from`
/// starts before `offset`.
fn gallop(nodes: &[Node], from: usize, offset: u64) -> usize {
    let n = nodes.len();
    if from >= n || nodes[from].start() >= offset {
        return from;
    }
    // nodes[lo] < offset; widen until nodes[hi] ≥ offset (or the end).
    let (mut lo, mut step) = (from, 1usize);
    let mut hi = from + 1;
    while hi < n && nodes[hi].start() < offset {
        lo = hi;
        step = step.saturating_mul(2);
        hi = lo.saturating_add(step);
    }
    let hi = hi.min(n);
    lo + 1 + nodes[lo + 1..hi].partition_point(|x| x.start() < offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{IndexOptions, build_bytes};
    use crate::job::Progress;

    fn index(bytes: &[u8]) -> crate::Index {
        build_bytes(
            bytes,
            &IndexOptions::default(),
            &CancelToken::new(),
            &Progress::default(),
        )
        .unwrap()
    }

    #[test]
    fn gallop_matches_partition_point() {
        let mut bytes = b"[".to_vec();
        for i in 0..3000 {
            bytes.extend_from_slice(if i % 3 == 0 { b"[[1]]," } else { b"7," });
        }
        bytes.extend_from_slice(b"0]");
        let idx = index(&bytes);
        let nodes = idx.nodes();
        for offset in (0..bytes.len() as u64).step_by(7) {
            let want = nodes.partition_point(|n| n.start() < offset);
            for from in [0, want / 2, want.saturating_sub(1), want] {
                assert_eq!(
                    gallop(nodes, from, offset),
                    want,
                    "offset {offset} from {from}"
                );
            }
        }
    }

    #[test]
    fn ready_calls_never_build_big_tables() {
        let mut bytes = b"[".to_vec();
        for i in 0..50_000u32 {
            bytes.extend_from_slice(format!("{i},").as_bytes());
        }
        bytes.extend_from_slice(b"0]");
        let idx = index(&bytes);
        let nav = Navigator::new(&bytes, &idx);
        let root = nav.root_container().unwrap();
        assert_eq!(nav.child_count_if_ready(&root), Some(50_001)); // stored count
        assert!(!nav.is_ready(&root));
        assert_eq!(nav.children_if_ready(&root, 0..3).unwrap().len(), 3);
        assert!(nav.children_if_ready(&root, 40_000..40_001).is_none());
        // Near the cursor left by the last read: ready without a table.
        let near = nav.children_if_ready(&root, 1000..1010).unwrap();
        assert_eq!(near[0].value.slice(&bytes), b"1000");
        let near = nav.children_if_ready(&root, 1500..1501).unwrap();
        assert_eq!(near[0].value.slice(&bytes), b"1500");
        nav.prepare(&root, &CancelToken::new()).unwrap();
        assert!(nav.is_ready(&root));
        let far = nav.children_if_ready(&root, 40_000..40_001).unwrap();
        assert_eq!(far[0].value.slice(&bytes), b"40000");
    }

    #[test]
    fn prepare_is_cancellable() {
        let bytes = format!("[{}0]", "1,".repeat(200_000)).into_bytes();
        let idx = index(&bytes);
        let nav = Navigator::new(&bytes, &idx);
        let root = nav.root_container().unwrap();
        let token = CancelToken::new();
        token.cancel();
        assert!(matches!(nav.prepare(&root, &token), Err(Error::Cancelled)));
        assert!(!nav.is_ready(&root));
    }

    #[test]
    fn small_unstored_containers_are_ready() {
        // Adaptive mode with depth 0: only the root is stored.
        let bytes = br#"[{"a": [1, 2, {"b": 3}]}, {"c": 4}]"#;
        let opts = IndexOptions {
            memory_budget: 1,
            ..IndexOptions::default()
        };
        let idx = build_bytes(bytes, &opts, &CancelToken::new(), &Progress::default()).unwrap();
        let nav = Navigator::new(bytes, &idx);
        let root = nav.root_container().unwrap();
        let first = nav.children_if_ready(&root, 0..1).unwrap()[0];
        let inner = nav.container_of(&first).unwrap();
        assert!(inner.node.is_none() || idx.max_stored_depth().is_none());
        assert_eq!(nav.child_count_if_ready(&inner), Some(1));
    }

    /// A child container bigger than `SYNC_SCAN_BYTES` that the index doesn't store is not
    /// scanned on the UI thread: `children_if_ready` says "not ready" until `prepare` has
    /// recorded its end, then reads without scanning it (I-009).
    #[test]
    fn big_unstored_children_wait_for_prepare() {
        let mut text = String::from("[{\"small\": [1, 2]}, [");
        for i in 0..200_000 {
            if i > 0 {
                text.push(',');
            }
            text.push_str(&(i % 10).to_string());
        }
        text.push_str("], {\"after\": true}]");
        let bytes = text.as_bytes();
        let opts = IndexOptions {
            memory_budget: 1,
            ..IndexOptions::default()
        };
        let idx = build_bytes(bytes, &opts, &CancelToken::new(), &Progress::default()).unwrap();
        let nav = Navigator::new(bytes, &idx);
        let root = nav.root_container().unwrap();
        let want = nav.children(&root, 0..3);
        assert_eq!(want.len(), 3);
        assert!((want[1].value.end - want[1].value.start) > SYNC_SCAN_BYTES);

        let fresh = Navigator::new(bytes, &idx);
        assert_eq!(
            fresh.children_if_ready(&root, 0..1).map(|v| v.len()),
            Some(1)
        );
        assert_eq!(
            fresh.children_if_ready(&root, 0..3),
            None,
            "big child: not ready"
        );
        fresh.prepare(&root, &CancelToken::new()).unwrap();
        assert_eq!(fresh.children_if_ready(&root, 0..3), Some(want.clone()));
        assert_eq!(
            fresh.children_if_ready(&root, 2..3),
            Some(want[2..].to_vec())
        );
        // Blocking reads give the same with or without the recorded ends.
        assert_eq!(fresh.children(&root, 0..3), want);
    }
}
