//! Storage of the node table: an owned `Vec` after a build, or a zero-copy view into a
//! memory-mapped index cache file.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use super::node::Node;
use crate::mmap::MappedFile;

/// The node table of an [`Index`](super::Index). Dereferences to `&[Node]`.
#[derive(Clone)]
pub(crate) enum NodeStore {
    /// Built in memory.
    Owned(Vec<Node>),
    /// Mapped from a cache file: `range` of the map holds the `Node`s, checked for size and
    /// alignment when the store was created ([`NodeStore::mapped`]).
    Mapped {
        map: Arc<MappedFile>,
        range: Range<usize>,
    },
}

impl NodeStore {
    /// A view of `map[range]` as nodes, or `None` if the range is out of bounds, not a
    /// multiple of the node size or not 8-byte aligned.
    pub(crate) fn mapped(map: Arc<MappedFile>, range: Range<usize>) -> Option<Self> {
        let bytes = map.bytes().get(range.clone())?;
        bytemuck::try_cast_slice::<u8, Node>(bytes).ok()?;
        Some(NodeStore::Mapped { map, range })
    }

    /// The nodes.
    #[inline]
    pub(crate) fn as_slice(&self) -> &[Node] {
        match self {
            NodeStore::Owned(v) => v,
            // Checked in `mapped`: the same bytes always cast, so the fallback never runs.
            NodeStore::Mapped { map, range } => map
                .bytes()
                .get(range.clone())
                .and_then(|b| bytemuck::try_cast_slice(b).ok())
                .unwrap_or(&[]),
        }
    }

    /// Heap (owned) or mapped bytes used by the table.
    pub(crate) fn memory_bytes(&self) -> usize {
        match self {
            NodeStore::Owned(v) => v.capacity() * std::mem::size_of::<Node>(),
            NodeStore::Mapped { range, .. } => range.len(),
        }
    }

    /// `true` if the nodes come from a mapped cache file.
    pub(crate) fn is_mapped(&self) -> bool {
        matches!(self, NodeStore::Mapped { .. })
    }
}

impl From<Vec<Node>> for NodeStore {
    fn from(v: Vec<Node>) -> Self {
        NodeStore::Owned(v)
    }
}

impl PartialEq for NodeStore {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for NodeStore {}

impl fmt::Debug for NodeStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = if self.is_mapped() { "mapped" } else { "owned" };
        f.debug_struct("NodeStore")
            .field("kind", &kind)
            .field("len", &self.as_slice().len())
            .finish()
    }
}
