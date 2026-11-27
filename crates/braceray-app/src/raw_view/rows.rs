//! Building raw view rows: pretty rows from the generator, original rows with their
//! highlight tokens, and the lexer state when scrolling up in original mode.

use braceray_core::nav::Span;
use braceray_core::pretty::lex::{LexState, tokenize};
use braceray_core::pretty::original::OrigRow;
use braceray_core::pretty::{PrettyLine, PrettyPos, Token};

/// A row start in either mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pos {
    Pretty(PrettyPos),
    /// The row and the lexer state at its start.
    Original(OrigRow, LexState),
}

impl Pos {
    pub(super) fn offset(&self) -> u64 {
        match self {
            Pos::Pretty(p) => p.offset,
            Pos::Original(r, _) => r.start,
        }
    }
}

/// What the gutter shows for a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gutter {
    /// 1-based line number (original mode, first row of a line).
    Line(u64),
    /// A continuation row (↩).
    Continuation,
    /// Byte offset of the row's first token (pretty mode).
    Offset(u64),
}

/// One row on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRow {
    pub(super) pos: Pos,
    /// The source bytes the row covers (rows tile the document).
    pub span: Span,
    /// The bytes shown in original mode (the row without its line terminator).
    pub text: Span,
    /// Indentation level (pretty mode).
    pub depth: u32,
    /// Highlight tokens. Pretty mode prints exactly these (with `": "` after colons);
    /// original mode prints `text` and colours these parts of it.
    pub tokens: Vec<Token>,
    /// Gutter content.
    pub gutter: Gutter,
    /// The row continues the previous one.
    pub continued: bool,
    /// The row goes on in the next one.
    pub wraps: bool,
    pub(super) end_state: LexState,
    /// Pretty mode: where the next row starts.
    pub(super) next: PrettyPos,
}

pub(super) fn pretty_row(line: &PrettyLine) -> RawRow {
    let first = line
        .tokens
        .first()
        .map_or(line.pos.offset, |t| t.span.start);
    RawRow {
        pos: Pos::Pretty(line.pos),
        span: line.source_span(),
        text: line.source_span(),
        depth: line.depth,
        tokens: line.tokens.clone(),
        gutter: if line.is_continuation() {
            Gutter::Continuation
        } else {
            Gutter::Offset(first)
        },
        continued: line.is_continuation(),
        wraps: line.wraps(),
        end_state: LexState::default(),
        next: line.next,
    }
}

pub(super) fn original_row(bytes: &[u8], row: OrigRow, state: LexState) -> RawRow {
    let text_len = row.text(bytes).len() as u64;
    let text = Span {
        start: row.start,
        end: row.start + text_len,
    };
    let mut tokens = Vec::new();
    let end_state = tokenize(bytes, text, state, &mut tokens);
    // The newline after the text ends a line (and any string state with it).
    let end_state = if text.end < row.end {
        LexState::default()
    } else {
        end_state
    };
    RawRow {
        pos: Pos::Original(row, state),
        span: Span {
            start: row.start,
            end: row.end,
        },
        text,
        depth: 0,
        tokens,
        gutter: if row.is_line_start() {
            Gutter::Line(row.line)
        } else {
            Gutter::Continuation
        },
        continued: !row.is_line_start(),
        wraps: text.end == row.end && (row.end as usize) < bytes.len(),
        end_state,
        next: PrettyPos::START,
    }
}

/// The lexer state at the start of `row`, given the state at its end (scrolling up in
/// original mode): the candidate whose lexing reproduces `end` (line starts are always
/// outside strings in valid JSON).
pub(super) fn infer_start_state(bytes: &[u8], row: &OrigRow, end: LexState) -> LexState {
    if row.is_line_start() {
        return LexState::default();
    }
    let text = Span {
        start: row.start,
        end: row.start + row.text(bytes).len() as u64,
    };
    let candidates = [
        LexState {
            in_string: true,
            escaped: false,
        },
        LexState::default(),
        LexState {
            in_string: true,
            escaped: true,
        },
    ];
    let mut sink = Vec::new();
    candidates
        .into_iter()
        .find(|&c| {
            sink.clear();
            tokenize(bytes, text, c, &mut sink) == end
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use braceray_core::pretty::original::{next_row, row_at};

    #[test]
    fn infer_state() {
        let bytes = br#"["abcdefghijklmnopqrstuvwxyz0123456789", 1]"#;
        let lines = braceray_core::index::LineTable::build(bytes);
        let first = row_at(bytes, &lines, 0, 16);
        let second = next_row(bytes, &first, 16).unwrap();
        let a = original_row(bytes, first, LexState::default());
        let b = original_row(bytes, second, a.end_state);
        assert!(a.end_state.in_string);
        let third = next_row(bytes, &second, 16).unwrap();
        assert_eq!(
            infer_start_state(
                bytes,
                &third,
                original_row(bytes, third, b.end_state).end_state
            ),
            b.end_state
        );
        assert_eq!(infer_start_state(bytes, &second, b.end_state), a.end_state);
    }
}
