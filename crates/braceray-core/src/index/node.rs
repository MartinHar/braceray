//! The packed container record of the node table.

use std::fmt;

/// Largest byte offset a [`Node`] can store (1 TiB − 1).
pub const MAX_OFFSET: u64 = (1 << 40) - 1;

/// Deepest nesting a [`Node`] can store (the root has depth 0).
pub const MAX_DEPTH: u32 = (1 << 24) - 1;

/// [`Node::child_count`] value for "not known" (container not closed, or too many children
/// to fit in a `u32`). Navigation computes it on demand.
pub const UNKNOWN_COUNT: u32 = u32::MAX;

/// [`Node::parent`] value of the root.
pub const NO_PARENT: u32 = u32::MAX;

const KIND_BIT: u64 = 1 << 63;
const OFFSET_MASK: u64 = MAX_OFFSET;
const DEPTH_SHIFT: u32 = 40;

/// Container kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerKind {
    /// `{ … }`
    Object,
    /// `[ … ]`
    Array,
}

/// One container (`{}` or `[]`) of the document, 24 bytes.
///
/// Nodes are stored in document order (pre-order), so a node's descendants follow it
/// directly and node ids grow with `start`. Primitive values are not nodes.
///
/// Packing (see docs/DECISIONS.md, ADR 008): `start` uses bit 63 for the kind; `end` keeps
/// the offset in its low 40 bits and the depth in its high 24 bits.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Node {
    start_kind: u64,
    end_depth: u64,
    parent: u32,
    child_count: u32,
}

const _: () = assert!(std::mem::size_of::<Node>() == 24);

impl Node {
    /// Builds a node. `start`/`end` must be ≤ [`MAX_OFFSET`], `depth` ≤ [`MAX_DEPTH`]
    /// (the indexers check this; larger values are masked).
    pub fn new(
        kind: ContainerKind,
        start: u64,
        end: u64,
        depth: u32,
        parent: u32,
        child_count: u32,
    ) -> Self {
        debug_assert!(start <= MAX_OFFSET && end <= MAX_OFFSET && depth <= MAX_DEPTH);
        let kind_bit = match kind {
            ContainerKind::Object => 0,
            ContainerKind::Array => KIND_BIT,
        };
        Node {
            start_kind: (start & OFFSET_MASK) | kind_bit,
            end_depth: (end & OFFSET_MASK) | (u64::from(depth & MAX_DEPTH) << DEPTH_SHIFT),
            parent,
            child_count,
        }
    }

    /// Offset of the opening `{` or `[`.
    #[inline]
    pub fn start(&self) -> u64 {
        self.start_kind & OFFSET_MASK
    }

    /// Offset of the matching `}` or `]`. For a container that isn't closed (invalid
    /// document), the offset of the error.
    #[inline]
    pub fn end(&self) -> u64 {
        self.end_depth & OFFSET_MASK
    }

    /// Object or array.
    #[inline]
    pub fn kind(&self) -> ContainerKind {
        if self.start_kind & KIND_BIT != 0 {
            ContainerKind::Array
        } else {
            ContainerKind::Object
        }
    }

    /// Nesting depth; the root is 0.
    #[inline]
    pub fn depth(&self) -> u32 {
        (self.end_depth >> DEPTH_SHIFT) as u32
    }

    /// Id of the parent node, or [`NO_PARENT`].
    #[inline]
    pub fn parent(&self) -> u32 {
        self.parent
    }

    /// Number of direct children (values of any type), or [`UNKNOWN_COUNT`].
    #[inline]
    pub fn child_count(&self) -> u32 {
        self.child_count
    }

    #[inline]
    pub(crate) fn set_end(&mut self, end: u64) {
        self.end_depth = (self.end_depth & !OFFSET_MASK) | (end & OFFSET_MASK);
    }

    #[inline]
    pub(crate) fn set_child_count(&mut self, count: u32) {
        self.child_count = count;
    }

    #[inline]
    pub(crate) fn with_parent(mut self, parent: u32) -> Self {
        self.parent = parent;
        self
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Node")
            .field("kind", &self.kind())
            .field("start", &self.start())
            .field("end", &self.end())
            .field("depth", &self.depth())
            .field("parent", &self.parent())
            .field("child_count", &self.child_count())
            .finish()
    }
}

/// Saturating conversion of a child count to the stored `u32`.
#[inline]
pub(crate) fn stored_count(count: u64) -> u32 {
    u32::try_from(count).unwrap_or(UNKNOWN_COUNT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut n = Node::new(ContainerKind::Array, MAX_OFFSET, 5, MAX_DEPTH, 7, 3);
        assert_eq!(n.kind(), ContainerKind::Array);
        assert_eq!(n.start(), MAX_OFFSET);
        assert_eq!(n.end(), 5);
        assert_eq!(n.depth(), MAX_DEPTH);
        assert_eq!(n.parent(), 7);
        assert_eq!(n.child_count(), 3);
        n.set_end(MAX_OFFSET);
        n.set_child_count(9);
        assert_eq!(
            (n.end(), n.depth(), n.child_count()),
            (MAX_OFFSET, MAX_DEPTH, 9)
        );
        let o = Node::new(ContainerKind::Object, 0, 1, 0, NO_PARENT, 0);
        assert_eq!(o.kind(), ContainerKind::Object);
        assert_eq!((o.start(), o.end(), o.depth()), (0, 1, 0));
    }

    #[test]
    fn counts_saturate() {
        assert_eq!(stored_count(3), 3);
        assert_eq!(stored_count(u64::from(u32::MAX)), UNKNOWN_COUNT);
        assert_eq!(stored_count(u64::MAX), UNKNOWN_COUNT);
    }
}
