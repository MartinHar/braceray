//! On-the-fly pretty printing of any region of a document (step 08), for the raw view.
//!
//! The printer never formats the whole file. It starts at a [`PrettyPos`] (a byte offset
//! where a formatted line begins, plus the nesting depth there) and emits formatted lines
//! one at a time until the viewport is full ([`pretty_lines`]). Every line knows the
//! source bytes it covers, so the view scrolls by byte offset and the scrollbar maps
//! `0..len` directly.
//!
//! The line layout is a pure function of the bytes: starting from any valid position
//! gives the same lines as starting from the beginning of the file. That is what lets the
//! view jump to 63 % of a 2 GB file ([`anchor_before`] + [`seek`]) and scroll back up
//! ([`prev_line`]) without formatting everything before it.
//!
//! **Layout:** one member or element per line; `{` / `[` end a line (empty containers
//! stay `{}` / `[]`); closing brackets start their own line; `,` ends a line; `:` is
//! followed by one space. Tokens longer than [`PrettyOptions::wrap`] bytes are split
//! into continuation rows at fixed steps from the token start (moved back to a UTF-8
//! character boundary and off an escape), so the row before or after any position inside
//! a 200 MB string is found in O(1).
//!
//! **Original mode** (the file's own lines, soft-wrapped) lives in [`original`].

mod anchor;
mod generate;
pub mod lex;
pub mod original;
mod wrap;

#[cfg(test)]
mod tests;

pub use anchor::{Seek, SeekState, anchor_before, prev_line, seek};
pub use generate::Generator;

use crate::nav::Span;

/// Default row width for long tokens (bytes).
pub const DEFAULT_WRAP: usize = 400;

/// Smallest accepted row width (the break rules need some room).
pub const MIN_WRAP: usize = 16;

/// Options of the pretty printer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrettyOptions {
    /// Bytes per row before a long token is split (clamped to at least [`MIN_WRAP`]).
    pub wrap: usize,
    /// Spaces per level in [`PrettyLine::write_text`].
    pub indent: u8,
}

impl Default for PrettyOptions {
    fn default() -> Self {
        PrettyOptions {
            wrap: DEFAULT_WRAP,
            indent: 2,
        }
    }
}

impl PrettyOptions {
    pub(crate) fn wrap(&self) -> usize {
        self.wrap.max(MIN_WRAP)
    }
}

/// What a token is (for highlighting).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// An object key (a string followed by `:`).
    Key,
    /// A string value.
    String,
    /// A number.
    Number,
    /// `true`, `false` or `null`.
    Literal,
    /// `{ } [ ] : ,`
    Punct,
    /// Anything else (invalid JSON).
    Error,
}

/// One token (or a part of a long one) on a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Token {
    /// Its type.
    pub kind: TokenKind,
    /// Its source bytes.
    pub span: Span,
}

/// The long token a continuation row is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenStart {
    /// Offset of the token's first byte (the `"` of a string).
    pub start: u64,
    /// The token type.
    pub kind: TokenKind,
    /// Offset where the line holding the token start begins.
    pub line: u64,
    /// Depth at that line start.
    pub line_depth: u32,
}

/// A position where a formatted row begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrettyPos {
    /// First source byte of the row (leading whitespace included).
    pub offset: u64,
    /// Nesting depth before the row (a closing bracket row shows one level less).
    pub depth: u32,
    /// `Some` when the row continues a long token.
    pub token: Option<TokenStart>,
}

impl PrettyPos {
    /// The start of the document.
    pub const START: PrettyPos = PrettyPos {
        offset: 0,
        depth: 0,
        token: None,
    };

    /// A line start (not inside a token).
    pub fn line(offset: u64, depth: u32) -> Self {
        PrettyPos {
            offset,
            depth,
            token: None,
        }
    }
}

/// One formatted row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrettyLine {
    /// Where the row starts.
    pub pos: PrettyPos,
    /// Where the next row starts (`next.offset` is one past this row's last byte).
    pub next: PrettyPos,
    /// Indentation level to display.
    pub depth: u32,
    /// The tokens, in order.
    pub tokens: Vec<Token>,
}

impl PrettyLine {
    /// The source bytes this row covers: `pos.offset..next.offset`. The spans of
    /// consecutive rows tile the document.
    pub fn source_span(&self) -> Span {
        Span {
            start: self.pos.offset,
            end: self.next.offset,
        }
    }

    /// `true` if the row continues a token from the previous row.
    pub fn is_continuation(&self) -> bool {
        self.pos.token.is_some()
    }

    /// `true` if the row's last token goes on in the next row.
    pub fn wraps(&self) -> bool {
        self.next.token.is_some()
    }

    /// Appends the row's text (indentation, tokens, `": "`) to `out`, without a newline.
    /// Continuation rows get no indentation, so joining rows with `\n` (and continuation
    /// rows with nothing) gives valid JSON for a valid document.
    pub fn write_text(&self, bytes: &[u8], indent: u8, out: &mut Vec<u8>) {
        if !self.is_continuation() {
            out.extend(std::iter::repeat_n(
                b' ',
                self.depth as usize * indent as usize,
            ));
        }
        for t in &self.tokens {
            out.extend_from_slice(t.span.slice(bytes));
            if t.kind == TokenKind::Punct && t.span.slice(bytes) == b":" {
                out.push(b' ');
            }
        }
    }
}

/// Up to `max_lines` formatted rows starting at `start` (fewer at the end of `bytes`).
///
/// `start` must be a row start: [`PrettyPos::START`], a `next` of an earlier row, or a
/// position from [`anchor_before`] / [`seek`] / [`prev_line`]. Pure and allocation-light:
/// it reads only the bytes of the rows it returns (plus whitespace before them).
///
/// # Example
/// ```
/// use braceray_core::pretty::{pretty_lines, PrettyOptions, PrettyPos};
///
/// let bytes = br#"{"a":[1,2],"b":{}}"#;
/// let opts = PrettyOptions::default();
/// let lines = pretty_lines(bytes, PrettyPos::START, 100, &opts);
/// let text: Vec<String> = lines
///     .iter()
///     .map(|l| {
///         let mut out = Vec::new();
///         l.write_text(bytes, 2, &mut out);
///         String::from_utf8(out).unwrap()
///     })
///     .collect();
/// assert_eq!(text, ["{", r#"  "a": ["#, "    1,", "    2", "  ],", r#"  "b": {}"#, "}"]);
/// ```
pub fn pretty_lines(
    bytes: &[u8],
    start: PrettyPos,
    max_lines: usize,
    opts: &PrettyOptions,
) -> Vec<PrettyLine> {
    Generator::new(bytes, start, opts).take(max_lines).collect()
}
