//! Navigation over a document and its index (architecture.md §3.4): root, children,
//! child counts, parents, the value at a byte offset and its path.
//!
//! Children are found by scanning the container's bytes from a **checkpoint**: the
//! offset of every 1024th child, built on first use and cached under a byte budget. Any
//! range of children therefore costs at most ~1024 child parses plus the range itself.
//! Containers that the index doesn't store (adaptive mode) work the same way; their end
//! is found with the SIMD classifier.

mod scan;

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use crate::index::{ContainerKind, Index, NO_PARENT, UNKNOWN_COUNT};

/// Children between two checkpoints.
pub const CHECKPOINT_EVERY: u64 = 1024;

/// Default byte budget of the checkpoint cache.
pub const DEFAULT_CACHE_BUDGET: usize = 256 << 20;

/// Id of a stored container (index into [`Index::nodes`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeRef(pub u32);

/// Type of a JSON value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueKind {
    /// `{ … }`
    Object,
    /// `[ … ]`
    Array,
    /// `"…"`
    String,
    /// A number.
    Number,
    /// `true` / `false`
    Bool,
    /// `null`
    Null,
}

/// Half-open byte range `start..end` in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    /// First byte.
    pub start: u64,
    /// One past the last byte.
    pub end: u64,
}

impl Span {
    fn new(start: usize, end: usize) -> Self {
        Span {
            start: start as u64,
            end: end as u64,
        }
    }

    /// `true` if `offset` lies in `start..end`.
    pub fn contains(&self, offset: u64) -> bool {
        self.start <= offset && offset < self.end
    }

    /// The bytes of this span.
    pub fn slice<'b>(&self, bytes: &'b [u8]) -> &'b [u8] {
        let end = (self.end as usize).min(bytes.len());
        &bytes[(self.start as usize).min(end)..end]
    }
}

/// One value inside a container (or the top-level value).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Child {
    /// The key, including its quotes (objects only).
    pub key: Option<Span>,
    /// The value.
    pub value: Span,
    /// Its type.
    pub kind: ValueKind,
}

/// A container value: stored in the index (`node`) or not (adaptive mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Container {
    /// Object or array.
    pub kind: ContainerKind,
    /// Offset of `{` / `[`.
    pub start: u64,
    /// Offset of the matching `}` / `]` (the error offset if it isn't closed).
    pub end: u64,
    /// The index node, if stored.
    pub node: Option<NodeRef>,
}

/// One step of a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathSegment {
    /// Object member; the span is the raw key including quotes.
    Key(Span),
    /// Array element.
    Index(u64),
}

/// Where a byte offset is: the innermost stored container, then the steps below it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// Innermost stored container whose range contains the offset (`None`: the offset is
    /// in a top-level scalar or outside the root value).
    pub node: Option<NodeRef>,
    /// Children walked from `node` down to the innermost value containing the offset:
    /// `(index in its container, child)`.
    pub steps: Vec<(u64, Child)>,
}

#[derive(Debug)]
struct Checkpoints {
    /// Number of children found.
    count: u64,
    /// Offset of child `k × CHECKPOINT_EVERY` (before its leading whitespace is skipped).
    starts: Vec<usize>,
}

#[derive(Debug, Default)]
struct Cache {
    map: HashMap<u64, (Arc<Checkpoints>, u64)>,
    bytes: usize,
    clock: u64,
}

/// Navigation over `bytes` (the document content) and its `index`.
///
/// # Example
/// ```
/// use braceray_core::index::{build_bytes, IndexOptions};
/// use braceray_core::nav::{Navigator, PathSegment};
/// use braceray_core::{CancelToken, Progress};
///
/// let bytes = br#"{"users": [{"id": 1}, {"id": 2}]}"#;
/// let index = build_bytes(bytes, &IndexOptions::default(), &CancelToken::new(), &Progress::default())?;
/// let nav = Navigator::new(bytes, &index);
///
/// let root = nav.root_container().unwrap();
/// let users = nav.children(&root, 0..1)[0];
/// let users = nav.container_of(&users).unwrap();
/// assert_eq!(nav.child_count(&users), 2);
///
/// let offset = bytes.windows(1).rposition(|w| w == b"2").unwrap() as u64;
/// let path = nav.path_of(&nav.node_at_offset(offset));
/// assert!(matches!(path[..], [PathSegment::Key(_), PathSegment::Index(1), PathSegment::Key(_)]));
/// # Ok::<(), braceray_core::Error>(())
/// ```
#[derive(Debug)]
pub struct Navigator<'a> {
    bytes: &'a [u8],
    index: &'a Index,
    cache: Mutex<Cache>,
    budget: usize,
}

impl<'a> Navigator<'a> {
    /// A navigator with the default cache budget.
    pub fn new(bytes: &'a [u8], index: &'a Index) -> Self {
        Self::with_cache_budget(bytes, index, DEFAULT_CACHE_BUDGET)
    }

    /// A navigator whose checkpoint cache uses at most about `budget` bytes.
    pub fn with_cache_budget(bytes: &'a [u8], index: &'a Index, budget: usize) -> Self {
        Navigator {
            bytes,
            index,
            cache: Mutex::new(Cache::default()),
            budget,
        }
    }

    /// The top-level value, if there is one.
    pub fn root(&self) -> Option<Child> {
        let pos = scan::skip_ws(self.bytes, 0, self.bytes.len());
        self.value_at(pos, self.bytes.len(), None)
    }

    /// The top-level container (`None` for a top-level scalar or an empty document).
    pub fn root_container(&self) -> Option<Container> {
        self.root().and_then(|c| self.container_of(&c))
    }

    /// The container behind a child, if the child is an object or array.
    pub fn container_of(&self, child: &Child) -> Option<Container> {
        let kind = match child.kind {
            ValueKind::Object => ContainerKind::Object,
            ValueKind::Array => ContainerKind::Array,
            _ => return None,
        };
        let node = self.node_starting_at(child.value.start);
        let end = match node {
            Some(n) => self.index.nodes()[n.0 as usize].end(),
            None => child.value.end.saturating_sub(1),
        };
        Some(Container {
            kind,
            start: child.value.start,
            end,
            node,
        })
    }

    /// The container of a stored node.
    pub fn container(&self, n: NodeRef) -> Container {
        let node = &self.index.nodes()[n.0 as usize];
        Container {
            kind: node.kind(),
            start: node.start(),
            end: node.end(),
            node: Some(n),
        }
    }

    /// Parent of a stored node.
    pub fn parent(&self, n: NodeRef) -> Option<NodeRef> {
        let p = self.index.nodes()[n.0 as usize].parent();
        (p != NO_PARENT).then_some(NodeRef(p))
    }

    /// Number of direct children.
    pub fn child_count(&self, c: &Container) -> u64 {
        if let Some(n) = c.node {
            let count = self.index.nodes()[n.0 as usize].child_count();
            if count != UNKNOWN_COUNT {
                return u64::from(count);
            }
        }
        self.checkpoints(c).count
    }

    /// Children `range` of `c` (clamped to the child count).
    pub fn children(&self, c: &Container, range: Range<u64>) -> Vec<Child> {
        let cps = self.checkpoints(c);
        let end = range.end.min(cps.count);
        if range.start >= end {
            return Vec::new();
        }
        let k = (range.start / CHECKPOINT_EVERY) as usize;
        let mut pos = cps.starts[k];
        let mut idx = k as u64 * CHECKPOINT_EVERY;
        let mut out = Vec::with_capacity((end - range.start) as usize);
        while idx < end {
            let Some((child, next)) = self.child_at(c, pos) else {
                break;
            };
            if idx >= range.start {
                out.push(child);
            }
            idx += 1;
            pos = next;
        }
        out
    }

    /// The innermost value containing `offset` (see [`Location`]).
    pub fn node_at_offset(&self, offset: u64) -> Location {
        let node = self.innermost_node(offset);
        let mut steps = Vec::new();
        let mut current = node.map(|n| self.container(n));
        while let Some(c) = current.take() {
            let Some((idx, child)) = self.child_containing(&c, offset) else {
                break;
            };
            steps.push((idx, child));
            if child.value.contains(offset) {
                current = self.container_of(&child);
            }
        }
        Location { node, steps }
    }

    /// Path from the root to `loc`.
    pub fn path_of(&self, loc: &Location) -> Vec<PathSegment> {
        let mut chain = Vec::new();
        let mut n = loc.node;
        while let Some(id) = n {
            chain.push(id);
            n = self.parent(id);
        }
        let mut path = Vec::with_capacity(chain.len() + loc.steps.len());
        for pair in chain.windows(2).rev() {
            let (child, parent) = (pair[0], pair[1]);
            let start = self.index.nodes()[child.0 as usize].start();
            if let Some((idx, c)) = self.child_containing(&self.container(parent), start) {
                path.push(segment(idx, &c));
            }
        }
        path.extend(loc.steps.iter().map(|(idx, c)| segment(*idx, c)));
        path
    }

    /// 1-based `(line, column)` of `offset`; the column counts characters.
    pub fn line_col(&self, offset: u64) -> (u64, u64) {
        self.index.lines().line_col(self.bytes, offset)
    }

    /// Stored node whose container starts exactly at `offset`.
    pub fn node_starting_at(&self, offset: u64) -> Option<NodeRef> {
        let nodes = self.index.nodes();
        let i = nodes.partition_point(|n| n.start() < offset);
        (nodes.get(i)?.start() == offset).then_some(NodeRef(i as u32))
    }

    /// Innermost stored node with `start ≤ offset ≤ end`.
    pub fn innermost_node(&self, offset: u64) -> Option<NodeRef> {
        let nodes = self.index.nodes();
        let i = nodes
            .partition_point(|n| n.start() <= offset)
            .checked_sub(1)?;
        let mut id = i as u32;
        loop {
            let n = &nodes[id as usize];
            if offset <= n.end() {
                return Some(NodeRef(id));
            }
            if n.parent() == NO_PARENT {
                return None;
            }
            id = n.parent();
        }
    }

    /// Child of `c` whose key or value contains `offset`, with its index.
    fn child_containing(&self, c: &Container, offset: u64) -> Option<(u64, Child)> {
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
        while idx < cps.count {
            let (child, next) = self.child_at(c, pos)?;
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

    fn checkpoints(&self, c: &Container) -> Arc<Checkpoints> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.clock += 1;
        let clock = cache.clock;
        if let Some((cps, used)) = cache.map.get_mut(&c.start) {
            *used = clock;
            return Arc::clone(cps);
        }
        drop(cache); // build without holding the lock
        let cps = Arc::new(self.build_checkpoints(c));
        let size = cps.starts.capacity() * std::mem::size_of::<usize>() + 64;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        while cache.bytes + size > self.budget && !cache.map.is_empty() {
            let oldest = cache
                .map
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| *k);
            if let Some(key) = oldest
                && let Some((old, _)) = cache.map.remove(&key)
            {
                cache.bytes -= old.starts.capacity() * std::mem::size_of::<usize>() + 64;
            }
        }
        cache.bytes += size;
        cache.map.insert(c.start, (Arc::clone(&cps), clock));
        cps
    }

    fn build_checkpoints(&self, c: &Container) -> Checkpoints {
        let mut starts = Vec::new();
        let mut count = 0u64;
        let mut pos = c.start as usize + 1;
        loop {
            if count.is_multiple_of(CHECKPOINT_EVERY) {
                starts.push(pos);
            }
            let Some((_, next)) = self.child_at(c, pos) else {
                break;
            };
            count += 1;
            pos = next;
        }
        starts.shrink_to_fit();
        Checkpoints { count, starts }
    }

    /// Parses the child starting at or after `pos` inside `c`; returns it and the offset
    /// after its trailing comma (or the container end).
    fn child_at(&self, c: &Container, pos: usize) -> Option<(Child, usize)> {
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
        let child = self.value_at(vpos, end, key)?;
        let after = scan::skip_ws(bytes, child.value.end as usize, end);
        let next = if after < end && bytes[after] == b',' {
            after + 1
        } else {
            end
        };
        Some((child, next))
    }

    /// The value starting at `pos` (bounded by `end`).
    fn value_at(&self, pos: usize, end: usize, key: Option<Span>) -> Option<Child> {
        let bytes = self.bytes;
        let first = *bytes.get(pos).filter(|_| pos < end)?;
        let (kind, value_end) = match first {
            b'{' | b'[' => {
                let kind = if first == b'{' {
                    ValueKind::Object
                } else {
                    ValueKind::Array
                };
                let stop = match self.node_starting_at(pos as u64) {
                    Some(n) => {
                        let node_end = self.index.nodes()[n.0 as usize].end() as usize;
                        (node_end + 1).min(end)
                    }
                    None => scan::container_end(bytes, pos, end).unwrap_or(end),
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

fn segment(idx: u64, child: &Child) -> PathSegment {
    match child.key {
        Some(k) => PathSegment::Key(k),
        None => PathSegment::Index(idx),
    }
}
