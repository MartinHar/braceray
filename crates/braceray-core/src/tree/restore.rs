//! Addresses (child-index paths), revealing a value, and carrying the expansion state over
//! to a new model (preview → full index, another group size).

use std::ops::Range;

use super::{Expand, List, Owner, TreeModel};
use crate::nav::{Container, Navigator};

/// Why [`TreeModel::reveal`] couldn't reach a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reveal {
    /// This container must be prepared first (see [`Expand::Pending`]).
    Pending(Container),
    /// The address doesn't exist in this document.
    NotFound,
}

/// One expanded row in pre-order: its parent entry and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Root,
    Child(u64),
    Group(Range<u64>),
}

/// The expansion state of a [`TreeModel`], independent of row numbers and group layout
/// (see [`TreeModel::expansions`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expansions {
    entries: Vec<(Option<usize>, Step)>,
}

impl Expansions {
    /// Number of expanded rows recorded.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if nothing was expanded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl TreeModel {
    /// Child-index path from the top-level value to `row`'s value (group rows: to their
    /// container). The root's address is empty.
    pub fn address(&self, row: u64) -> Option<Vec<u64>> {
        let mut out = Vec::new();
        let mut r = row;
        loop {
            let (s, j) = self.locate(r)?;
            let list = self.segs[s].list;
            if matches!(list.owner, Owner::Node(_)) && list.unit == 1 {
                out.push(list.base + j);
            }
            match self.parent(r) {
                Some(p) => r = p,
                None => break,
            }
        }
        out.reverse();
        Some(out)
    }

    /// Rows of the value rows from the top-level value down to `row` (included when it is
    /// a value row; group rows are skipped). One pass over the segments.
    ///
    /// # Example
    /// ```
    /// use braceray_core::index::{build_bytes, IndexOptions};
    /// use braceray_core::nav::Navigator;
    /// use braceray_core::tree::TreeModel;
    /// use braceray_core::{CancelToken, Progress};
    ///
    /// let bytes = br#"{"a": [10, 20, 30]}"#;
    /// let index = build_bytes(bytes, &IndexOptions::default(), &CancelToken::new(), &Progress::default())?;
    /// let nav = Navigator::new(bytes, &index);
    /// let mut tree = TreeModel::new(&nav, 2);
    /// tree.expand(&nav, 0); // root → "a"
    /// tree.expand(&nav, 1); // "a" → groups [0 … 1], [2]
    /// tree.expand(&nav, 3); // [2] → 30
    /// assert_eq!(tree.ancestors(4), vec![0, 1, 4]); // root, "a", 30 (group row 3 skipped)
    /// assert_eq!(tree.ancestors(3), vec![0, 1]);
    /// # Ok::<(), braceray_core::Error>(())
    /// ```
    pub fn ancestors(&self, row: u64) -> Vec<u64> {
        let Some((s, _)) = self.locate(row) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut depth = self.segs[s].depth;
        if self.segs[s].list.unit == 1 {
            out.push(row);
        }
        for seg in self.segs[..s].iter().rev() {
            if seg.depth < depth {
                depth = seg.depth;
                if seg.list.unit == 1 {
                    out.push(seg.end_row() - 1);
                }
                if depth == 0 {
                    break;
                }
            }
        }
        out.reverse();
        out
    }

    /// Expands what is needed to show the value at `address` and returns its row.
    pub fn reveal(&mut self, nav: &Navigator<'_>, address: &[u64]) -> Result<u64, Reveal> {
        if self.segs.is_empty() {
            return Err(Reveal::NotFound);
        }
        let mut row = 0;
        for &k in address {
            row = self.descend_to_child(nav, row, k)?;
        }
        Ok(row)
    }

    /// From an expandable `row`, expands it and any groups on the way to child `k` of its
    /// value; returns the child's row.
    fn descend_to_child(
        &mut self,
        nav: &Navigator<'_>,
        mut row: u64,
        k: u64,
    ) -> Result<u64, Reveal> {
        loop {
            let list = self.open(nav, row)?;
            if k < list.base || k >= list.base + list.len {
                return Err(Reveal::NotFound);
            }
            row = self.child_row(row, (k - list.base) / list.unit)?;
            if list.unit == 1 {
                return Ok(row);
            }
        }
    }

    /// Expands `row` if needed; returns the item list of its children.
    fn open(&mut self, nav: &Navigator<'_>, row: u64) -> Result<List, Reveal> {
        match self.expand(nav, row) {
            Expand::Added(_) | Expand::Already => {}
            Expand::Pending(c) => return Err(Reveal::Pending(c)),
            Expand::NotExpandable => return Err(Reveal::NotFound),
        }
        let (s, _) = self.locate(row).ok_or(Reveal::NotFound)?;
        Ok(self.segs[s + 1].list)
    }

    /// Row of item `j` of the children of expanded `row`.
    fn child_row(&self, row: u64, j: u64) -> Result<u64, Reveal> {
        let (s, _) = self.locate(row).ok_or(Reveal::NotFound)?;
        let depth = self.segs[s].depth + 1;
        for seg in &self.segs[s + 1..] {
            if seg.depth < depth {
                break;
            }
            if seg.depth == depth && seg.items.contains(&j) {
                return Ok(seg.start_row + (j - seg.items.start));
            }
        }
        Err(Reveal::NotFound)
    }

    /// The expansion state, in one pass over the segments.
    pub fn expansions(&self) -> Expansions {
        let mut entries = Vec::new();
        // owners[d] = entry whose children are shown at depth d + 1.
        let mut owners: Vec<usize> = Vec::new();
        for (s, seg) in self.segs.iter().enumerate() {
            owners.truncate(seg.depth as usize);
            let Some(next) = self.segs.get(s + 1) else {
                break;
            };
            if next.depth <= seg.depth {
                continue;
            }
            let j = seg.items.end - 1;
            let step = match (seg.list.owner, seg.list.unit) {
                (Owner::Top, _) => Step::Root,
                (_, 1) => Step::Child(seg.list.base + j),
                _ => Step::Group(seg.list.item_children(j)),
            };
            let parent = seg
                .depth
                .checked_sub(1)
                .and_then(|d| owners.get(d as usize).copied());
            entries.push((parent, step));
            owners.push(entries.len() - 1);
        }
        Expansions { entries }
    }

    /// Re-applies `exp` (from [`expansions`](Self::expansions) of a model over the same or
    /// a longer prefix of the document, with any group size). Returns how many value
    /// expansions couldn't be applied (not found, not ready); everything below such an
    /// entry is skipped too. A group that doesn't exist in this layout is dropped, and the
    /// entries below it are applied from its container.
    pub fn restore(&mut self, nav: &Navigator<'_>, exp: &Expansions) -> usize {
        // Row of each applied entry. Rows only grow below the latest entry, so the rows
        // of earlier entries (all above it, in pre-order) stay valid.
        let mut rows: Vec<Option<u64>> = Vec::with_capacity(exp.entries.len());
        let mut skipped = 0;
        for (parent, step) in &exp.entries {
            let from = parent.and_then(|p| rows[p]);
            let row = match (parent, step) {
                (None, Step::Root) => (!self.segs.is_empty()).then_some(0),
                (Some(_), Step::Child(k)) => {
                    from.and_then(|r| self.descend_to_child(nav, r, *k).ok())
                }
                (Some(_), Step::Group(range)) => {
                    // Missing group: stand in with the container row.
                    rows.push(from.map(|r| self.descend_to_group(nav, r, range).unwrap_or(r)));
                    if let Some(&Some(r)) = rows.last() {
                        self.expand(nav, r);
                    }
                    continue;
                }
                _ => None,
            };
            let row =
                row.filter(|&r| matches!(self.expand(nav, r), Expand::Added(_) | Expand::Already));
            if row.is_none() {
                skipped += 1;
            }
            rows.push(row);
        }
        skipped
    }

    /// Row of the group with exactly `range` below expanded `row`, if this model has it.
    fn descend_to_group(
        &mut self,
        nav: &Navigator<'_>,
        mut row: u64,
        range: &Range<u64>,
    ) -> Option<u64> {
        loop {
            let list = self.open(nav, row).ok()?;
            if list.unit == 1 || range.start < list.base || range.start >= list.base + list.len {
                return None;
            }
            let j = (range.start - list.base) / list.unit;
            row = self.child_row(row, j).ok()?;
            let here = list.item_children(j);
            if here == *range {
                return Some(row);
            }
            if here.end < range.end {
                return None;
            }
        }
    }
}
