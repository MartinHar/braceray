//! Finding rows away from the current one: a row start before any offset
//! ([`anchor_before`], from the index), the row holding an offset ([`seek`], budgeted and
//! resumable), and the row before a row ([`prev_line`]).

use super::generate::{scalar_close, string_close};
use super::wrap::{row_of, token_break};
use super::{Generator, PrettyOptions, PrettyPos, TokenKind, TokenStart};
use crate::document::is_json_ws;
use crate::index::NO_PARENT;
use crate::nav::{Navigator, NodeRef};

/// Scanning for the end of a long token counts this many bytes as one budget byte (memchr
/// is that much faster than formatting rows).
const SCAN_DISCOUNT: usize = 8;

/// Result of [`seek`] / [`prev_line`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seek {
    /// The row start that was looked for.
    Found(PrettyPos),
    /// The budget ran out: continue with [`SeekState::resume`] (on a worker, with a
    /// bigger budget).
    More(SeekState),
}

/// A [`seek`] that ran out of budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeekState {
    from: PrettyPos,
    last: PrettyPos,
    target: u64,
}

impl SeekState {
    /// The offset whose row is looked for.
    pub fn target(&self) -> u64 {
        self.target
    }

    /// How far the search got (a byte offset ≤ the target).
    pub fn reached(&self) -> u64 {
        self.from.offset
    }

    /// Continues the search with another `budget`.
    pub fn resume(&self, bytes: &[u8], budget: usize, opts: &PrettyOptions) -> Seek {
        run(bytes, *self, budget, opts)
    }
}

/// A row start at or before `offset`, found from the index without formatting anything:
/// the first child or a cached checkpoint of the innermost stored container, or the end of
/// the last stored container closed before `offset`. Falls back to
/// [`PrettyPos::START`]. Cost: a binary search plus a walk up the parents.
///
/// The gap from the anchor to `offset` holds no stored container start, but it can be
/// long (a huge string, or millions of scalars in an array without a cached checkpoint
/// table: prepare it with [`Navigator::prepare`] first, on a worker).
///
/// # Example
/// ```
/// use braceray_core::index::{build_bytes, IndexOptions};
/// use braceray_core::nav::Navigator;
/// use braceray_core::pretty::{anchor_before, seek, PrettyOptions, PrettyPos, Seek};
/// use braceray_core::{CancelToken, Progress};
///
/// let bytes = br#"{"a": [1, 2, 3], "b": {"c": true}}"#;
/// let index = build_bytes(bytes, &IndexOptions::default(), &CancelToken::new(), &Progress::default())?;
/// let nav = Navigator::new(bytes, &index);
/// let target = bytes.iter().position(|&b| b == b't').unwrap() as u64;
/// let anchor = anchor_before(&nav, target);
/// assert_eq!(anchor, PrettyPos::line(23, 2)); // just after the `{` of "b"
/// let Seek::Found(row) = seek(bytes, anchor, target, usize::MAX, &PrettyOptions::default()) else {
///     unreachable!()
/// };
/// assert_eq!(row, anchor); // `"c": true` is that row
/// # Ok::<(), braceray_core::Error>(())
/// ```
pub fn anchor_before(nav: &Navigator<'_>, offset: u64) -> PrettyPos {
    let bytes = nav.bytes();
    let nodes = nav.index().nodes();
    let x = offset.min(bytes.len() as u64);
    let mut best = PrettyPos::START;
    let mut consider = |p: PrettyPos| {
        if p.offset <= x && p.offset > best.offset {
            best = p;
        }
    };

    // The innermost container whose *inside* holds x (its opening bracket lies on the
    // parent's row; an empty container is a single row of the parent).
    let mut inner = nav.innermost_node(x);
    while let Some(n) = inner {
        let node = &nodes[n.0 as usize];
        if node.start() == x || is_empty(bytes, node.start(), node.end()) {
            inner = nav.parent(n);
        } else {
            break;
        }
    }
    if let Some(n) = inner {
        let node = &nodes[n.0 as usize];
        let d = node.depth() + 1;
        consider(PrettyPos::line(node.start() + 1, d));
        if let Some(cp) = nav.checkpoint_before_if_ready(&nav.container(n), x) {
            consider(PrettyPos::line(cp, d));
        }
        if node.end() <= x && is_close(bytes, node.end()) {
            consider(PrettyPos::line(node.end(), d));
        }
    }

    // The last stored container that starts at or before x; its ancestor directly inside
    // `inner` (if it closed before x) ends close to x.
    let last = nodes.partition_point(|n| n.start() <= x);
    if let Some(i) = last.checked_sub(1) {
        let stop = inner.map_or(NO_PARENT, |n| n.0);
        let mut c = i as u32;
        let child = loop {
            if c == stop {
                break None;
            }
            let p = nodes[c as usize].parent();
            if p == stop {
                break Some(NodeRef(c));
            }
            if p == NO_PARENT {
                break None;
            }
            c = p;
        };
        if let Some(c) = child {
            let node = &nodes[c.0 as usize];
            if node.end() < x && is_close(bytes, node.end()) {
                if !is_empty(bytes, node.start(), node.end()) {
                    consider(PrettyPos::line(node.end(), node.depth() + 1));
                }
                let after = skip_ws(bytes, node.end() as usize + 1);
                match bytes.get(after) {
                    Some(b',') => consider(PrettyPos::line(after as u64 + 1, node.depth())),
                    Some(b'}' | b']') => consider(PrettyPos::line(after as u64, node.depth())),
                    _ => {}
                }
            }
        }
    }
    best
}

/// The row whose source span holds `target`, formatting forward from the row start
/// `from` (≤ `target`). Stops after about `budget` bytes of work and returns
/// [`Seek::More`]; every call makes progress, however small the budget. Inside long
/// tokens it jumps from row to row without formatting them. A `target` past the last row
/// gives the last row.
pub fn seek(
    bytes: &[u8],
    from: PrettyPos,
    target: u64,
    budget: usize,
    opts: &PrettyOptions,
) -> Seek {
    let state = SeekState {
        from,
        last: from,
        target,
    };
    run(bytes, state, budget, opts)
}

fn run(bytes: &[u8], state: SeekState, budget: usize, opts: &PrettyOptions) -> Seek {
    let wrap = opts.wrap();
    let target = state.target;
    let mut g = Generator::new(bytes, state.from, opts);
    let mut last = state.last;
    let mut spent = 0usize;
    loop {
        let pos = g.pos();
        if spent > budget {
            return Seek::More(SeekState {
                from: pos,
                last,
                target,
            });
        }
        if let Some(t) = pos.token {
            let at = pos.offset as usize;
            let s = t.start as usize;
            let row_at = |x: usize| {
                let (k, b) = row_of(bytes, s, t.kind, x, wrap);
                (k > 0).then_some(PrettyPos {
                    offset: b as u64,
                    depth: pos.depth,
                    token: Some(t),
                })
            };
            // Scan for the token end, but always at least a few rows' worth.
            let allowed = budget
                .saturating_sub(spent)
                .saturating_mul(SCAN_DISCOUNT)
                .max(4 * wrap);
            match token_end(bytes, &t, at, allowed) {
                Some(end) => {
                    spent += (end - at) / SCAN_DISCOUNT;
                    if (target as usize) < end {
                        if let Some(row) = row_at(target as usize) {
                            return Seek::Found(row);
                        }
                    } else if let Some(row) = row_at(end.saturating_sub(1))
                        && row.offset > pos.offset
                    {
                        g.set_pos(row);
                    }
                }
                None => {
                    // The token goes on past `reach`: jump there and pause.
                    let reach = at + allowed;
                    if (target as usize) < reach
                        && let Some(row) = row_at(target as usize)
                    {
                        return Seek::Found(row);
                    }
                    if let Some(row) = row_at(reach - 1)
                        && row.offset > pos.offset
                    {
                        return Seek::More(SeekState {
                            from: row,
                            last: row,
                            target,
                        });
                    }
                }
            }
        }
        match g.next_line() {
            None => return Seek::Found(last),
            Some(line) => {
                spent += (line.next.offset - line.pos.offset) as usize;
                if line.next.offset > target {
                    return Seek::Found(line.pos);
                }
                last = line.pos;
            }
        }
    }
}

/// The row before the row starting at `pos`, or `None` for the first row.
pub fn prev_line(
    nav: &Navigator<'_>,
    pos: PrettyPos,
    budget: usize,
    opts: &PrettyOptions,
) -> Option<Seek> {
    let bytes = nav.bytes();
    match pos.token {
        Some(t) => {
            let s = t.start as usize;
            let (k, _) = row_of(bytes, s, t.kind, pos.offset as usize, opts.wrap());
            if k >= 2 {
                let b = token_break(bytes, s, t.kind, k - 1, opts.wrap());
                return Some(Seek::Found(PrettyPos {
                    offset: b as u64,
                    ..pos
                }));
            }
            // The row holding the token start.
            let line = PrettyPos::line(t.line, t.line_depth);
            Some(seek(bytes, line, t.start, budget, opts))
        }
        None if pos.offset == 0 => None,
        None => {
            let target = pos.offset - 1;
            Some(seek(
                bytes,
                anchor_before(nav, target),
                target,
                budget,
                opts,
            ))
        }
    }
}

/// End of the long token `t` whose row starts at `at`, scanning at most `allowed` bytes.
fn token_end(bytes: &[u8], t: &TokenStart, at: usize, allowed: usize) -> Option<usize> {
    let limit = at.saturating_add(allowed).min(bytes.len());
    let found = match t.kind {
        TokenKind::String | TokenKind::Key => string_close(bytes, at, limit),
        _ => scalar_close(bytes, at, limit),
    };
    match found {
        Some(e) => Some(e),
        None if limit == bytes.len() => Some(bytes.len()),
        None => None,
    }
}

fn skip_ws(bytes: &[u8], mut p: usize) -> usize {
    while p < bytes.len() && is_json_ws(bytes[p]) {
        p += 1;
    }
    p
}

fn is_close(bytes: &[u8], at: u64) -> bool {
    matches!(bytes.get(at as usize), Some(b'}' | b']'))
}

/// `true` if the container `start..=end` holds only whitespace.
fn is_empty(bytes: &[u8], start: u64, end: u64) -> bool {
    skip_ws(bytes, start as usize + 1) >= end as usize
}
