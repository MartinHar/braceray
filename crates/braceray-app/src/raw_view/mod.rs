//! State of one tab's raw text view: mode, the top row (anchored to a byte offset, never
//! to a global line number), the rows on screen, far jumps finished on a worker, the text
//! selection and the "flash" after a tree click. No egui here; `ui::raw` draws it.

use std::time::{Duration, Instant};

use braceray_core::job::Repaint;
use braceray_core::nav::{Navigator, Span};
use braceray_core::pretty::lex::{LexState, state_in_row};
use braceray_core::pretty::original::{next_row, prev_row, row_at};
use braceray_core::pretty::{
    Generator, PrettyOptions, PrettyPos, Seek, SeekState, anchor_before, prev_line, seek,
};

use crate::tree_view::TreeView;

mod rows;
mod seek;
pub mod text;

pub use rows::{Gutter, RawRow};
use rows::{Pos, infer_start_state, original_row, pretty_row};
use seek::{AfterSeek, SeekJob};

/// Bytes per row before a long line or token wraps.
pub const WRAP: usize = 400;
/// Seek work done on the UI thread before a jump moves to a worker (≈ 1–2 ms).
pub const SYNC_BUDGET: usize = 1 << 20;
/// Seek work per step on a worker (cancel checked between steps).
pub(super) const JOB_BUDGET: usize = 16 << 20;
/// Selections up to this size can be copied.
pub const COPY_LIMIT: u64 = 50 << 20;
/// How long a revealed value stays highlighted.
pub const FLASH: Duration = Duration::from_millis(1200);
/// Rows kept above a revealed value.
const CONTEXT_ROWS: usize = 2;
/// Pretty mode is the default when lines are longer than this on average.
pub const PRETTY_AVG_LINE: u64 = 2000;

/// How the raw view lays out the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawMode {
    /// The file's own lines, soft-wrapped.
    Original,
    /// Formatted on the fly.
    Pretty,
}

/// Raw view state of one tab.
pub struct RawView {
    /// Current mode.
    pub mode: RawMode,
    top: Pos,
    /// Fraction of the top row scrolled out of view (0 ≤ frac < 1).
    pub frac: f32,
    /// Horizontal scroll in points.
    pub hscroll: f32,
    rows: Vec<RawRow>,
    seek: Option<SeekJob>,
    /// Text selection as source offsets: where the drag started and where it is now.
    pub selection: Option<(u64, u64)>,
    /// A revealed value, highlighted until the instant + [`FLASH`].
    pub flash: Option<(Span, Instant)>,
    /// A short message (e.g. the copy limit), shown until the instant.
    pub notice: Option<(String, Instant)>,
    /// Scroll horizontally so this offset is visible once its row is drawn.
    pub reveal_x: Option<u64>,
    source_generation: u64,
    len: u64,
    opts: PrettyOptions,
}

impl std::fmt::Debug for RawView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawView")
            .field("mode", &self.mode)
            .field("top", &self.top_offset())
            .field("seeking", &self.is_seeking())
            .finish_non_exhaustive()
    }
}

/// The default mode for a document: Pretty when its lines are long on average.
pub fn default_mode(nav: &Navigator<'_>) -> RawMode {
    let lines = nav.index().lines().line_count().max(1);
    if nav.bytes().len() as u64 / lines > PRETTY_AVG_LINE {
        RawMode::Pretty
    } else {
        RawMode::Original
    }
}

impl RawView {
    /// A raw view of the tree's document at its start.
    pub fn new(tree: &TreeView) -> Self {
        let nav = tree.nav();
        let mode = default_mode(&nav);
        let mut view = RawView {
            mode,
            top: Pos::Pretty(PrettyPos::START),
            frac: 0.0,
            hscroll: 0.0,
            rows: Vec::new(),
            seek: None,
            selection: None,
            flash: None,
            reveal_x: None,
            notice: None,
            source_generation: tree.source_generation(),
            len: nav.bytes().len() as u64,
            opts: PrettyOptions {
                wrap: WRAP,
                indent: 2,
            },
        };
        view.top = view.start_pos(&nav);
        view
    }

    fn start_pos(&self, nav: &Navigator<'_>) -> Pos {
        match self.mode {
            RawMode::Pretty => Pos::Pretty(PrettyPos::START),
            RawMode::Original => Pos::Original(
                row_at(nav.bytes(), nav.index().lines(), 0, WRAP),
                LexState::default(),
            ),
        }
    }

    /// Offset of the top row's first byte.
    pub fn top_offset(&self) -> u64 {
        self.top.offset()
    }

    /// Length of the browsed bytes.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// `true` for an empty document.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// `true` while a far jump runs on a worker.
    pub fn is_seeking(&self) -> bool {
        self.seek.is_some()
    }

    /// Follows the tree to a new source (preview → full index), keeping the position.
    pub fn sync_source(&mut self, tree: &TreeView, repaint: &Repaint) {
        if tree.source_generation() != self.source_generation {
            self.source_generation = tree.source_generation();
            let nav = tree.nav();
            self.len = nav.bytes().len() as u64;
            let at = self.top_offset().min(self.len);
            self.seek = None;
            self.go_to(tree, at, 0, repaint);
        }
    }

    /// Switches the mode, keeping the top row's position.
    pub fn set_mode(&mut self, mode: RawMode, tree: &TreeView, repaint: &Repaint) {
        if mode != self.mode {
            let at = self.top_offset();
            self.mode = mode;
            self.seek = None;
            self.hscroll = 0.0;
            self.go_to(tree, at, 0, repaint);
        }
    }

    /// Shows `span` near the top and highlights it for a moment.
    pub fn reveal(&mut self, tree: &TreeView, span: Span, repaint: &Repaint) {
        self.flash = Some((span, Instant::now()));
        self.reveal_x = Some(span.start);
        self.go_to(tree, span.start, CONTEXT_ROWS, repaint);
    }

    /// Puts the row holding `offset` at the top, with `context` rows above it. Far jumps
    /// that need more than [`SYNC_BUDGET`] finish on a worker.
    pub fn go_to(&mut self, tree: &TreeView, offset: u64, context: usize, repaint: &Repaint) {
        let nav = tree.nav();
        let bytes = nav.bytes();
        let offset = offset.min(self.len);
        self.frac = 0.0;
        self.rows.clear();
        match self.mode {
            RawMode::Pretty => {
                match seek(
                    bytes,
                    anchor_before(&nav, offset),
                    offset,
                    SYNC_BUDGET,
                    &self.opts,
                ) {
                    Seek::Found(p) => {
                        self.seek = None;
                        self.top = Pos::Pretty(p);
                        self.step_up(&nav, context);
                    }
                    Seek::More(state) => {
                        self.spawn_seek(tree, state, AfterSeek::Top { context }, repaint);
                    }
                }
            }
            RawMode::Original => {
                self.seek = None;
                let row = row_at(bytes, nav.index().lines(), offset, WRAP);
                let state = if row.is_line_start() {
                    Ok(LexState::default())
                } else {
                    self.state_at(&nav, row.start)
                };
                let state = match state {
                    Ok(s) => s,
                    Err(more) => {
                        self.spawn_seek(tree, more, AfterSeek::OriginalState, repaint);
                        LexState::default()
                    }
                };
                self.top = Pos::Original(row, state);
                self.step_up(&nav, context);
            }
        }
    }

    /// The lexer state at `offset` from the pretty row holding it (budgeted).
    fn state_at(&self, nav: &Navigator<'_>, offset: u64) -> Result<LexState, SeekState> {
        let bytes = nav.bytes();
        match seek(
            bytes,
            anchor_before(nav, offset),
            offset,
            SYNC_BUDGET,
            &self.opts,
        ) {
            Seek::Found(p) => Ok(self.state_in(bytes, p, offset)),
            Seek::More(state) => Err(state),
        }
    }

    fn state_in(&self, bytes: &[u8], row: PrettyPos, offset: u64) -> LexState {
        Generator::new(bytes, row, &self.opts)
            .next()
            .map(|line| state_in_row(bytes, &line, offset))
            .unwrap_or_default()
    }

    /// Moves the top up by up to `n` rows (stops at the first row or when a step needs a
    /// worker). Returns the rows moved.
    fn step_up(&mut self, nav: &Navigator<'_>, n: usize) -> usize {
        for k in 0..n {
            match self.prev_pos(nav) {
                Some(Ok(p)) => {
                    self.top = p;
                    self.rows.clear();
                }
                Some(Err(_)) | None => return k,
            }
        }
        n
    }

    /// The row start before the top: `None` at the first row, `Err` when it needs a worker.
    fn prev_pos(&self, nav: &Navigator<'_>) -> Option<Result<Pos, SeekState>> {
        let bytes = nav.bytes();
        match self.top {
            Pos::Pretty(p) => match prev_line(nav, p, SYNC_BUDGET, &self.opts)? {
                Seek::Found(q) => Some(Ok(Pos::Pretty(q))),
                Seek::More(s) => Some(Err(s)),
            },
            Pos::Original(row, state) => {
                let prev = prev_row(bytes, nav.index().lines(), &row, WRAP)?;
                Some(Ok(Pos::Original(
                    prev,
                    infer_start_state(bytes, &prev, state),
                )))
            }
        }
    }

    /// The first `n` rows from the top (fewer at the end of the document).
    pub fn rows(&mut self, tree: &TreeView, n: usize) -> &[RawRow] {
        self.fill(&tree.nav(), n);
        // Scrolling up prepends rows; drop the ones far below the screen.
        if self.rows.len() > 2 * n + 64 {
            self.rows.truncate(n + 32);
        }
        &self.rows[..n.min(self.rows.len())]
    }

    fn fill(&mut self, nav: &Navigator<'_>, n: usize) {
        if self.rows.len() < n {
            let have = self.rows.len();
            let next = match self.rows.last() {
                Some(last) => self.after(nav, last),
                None => Some(self.top),
            };
            if let Some(start) = next {
                let more = self.make_rows(nav, start, n - have);
                self.rows.extend(more);
            }
        }
    }

    /// The row start after `row`, if any.
    fn after(&self, nav: &Navigator<'_>, row: &RawRow) -> Option<Pos> {
        match row.pos {
            Pos::Pretty(_) => Some(Pos::Pretty(row.next)),
            Pos::Original(r, _) => {
                next_row(nav.bytes(), &r, WRAP).map(|n| Pos::Original(n, row.end_state))
            }
        }
    }

    fn make_rows(&self, nav: &Navigator<'_>, start: Pos, n: usize) -> Vec<RawRow> {
        let bytes = nav.bytes();
        let mut out = Vec::with_capacity(n);
        match start {
            Pos::Pretty(p) => {
                for line in Generator::new(bytes, p, &self.opts).take(n) {
                    out.push(pretty_row(&line));
                }
            }
            Pos::Original(mut row, mut state) => loop {
                let r = original_row(bytes, row, state);
                state = r.end_state;
                out.push(r);
                if out.len() >= n {
                    break;
                }
                match next_row(bytes, &row, WRAP) {
                    Some(next) => row = next,
                    None => break,
                }
            },
        }
        out
    }

    /// Scrolls by `delta` rows (fractional; negative = up), keeping at least `full_rows`
    /// rows on screen when the end is reached.
    pub fn scroll_by(&mut self, tree: &TreeView, delta: f64, full_rows: usize, repaint: &Repaint) {
        if self.seek.is_some() {
            return;
        }
        let nav = tree.nav();
        let mut pos = f64::from(self.frac) + delta;
        // Down: drop rows from the top while more than a screen follows.
        while pos >= 1.0 {
            self.fill(&nav, full_rows + 1);
            if self.rows.len() <= full_rows {
                pos = 0.0;
                break;
            }
            self.rows.remove(0);
            self.top = self.rows[0].pos;
            pos -= 1.0;
        }
        // Up: step back.
        while pos < 0.0 {
            if !self.step_back(tree, &nav, repaint) {
                pos = 0.0;
                break;
            }
            pos += 1.0;
        }
        self.frac = pos as f32;
    }

    /// Moves the top one row up; `false` at the first row or when a worker takes over.
    fn step_back(&mut self, tree: &TreeView, nav: &Navigator<'_>, repaint: &Repaint) -> bool {
        match self.prev_pos(nav) {
            Some(Ok(p)) => {
                let first = self.make_rows(nav, p, 1);
                self.top = p;
                self.rows.splice(0..0, first);
                true
            }
            Some(Err(state)) => {
                self.spawn_seek(tree, state, AfterSeek::Previous, repaint);
                false
            }
            None => false,
        }
    }

    /// Scrolls up until `full_rows` rows are on screen (after a jump near the end).
    pub fn fill_screen(&mut self, tree: &TreeView, full_rows: usize, repaint: &Repaint) {
        if self.seek.is_some() {
            return;
        }
        let nav = tree.nav();
        for _ in 0..full_rows {
            self.fill(&nav, full_rows);
            if self.rows.len() >= full_rows || !self.step_back(tree, &nav, repaint) {
                break;
            }
        }
    }

    /// Fraction of the document above the top row (for the scrollbar).
    pub fn fraction(&self) -> f64 {
        if self.len == 0 {
            0.0
        } else {
            self.top_offset() as f64 / self.len as f64
        }
    }

    /// The selected source span, if any (non-empty).
    pub fn selected_span(&self) -> Option<Span> {
        let (a, b) = self.selection?;
        (a != b).then(|| Span {
            start: a.min(b),
            end: a.max(b),
        })
    }

    /// The selected source bytes as text for the clipboard, or a message when the
    /// selection is over [`COPY_LIMIT`].
    pub fn copy_text(&self, bytes: &[u8]) -> Option<Result<String, String>> {
        let span = self.selected_span()?;
        let size = span.end - span.start;
        if size > COPY_LIMIT {
            return Some(Err(format!(
                "The selection is {}; the copy limit is {}.",
                crate::format::bytes(size),
                crate::format::bytes(COPY_LIMIT)
            )));
        }
        Some(Ok(String::from_utf8_lossy(span.slice(bytes)).into_owned()))
    }

    /// Number of formatted rows cached (debug overlay).
    pub fn cached_rows(&self) -> usize {
        self.rows.len()
    }
}
