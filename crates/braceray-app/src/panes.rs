//! The tree ↔ raw view link of one tab: which pane has the keyboard, the raw view itself,
//! and the sync in both directions (tree selection → raw scroll + flash; raw click →
//! tree selection, located on a worker).

use std::time::{Duration, Instant};

use braceray_core::job::{JobEvent, JobHandle, Repaint};
use braceray_core::nav::Span;

use crate::raw_view::RawView;
use crate::tree_view::{TreeView, navigator};

/// A tree selection caused by the raw view doesn't scroll the raw view back for this long.
const SUPPRESS: Duration = Duration::from_secs(3);

/// Which pane gets the keyboard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Pane {
    /// The tree (arrows move the selection).
    #[default]
    Tree,
    /// The raw view (arrows scroll).
    Raw,
}

/// Tree ↔ raw state of a tab.
#[derive(Debug, Default)]
pub struct Panes {
    /// The raw view (created with the tree).
    pub raw: Option<RawView>,
    /// Keyboard focus.
    pub focus: Pane,
    /// The tree selection the raw view last followed (key-or-value span).
    synced: Option<Span>,
    /// Until then, the next tree selection change came from the raw view.
    suppress_until: Option<Instant>,
    locate: Option<JobHandle<(), Vec<u64>>>,
}

/// Key-or-value span of the tree selection.
fn selected_span(tree: &TreeView) -> Option<Span> {
    let (key, value) = tree.selected_spans()?;
    Some(Span {
        start: key.map_or(value.start, |k| k.start),
        end: value.end,
    })
}

impl Panes {
    /// Every frame: creates the raw view, applies a finished locate job, and makes the raw
    /// view follow a new tree selection.
    pub fn update(&mut self, tree: &mut TreeView, repaint: &Repaint) {
        if self.raw.is_none() {
            self.raw = Some(RawView::new(tree));
            self.synced = selected_span(tree);
        }
        if let Some(event) = self.locate.as_mut().and_then(|j| j.try_recv()) {
            self.locate = None;
            if let JobEvent::Done(address) = event {
                self.suppress_until = Some(Instant::now() + SUPPRESS);
                tree.reveal_address(address, repaint);
            }
        }
        let span = selected_span(tree);
        if span != self.synced {
            self.synced = span;
            let from_raw = self
                .suppress_until
                .take()
                .is_some_and(|t| Instant::now() < t);
            if !from_raw && let (Some(span), Some(raw)) = (span, self.raw.as_mut()) {
                raw.reveal(tree, span, repaint);
            }
        }
    }

    /// Scrolls the raw view to the tree selection now (context menu "Show in raw view").
    pub fn show_selection(&mut self, tree: &TreeView, repaint: &Repaint) {
        if let (Some(span), Some(raw)) = (selected_span(tree), self.raw.as_mut()) {
            raw.reveal(tree, span, repaint);
        }
    }

    /// Selects the innermost value at `offset` in the tree (on a worker: finding it may
    /// need a big container's checkpoints).
    pub fn locate(&mut self, tree: &TreeView, offset: u64, repaint: &Repaint) {
        let (doc, source, cache) = tree.shared();
        self.locate = Some(JobHandle::spawn(
            "raw-locate",
            0,
            repaint.clone(),
            move |_| {
                let nav = navigator(&doc, &source, &cache);
                let loc = nav.node_at_offset(offset);
                Ok(nav.address_of(&loc))
            },
        ));
    }

    /// Selects `address` in the tree without scrolling the raw view (Go to).
    pub fn reveal_in_tree(&mut self, tree: &mut TreeView, address: Vec<u64>, repaint: &Repaint) {
        self.suppress_until = Some(Instant::now() + SUPPRESS);
        tree.reveal_address(address, repaint);
    }

    /// `true` while a locate job runs.
    pub fn is_locating(&self) -> bool {
        self.locate.is_some()
    }
}
