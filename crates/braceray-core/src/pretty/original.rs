//! Original mode: the file's own lines, soft-wrapped every `wrap` bytes from the line
//! start (moved back onto a UTF-8 boundary). Every row is found in O(log n + 64 KiB) from
//! any offset through the [`LineTable`], so a single 2 GB line is never scanned.

use super::TokenKind;
use super::wrap::{next_break, row_of, token_break};
use crate::index::LineTable;

/// One row of the original-mode view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OrigRow {
    /// First byte.
    pub start: u64,
    /// One past the last byte; includes the line's `\n` on its last row, so rows tile
    /// the document.
    pub end: u64,
    /// Start of the file line this row belongs to.
    pub line_start: u64,
    /// 1-based line number.
    pub line: u64,
}

impl OrigRow {
    /// `true` for the first row of a line (the gutter shows the number there).
    pub fn is_line_start(&self) -> bool {
        self.start == self.line_start
    }

    /// The bytes to show: the row without its line terminator.
    pub fn text<'b>(&self, bytes: &'b [u8]) -> &'b [u8] {
        let end = (self.end as usize).min(bytes.len());
        let mut t = &bytes[(self.start as usize).min(end)..end];
        if let Some(rest) = t.strip_suffix(b"\n") {
            t = rest.strip_suffix(b"\r").unwrap_or(rest);
        }
        t
    }
}

// Breaks only care about UTF-8 here, which is what a non-string kind gives.
const KIND: TokenKind = TokenKind::Punct;

/// The row starting at `start` (a line start or a break) of the line at `line_start`.
fn row_from(bytes: &[u8], start: u64, line_start: u64, line: u64, wrap: usize) -> OrigRow {
    let (s, l) = (start as usize, line_start as usize);
    let brk = next_break(bytes, l, KIND, s, wrap).min(bytes.len());
    let end = match memchr::memchr(b'\n', &bytes[s..brk]) {
        Some(p) => s + p + 1,
        None => brk,
    };
    OrigRow {
        start,
        end: end as u64,
        line_start,
        line,
    }
}

/// The row holding `offset` (the last row for `offset ≥ len`).
///
/// # Example
/// ```
/// use braceray_core::index::LineTable;
/// use braceray_core::pretty::original::{next_row, row_at};
///
/// let bytes = b"[1,\n 22222222222222222222222222222222222222,\n 3]";
/// let lines = LineTable::build(bytes);
/// let row = row_at(bytes, &lines, 10, 16);
/// assert_eq!((row.start, row.line, row.is_line_start()), (4, 2, true));
/// let next = next_row(bytes, &row, 16).unwrap();
/// assert_eq!((next.start, next.line, next.is_line_start()), (20, 2, false));
/// ```
pub fn row_at(bytes: &[u8], lines: &LineTable, offset: u64, wrap: usize) -> OrigRow {
    let wrap = wrap.max(super::MIN_WRAP);
    let x = (offset as usize).min(bytes.len().saturating_sub(1));
    let l = lines.line_start(bytes, x as u64) as usize;
    let (_, b) = row_of(bytes, l, KIND, x, wrap);
    let line = lines.line_of(bytes, l as u64);
    row_from(bytes, b as u64, l as u64, line, wrap)
}

/// The row after `row`, or `None` at the end.
pub fn next_row(bytes: &[u8], row: &OrigRow, wrap: usize) -> Option<OrigRow> {
    let wrap = wrap.max(super::MIN_WRAP);
    if row.end as usize >= bytes.len() {
        return None;
    }
    if bytes[row.end as usize - 1] == b'\n' {
        Some(row_from(bytes, row.end, row.end, row.line + 1, wrap))
    } else {
        Some(row_from(bytes, row.end, row.line_start, row.line, wrap))
    }
}

/// The row before `row`, or `None` for the first row.
pub fn prev_row(bytes: &[u8], lines: &LineTable, row: &OrigRow, wrap: usize) -> Option<OrigRow> {
    let wrap = wrap.max(super::MIN_WRAP);
    if row.start == 0 {
        return None;
    }
    if row.start > row.line_start {
        let l = row.line_start as usize;
        let (k, _) = row_of(bytes, l, KIND, row.start as usize, wrap);
        let start = if k <= 1 {
            l
        } else {
            token_break(bytes, l, KIND, k - 1, wrap)
        };
        return Some(row_from(
            bytes,
            start as u64,
            row.line_start,
            row.line,
            wrap,
        ));
    }
    let x = row.start as usize - 1; // the previous line's `\n`
    let l = lines.line_start(bytes, x as u64) as usize;
    let (_, b) = row_of(bytes, l, KIND, x, wrap);
    Some(row_from(
        bytes,
        b as u64,
        l as u64,
        row.line.saturating_sub(1).max(1),
        wrap,
    ))
}
