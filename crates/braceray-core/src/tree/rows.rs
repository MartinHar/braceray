//! Reading visible rows, and guarded "expand all".

use std::ops::Range;

use super::{Expand, Owner, TreeModel};
use crate::index::ContainerKind;
use crate::nav::{Child, Container, Navigator};

/// Rows fetched per step of [`TreeModel::expand_all`].
const PAGE: u64 = 256;

/// A value row: the top-level value or a child of a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueRow {
    /// Position in the parent (array index or member number); 0 for the top level.
    pub index: u64,
    /// Kind of the parent container (`None` for the top-level value).
    pub parent: Option<ContainerKind>,
    /// Key and value spans and the value type.
    pub child: Child,
    /// The value as a container (objects and arrays).
    pub container: Option<Container>,
    /// Number of children, when known without slow work.
    pub count: Option<u64>,
}

/// What a row shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowItem {
    /// A value.
    Value(ValueRow),
    /// Children `range` of `container`, shown as one row.
    Group {
        /// The container.
        container: Container,
        /// Child indices in this group.
        range: Range<u64>,
    },
    /// Child `index` of `container`, not readable until the container is prepared.
    Loading {
        /// The container.
        container: Container,
        /// Child index.
        index: u64,
    },
}

/// One visible row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Indentation level (the root is 0).
    pub depth: u32,
    /// The row is expanded.
    pub expanded: bool,
    /// What it shows.
    pub item: RowItem,
}

impl Row {
    /// `true` if expanding may add rows: groups, and containers that aren't known to be
    /// empty.
    pub fn expandable(&self) -> bool {
        match &self.item {
            RowItem::Value(v) => v.container.is_some() && v.count != Some(0),
            RowItem::Group { .. } => true,
            RowItem::Loading { .. } => false,
        }
    }
}

/// Result of [`TreeModel::rows`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rows {
    /// The rows, in order.
    pub rows: Vec<Row>,
    /// Containers whose children are needed but not ready (shown as
    /// [`RowItem::Loading`]); prepare them on a worker.
    pub pending: Vec<Container>,
}

/// Result of [`TreeModel::expand_all`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpandAll {
    /// Rows added.
    pub added: u64,
    /// Everything below the row (within the level limit) is expanded.
    pub complete: bool,
    /// Stopped because this container isn't ready.
    pub pending: Option<Container>,
}

impl TreeModel {
    /// Rows `range` (clamped to [`row_count`](Self::row_count)).
    pub fn rows(&self, nav: &Navigator<'_>, range: Range<u64>) -> Rows {
        let mut out = Rows::default();
        let end = range.end.min(self.row_count());
        let Some((mut s, _)) = self.locate(range.start) else {
            return out;
        };
        let mut row = range.start;
        while row < end && s < self.segs.len() {
            let seg = &self.segs[s];
            let first = seg.items.start + (row - seg.start_row);
            let last = seg.items.start + (end.min(seg.end_row()) - seg.start_row);
            let list = seg.list;
            for j in first..last {
                let expanded = j + 1 == seg.items.end
                    && self.segs.get(s + 1).is_some_and(|n| n.depth > seg.depth);
                let item = if list.unit > 1 {
                    let Owner::Node(container) = list.owner else {
                        continue; // the top list never has groups
                    };
                    RowItem::Group {
                        container,
                        range: list.item_children(j),
                    }
                } else {
                    RowItem::Loading {
                        // Placeholder, filled below.
                        container: dummy_container(),
                        index: list.base + j,
                    }
                };
                out.rows.push(Row {
                    depth: seg.depth,
                    expanded,
                    item,
                });
            }
            if list.unit == 1 {
                let at = out.rows.len() - (last - first) as usize;
                self.fill_values(
                    nav,
                    list.owner,
                    list.base + first..list.base + last,
                    at,
                    &mut out,
                );
            }
            row += last - first;
            s += 1;
        }
        out
    }

    /// Replaces the placeholders at `out.rows[at..]` with children `range` of `owner`.
    fn fill_values(
        &self,
        nav: &Navigator<'_>,
        owner: Owner,
        range: Range<u64>,
        at: usize,
        out: &mut Rows,
    ) {
        let (children, parent) = match owner {
            Owner::Top => (nav.root().into_iter().collect::<Vec<_>>(), None),
            Owner::Node(c) => match nav.children_if_ready(&c, range.clone()) {
                Some(v) => (v, Some(c)),
                None => {
                    for row in &mut out.rows[at..] {
                        row.item = RowItem::Loading {
                            container: c,
                            index: match row.item {
                                RowItem::Loading { index, .. } => index,
                                _ => 0,
                            },
                        };
                    }
                    if !out.pending.contains(&c) {
                        out.pending.push(c);
                    }
                    return;
                }
            },
        };
        for (k, row) in out.rows[at..].iter_mut().enumerate() {
            let index = range.start + k as u64;
            row.item = match (children.get(k), parent) {
                (Some(child), _) => {
                    let container = nav.container_of(child);
                    RowItem::Value(ValueRow {
                        index,
                        parent: parent.map(|p| p.kind),
                        child: *child,
                        container,
                        count: container.and_then(|c| nav.child_count_if_ready(&c)),
                    })
                }
                // Fewer children than the count promised (a truncated preview).
                (None, Some(c)) => RowItem::Loading {
                    container: c,
                    index,
                },
                (None, None) => continue,
            };
        }
    }

    /// Expands `row` and everything below it down to `levels` levels (1 = its children
    /// become visible, 2 = their children too, …), adding at most about `budget` rows.
    /// Call again to continue after an incomplete result.
    pub fn expand_all(
        &mut self,
        nav: &Navigator<'_>,
        row: u64,
        levels: u32,
        budget: u64,
    ) -> ExpandAll {
        let mut result = ExpandAll {
            added: 0,
            complete: false,
            pending: None,
        };
        let Some(d0) = self.depth(row) else {
            result.complete = true;
            return result;
        };
        let mut r = row;
        'outer: loop {
            let page = self.rows(nav, r..r.saturating_add(PAGE));
            if page.rows.is_empty() {
                break;
            }
            let len = page.rows.len() as u64;
            for (o, rw) in page.rows.iter().enumerate() {
                let at = r + o as u64;
                if at != row && rw.depth <= d0 {
                    break 'outer;
                }
                if let RowItem::Loading { container, .. } = rw.item {
                    result.pending = Some(container);
                    return result;
                }
                if rw.depth - d0 < levels && rw.expandable() && !rw.expanded {
                    match self.expand(nav, at) {
                        Expand::Added(n) => {
                            result.added += n;
                            if result.added >= budget {
                                return result;
                            }
                            // Rows after `at` moved; continue with its new children.
                            r = at + 1;
                            continue 'outer;
                        }
                        Expand::Pending(c) => {
                            result.pending = Some(c);
                            return result;
                        }
                        Expand::Already | Expand::NotExpandable => {}
                    }
                }
            }
            r += len;
        }
        result.complete = true;
        result
    }
}

fn dummy_container() -> Container {
    Container {
        kind: ContainerKind::Array,
        start: 0,
        end: 0,
        node: None,
    }
}
