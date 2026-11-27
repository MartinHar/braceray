//! The tree view's row model: which rows are visible, without materialising them.
//!
//! The visible tree is a flat list of rows. It is stored as a `Vec` of **segments**
//! ("items `a..b` of this item list at depth `d`"), the `RowSpan` variant of the step 07
//! design. An *item list* is either the children of a container or, for containers with
//! more than `group_size` children, a list of **group** items (`[0 … 999]`, …), nested
//! when there are more than `group_size²` children. Expanding item `j` splits its segment
//! after `j` and inserts the child list; collapsing removes the deeper segments again.
//!
//! Costs: finding a row is a binary search over the segments; expanding or collapsing is
//! O(#segments) and never touches the children themselves (a 10 M-child array is one
//! segment of 10,000 group rows); reading rows for the viewport is O(visible rows) plus
//! the navigator's child lookup.
//!
//! Collapsing forgets the expansion state below the collapsed row.

mod restore;
mod rows;

pub mod preview;

use std::ops::Range;

pub use restore::{Expansions, Reveal};
pub use rows::{ExpandAll, Row, RowItem, Rows, ValueRow};

use crate::nav::{Container, Navigator};

/// Default number of children per group row.
pub const DEFAULT_GROUP_SIZE: u64 = 1000;

/// Smallest accepted group size.
pub const MIN_GROUP_SIZE: u64 = 2;

/// Whose children an item list holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    /// The single top-level value.
    Top,
    /// A container's children.
    Node(Container),
}

/// Items of one list: children `base..base + len` of `owner`, `unit` children per item
/// (`unit == 1`: the items are the children; otherwise they are groups).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct List {
    owner: Owner,
    base: u64,
    len: u64,
    unit: u64,
}

impl List {
    fn items(&self) -> u64 {
        self.len.div_ceil(self.unit)
    }

    /// Child indices covered by item `j`.
    fn item_children(&self, j: u64) -> Range<u64> {
        let start = self.base + j * self.unit;
        start..start.saturating_add(self.unit).min(self.base + self.len)
    }
}

/// Items `items` of `list`, shown at `depth`, starting at row `start_row`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    list: List,
    depth: u32,
    items: Range<u64>,
    start_row: u64,
}

impl Segment {
    fn rows(&self) -> u64 {
        self.items.end - self.items.start
    }

    fn end_row(&self) -> u64 {
        self.start_row + self.rows()
    }
}

/// Result of [`TreeModel::expand`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expand {
    /// Expanded; this many rows were added below the row.
    Added(u64),
    /// The row was already expanded.
    Already,
    /// A scalar, an empty container, a loading row or no such row.
    NotExpandable,
    /// The container's children aren't ready: [`Navigator::prepare`] it on a worker, then
    /// try again.
    Pending(Container),
}

/// The visible rows of a tree view (see the module docs).
///
/// # Example
/// ```
/// use braceray_core::index::{build_bytes, IndexOptions};
/// use braceray_core::nav::Navigator;
/// use braceray_core::tree::{Expand, RowItem, TreeModel};
/// use braceray_core::{CancelToken, Progress};
///
/// let bytes = br#"{"a": [1, 2, 3, 4, 5], "b": true}"#;
/// let index = build_bytes(bytes, &IndexOptions::default(), &CancelToken::new(), &Progress::default())?;
/// let nav = Navigator::new(bytes, &index);
/// let mut tree = TreeModel::new(&nav, 2); // groups of 2 to show grouping
/// assert_eq!(tree.row_count(), 1); // the root
/// assert_eq!(tree.expand(&nav, 0), Expand::Added(2)); // "a", "b"
/// assert_eq!(tree.expand(&nav, 1), Expand::Added(2)); // groups of "a": [0 … 3], [4]
/// let rows = tree.rows(&nav, 0..tree.row_count()).rows;
/// assert!(matches!(&rows[2].item, RowItem::Group { range, .. } if *range == (0..4)));
/// assert_eq!(tree.collapse(1), 2);
/// # Ok::<(), braceray_core::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeModel {
    segs: Vec<Segment>,
    group_size: u64,
}

impl TreeModel {
    /// A tree showing only the top-level value (collapsed), or no rows for an empty
    /// document. `group_size` is clamped to at least [`MIN_GROUP_SIZE`].
    pub fn new(nav: &Navigator<'_>, group_size: u64) -> Self {
        let mut segs = Vec::new();
        if nav.root().is_some() {
            segs.push(Segment {
                list: List {
                    owner: Owner::Top,
                    base: 0,
                    len: 1,
                    unit: 1,
                },
                depth: 0,
                items: 0..1,
                start_row: 0,
            });
        }
        TreeModel {
            segs,
            group_size: group_size.max(MIN_GROUP_SIZE),
        }
    }

    /// Children per group row.
    pub fn group_size(&self) -> u64 {
        self.group_size
    }

    /// Number of visible rows.
    pub fn row_count(&self) -> u64 {
        self.segs.last().map_or(0, Segment::end_row)
    }

    /// Number of segments (a measure of how much is expanded).
    pub fn segment_count(&self) -> usize {
        self.segs.len()
    }

    /// Indentation level of `row` (the root is 0).
    pub fn depth(&self, row: u64) -> Option<u32> {
        self.locate(row).map(|(s, _)| self.segs[s].depth)
    }

    /// `true` if `row` is expanded.
    pub fn is_expanded(&self, row: u64) -> bool {
        self.locate(row)
            .is_some_and(|(s, j)| self.expanded_at(s, j))
    }

    /// The row of the parent (container or group) of `row`.
    pub fn parent(&self, row: u64) -> Option<u64> {
        let (s, _) = self.locate(row)?;
        let depth = self.segs[s].depth;
        self.segs[..s]
            .iter()
            .rev()
            .find(|seg| seg.depth < depth)
            .map(|seg| seg.end_row() - 1)
    }

    /// Expands `row` (see [`Expand`]).
    pub fn expand(&mut self, nav: &Navigator<'_>, row: u64) -> Expand {
        let Some((s, j)) = self.locate(row) else {
            return Expand::NotExpandable;
        };
        if self.expanded_at(s, j) {
            return Expand::Already;
        }
        let list = self.segs[s].list;
        let children = if list.unit > 1 {
            let range = list.item_children(j);
            let len = range.end - range.start;
            List {
                owner: list.owner,
                base: range.start,
                len,
                unit: unit_for(len, self.group_size),
            }
        } else {
            let child = match list.owner {
                Owner::Top => nav.root(),
                Owner::Node(c) => {
                    let idx = list.base + j;
                    match nav.children_if_ready(&c, idx..idx + 1) {
                        Some(v) => v.first().copied(),
                        None => return Expand::Pending(c),
                    }
                }
            };
            let Some(inner) = child.and_then(|c| nav.container_of(&c)) else {
                return Expand::NotExpandable;
            };
            match nav.child_count_if_ready(&inner) {
                None => return Expand::Pending(inner),
                Some(0) => return Expand::NotExpandable,
                Some(n) => List {
                    owner: Owner::Node(inner),
                    base: 0,
                    len: n,
                    unit: unit_for(n, self.group_size),
                },
            }
        };
        let added = children.items();
        let seg = &mut self.segs[s];
        let depth = seg.depth;
        let tail = j + 1..seg.items.end;
        seg.items.end = j + 1;
        let mut insert = vec![Segment {
            list: children,
            depth: depth + 1,
            items: 0..added,
            start_row: 0,
        }];
        if !tail.is_empty() {
            insert.push(Segment {
                list,
                depth,
                items: tail,
                start_row: 0,
            });
        }
        self.segs.splice(s + 1..s + 1, insert);
        self.renumber(s + 1);
        Expand::Added(added)
    }

    /// Collapses `row`; returns the number of rows removed (0 if it wasn't expanded).
    pub fn collapse(&mut self, row: u64) -> u64 {
        let Some((s, j)) = self.locate(row) else {
            return 0;
        };
        if !self.expanded_at(s, j) {
            return 0;
        }
        let depth = self.segs[s].depth;
        let end = self.segs[s + 1..]
            .iter()
            .position(|seg| seg.depth <= depth)
            .map_or(self.segs.len(), |p| s + 1 + p);
        let removed: u64 = self.segs[s + 1..end].iter().map(Segment::rows).sum();
        self.segs.drain(s + 1..end);
        if let Some(next) = self.segs.get(s + 1)
            && next.list == self.segs[s].list
            && next.depth == depth
            && next.items.start == self.segs[s].items.end
        {
            let tail_end = next.items.end;
            self.segs[s].items.end = tail_end;
            self.segs.remove(s + 1);
        }
        self.renumber(s + 1);
        removed
    }

    /// Expands a collapsed row or collapses an expanded one.
    pub fn toggle(&mut self, nav: &Navigator<'_>, row: u64) -> Expand {
        if self.is_expanded(row) {
            self.collapse(row);
            Expand::Already
        } else {
            self.expand(nav, row)
        }
    }

    /// Segment and item index of `row`.
    fn locate(&self, row: u64) -> Option<(usize, u64)> {
        let s = self
            .segs
            .partition_point(|seg| seg.start_row <= row)
            .checked_sub(1)?;
        let seg = &self.segs[s];
        (row < seg.end_row()).then(|| (s, seg.items.start + (row - seg.start_row)))
    }

    /// Item `j` of segment `s` is expanded: it ends its segment and a deeper one follows.
    fn expanded_at(&self, s: usize, j: u64) -> bool {
        let seg = &self.segs[s];
        j + 1 == seg.items.end
            && self
                .segs
                .get(s + 1)
                .is_some_and(|next| next.depth > seg.depth)
    }

    fn renumber(&mut self, from: usize) {
        let mut row = match from.checked_sub(1) {
            Some(p) => self.segs[p].end_row(),
            None => 0,
        };
        for seg in &mut self.segs[from..] {
            seg.start_row = row;
            row += seg.rows();
        }
    }
}

/// Children per item for a list of `n` children: 1 up to `g` children, otherwise the
/// smallest power of `g` that leaves at most `g` items.
fn unit_for(n: u64, g: u64) -> u64 {
    if n <= g {
        return 1;
    }
    let mut unit = g;
    while n.div_ceil(unit) > g {
        match unit.checked_mul(g) {
            Some(u) => unit = u,
            None => break,
        }
    }
    unit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units() {
        assert_eq!(unit_for(0, 1000), 1);
        assert_eq!(unit_for(1000, 1000), 1);
        assert_eq!(unit_for(1001, 1000), 1000);
        assert_eq!(unit_for(1_000_000, 1000), 1000);
        assert_eq!(unit_for(1_000_001, 1000), 1_000_000);
        assert_eq!(unit_for(10_000_000, 1000), 1_000_000);
        assert_eq!(unit_for(u64::MAX, 1000), 1_000_000_000_000_000_000);
        assert_eq!(unit_for(u64::MAX, 2), 1 << 63);
    }

    #[test]
    fn list_items() {
        let l = List {
            owner: Owner::Top,
            base: 10,
            len: 25,
            unit: 10,
        };
        assert_eq!(l.items(), 3);
        assert_eq!(l.item_children(0), 10..20);
        assert_eq!(l.item_children(2), 30..35);
    }
}
