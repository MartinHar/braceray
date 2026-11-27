//! Far jumps and steps up that need more than the UI thread's budget: they continue on a
//! worker (building a big container's checkpoint table first when that gives better
//! anchors), and the view picks the result up in [`RawView::poll`].

use braceray_core::job::{JobEvent, JobHandle, Repaint};
use braceray_core::pretty::{PrettyPos, Seek, SeekState, anchor_before, seek};

use super::rows::Pos;
use super::{JOB_BUDGET, RawMode, RawView};
use crate::tree_view::{TreeView, navigator};

/// A far jump (or a step up) that a worker finishes.
pub(super) struct SeekJob {
    pub(super) target: u64,
    pub(super) job: JobHandle<(), PrettyPos>,
    pub(super) then: AfterSeek,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AfterSeek {
    /// Put the row near the top (`context` rows above it).
    Top { context: usize },
    /// The row before the current top (scrolling up).
    Previous,
    /// Original mode: the lexer state of the top row.
    OriginalState,
}

impl RawView {
    pub(super) fn spawn_seek(
        &mut self,
        tree: &TreeView,
        state: SeekState,
        then: AfterSeek,
        repaint: &Repaint,
    ) {
        let (doc, source, cache) = tree.shared();
        let opts = self.opts;
        let target = state.target();
        let job = JobHandle::spawn("raw-seek", 0, repaint.clone(), move |ctx| {
            let nav = navigator(&doc, &source, &cache);
            let bytes = nav.bytes();
            // A big container without a checkpoint table gives poor anchors: build it.
            let mut state = state;
            if let Some(n) = nav.innermost_node(target) {
                let c = nav.container(n);
                if !nav.is_ready(&c) {
                    nav.prepare(&c, ctx.cancel())?;
                    let anchor = anchor_before(&nav, target);
                    if anchor.offset > state.reached() {
                        state = match seek(bytes, anchor, target, 0, &opts) {
                            Seek::Found(p) => return Ok(p),
                            Seek::More(s) => s,
                        };
                    }
                }
            }
            loop {
                ctx.cancel().check()?;
                match state.resume(bytes, JOB_BUDGET, &opts) {
                    Seek::Found(p) => return Ok(p),
                    Seek::More(s) => state = s,
                }
            }
        });
        self.seek = Some(SeekJob { target, job, then });
    }

    /// Polls the worker seek (every frame).
    pub fn poll(&mut self, tree: &TreeView) {
        let Some(s) = &mut self.seek else {
            return;
        };
        let event = s.job.try_recv();
        let (target, then) = (s.target, s.then);
        match event {
            Some(JobEvent::Done(p)) => {
                self.seek = None;
                let nav = tree.nav();
                match (then, self.mode) {
                    (AfterSeek::Top { context }, RawMode::Pretty) => {
                        self.top = Pos::Pretty(p);
                        self.frac = 0.0;
                        self.rows.clear();
                        self.step_up(&nav, context);
                    }
                    (AfterSeek::Previous, RawMode::Pretty) => {
                        self.top = Pos::Pretty(p);
                        self.rows.clear();
                    }
                    (AfterSeek::OriginalState, RawMode::Original) => {
                        if let Pos::Original(row, _) = self.top
                            && row.start == target
                        {
                            self.top = Pos::Original(row, self.state_in(nav.bytes(), p, target));
                            self.rows.clear();
                        }
                    }
                    _ => {} // the mode changed meanwhile
                }
            }
            Some(JobEvent::Failed(e)) => {
                tracing::warn!("raw view: seek failed: {e}");
                self.seek = None;
            }
            Some(JobEvent::Cancelled) => self.seek = None,
            _ => {
                if s.job.is_finished() {
                    self.seek = None;
                }
            }
        }
    }
}
