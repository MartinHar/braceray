//! Navigation over a document and its index (architecture.md §3.4): root, children,
//! child counts, parents, the value at a byte offset and its path.
//!
//! Children are found by scanning the container's bytes from a **checkpoint**: the
//! offset of every 1024th child, built on first use and cached under a byte budget. Any
//! range of children therefore costs at most ~1024 child parses plus the range itself.
//! Containers that the index doesn't store (adaptive mode) work the same way; their end
//! is found with the SIMD classifier.

mod cache;
mod read;
mod scan;

pub use cache::{DEFAULT_CACHE_BUDGET, NavCache};

use crate::index::{ContainerKind, Index, NO_PARENT, Node};

/// Children between two checkpoints.
pub const CHECKPOINT_EVERY: u64 = 1024;

/// Containers up to this many bytes are scanned on the spot by the `*_if_ready` calls
/// (a few µs); bigger ones need [`Navigator::prepare`] first.
pub const SYNC_SCAN_BYTES: u64 = 64 << 10;

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
    cache: CacheRef<'a>,
}

#[derive(Debug)]
enum CacheRef<'a> {
    Owned(NavCache),
    Shared(&'a NavCache),
}

impl<'a> Navigator<'a> {
    /// A navigator with its own cache (default budget).
    pub fn new(bytes: &'a [u8], index: &'a Index) -> Self {
        Self::with_cache_budget(bytes, index, DEFAULT_CACHE_BUDGET)
    }

    /// A navigator with its own cache of at most about `budget` bytes.
    pub fn with_cache_budget(bytes: &'a [u8], index: &'a Index, budget: usize) -> Self {
        Navigator {
            bytes,
            index,
            cache: CacheRef::Owned(NavCache::new(budget)),
        }
    }

    /// A navigator using a shared `cache` (cheap: borrows only). The cache must belong to
    /// this `bytes` + `index` pair.
    pub fn with_cache(bytes: &'a [u8], index: &'a Index, cache: &'a NavCache) -> Self {
        Navigator {
            bytes,
            index,
            cache: CacheRef::Shared(cache),
        }
    }

    fn cache(&self) -> &NavCache {
        match &self.cache {
            CacheRef::Owned(c) => c,
            CacheRef::Shared(c) => c,
        }
    }

    /// The document bytes.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The index.
    pub fn index(&self) -> &'a Index {
        self.index
    }

    /// The top-level value, if there is one.
    pub fn root(&self) -> Option<Child> {
        let pos = scan::skip_ws(self.bytes, 0, self.bytes.len());
        self.value_at(pos, self.bytes.len(), None, None)
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
            Some(n) => self.node(n).end(),
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
        let node = self.node(n);
        Container {
            kind: node.kind(),
            start: node.start(),
            end: node.end(),
            node: Some(n),
        }
    }

    fn node(&self, n: NodeRef) -> &Node {
        &self.index.nodes()[n.0 as usize]
    }

    /// Parent of a stored node.
    pub fn parent(&self, n: NodeRef) -> Option<NodeRef> {
        let p = self.node(n).parent();
        (p != NO_PARENT).then_some(NodeRef(p))
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
            let start = self.node(child).start();
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
}

fn segment(idx: u64, child: &Child) -> PathSegment {
    match child.key {
        Some(k) => PathSegment::Key(k),
        None => PathSegment::Index(idx),
    }
}
