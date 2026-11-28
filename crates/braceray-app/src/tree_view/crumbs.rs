//! The breadcrumb chain of the selection: the value rows from the root down to it, their
//! labels, the selection's path and what the copy actions need.

use std::rc::Rc;

use braceray_core::index::ContainerKind;
use braceray_core::nav::{Child, Container, PathSegment, ValueKind};
use braceray_core::path::decode_key;
use braceray_core::tree::{RowItem, ValueRow};

use super::TreeView;

/// Keys longer than this many characters are cut in breadcrumb labels.
pub const LABEL_CHARS: usize = 40;

/// Siblings listed in a breadcrumb dropdown.
pub const SIBLINGS: u64 = 200;

/// What the selection is, for the copy actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selected {
    /// The value (group rows: their container).
    pub child: Child,
    /// It is a group row (no key of its own).
    pub group: bool,
}

/// One entry of a sibling dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sibling {
    /// Child index in the parent.
    pub index: u64,
    /// `key` or `[i]`.
    pub label: String,
}

impl TreeView {
    /// Rows of the breadcrumb chain of the selection (see
    /// [`TreeModel::ancestors`](braceray_core::tree::TreeModel::ancestors)), cached per
    /// selection and model generation.
    pub fn crumb_rows(&self) -> Rc<[u64]> {
        let Some(row) = self.selected else {
            return Rc::from([]);
        };
        if let Some((r, g, rows)) = &*self.crumbs_cache.borrow()
            && *r == row
            && *g == self.generation
        {
            return Rc::clone(rows);
        }
        let rows: Rc<[u64]> = self.model.ancestors(row).into();
        *self.crumbs_cache.borrow_mut() = Some((row, self.generation, Rc::clone(&rows)));
        rows
    }

    /// The value row at `row`, if it is one.
    pub fn value_row(&self, row: u64) -> Option<ValueRow> {
        match self.row(row)?.item {
            RowItem::Value(v) => Some(v),
            _ => None,
        }
    }

    /// Breadcrumb label of a value row: `$` for the root, `[i]` for array elements, the
    /// decoded key (cut at [`LABEL_CHARS`]) for members.
    pub fn crumb_label(&self, v: &ValueRow) -> String {
        match (v.parent, v.child.key) {
            (None, _) => "$".to_owned(),
            (Some(ContainerKind::Object), Some(k)) => {
                cut(&decode_key(k.slice(self.document().bytes())))
            }
            _ => format!("[{}]", v.index),
        }
    }

    /// Path of the selection from the root (empty for the root itself). Reads every
    /// level: O(depth).
    pub fn selected_path(&self) -> Option<Vec<PathSegment>> {
        super::deep::path_of(&self.model, &self.nav(), self.selected?)
    }

    /// The selected value (group rows: their container as a value).
    pub fn selected_value(&self) -> Option<Selected> {
        let row = self.row(self.selected?)?;
        match row.item {
            RowItem::Value(v) => Some(Selected {
                child: v.child,
                group: false,
            }),
            RowItem::Group { container, .. } | RowItem::Loading { container, .. } => {
                let (key, value) = self.selected_spans()?;
                Some(Selected {
                    child: Child {
                        key,
                        value,
                        kind: match container.kind {
                            ContainerKind::Object => ValueKind::Object,
                            ContainerKind::Array => ValueKind::Array,
                        },
                    },
                    group: true,
                })
            }
        }
    }

    /// The first [`SIBLINGS`] children of `parent` and the total count, or `None` while
    /// the container isn't ready (a prepare job is started).
    pub fn siblings(
        &mut self,
        parent: Container,
        repaint: &braceray_core::job::Repaint,
    ) -> Option<(Vec<Sibling>, u64)> {
        let nav = self.nav();
        let (Some(count), Some(kids)) = (
            nav.child_count_if_ready(&parent),
            nav.children_if_ready(&parent, 0..SIBLINGS),
        ) else {
            drop(nav);
            self.request_prepare(parent, repaint);
            return None;
        };
        let bytes = nav.bytes();
        let list = kids
            .iter()
            .enumerate()
            .map(|(i, c)| Sibling {
                index: i as u64,
                label: match c.key {
                    Some(k) => cut(&decode_key(k.slice(bytes))),
                    None => format!("[{i}]"),
                },
            })
            .collect();
        Some((list, count))
    }
}

/// `s` cut to [`LABEL_CHARS`] characters (with `…`), control characters shown as `·`.
fn cut(s: &str) -> String {
    let mut out: String = s
        .chars()
        .take(LABEL_CHARS)
        .map(|c| if c.is_control() { '·' } else { c })
        .collect();
    if s.chars().nth(LABEL_CHARS).is_some() {
        out.push('…');
    }
    if out.is_empty() {
        out.push_str("\"\"");
    }
    out
}
