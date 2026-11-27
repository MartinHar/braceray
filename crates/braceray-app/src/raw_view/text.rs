//! The text of one raw view row and the mapping between its characters and source bytes
//! (for clicks, selections and highlights).

use braceray_core::nav::Span;
use braceray_core::pretty::TokenKind;

use super::{RawMode, RawRow};

/// Display text of a row and the pieces of source it shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowText {
    /// The text.
    pub text: String,
    /// Every piece of source shown, in order.
    pub pieces: Vec<Piece>,
    chars: usize,
}

/// A piece of a row's text that shows source bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    /// First character in [`RowText::text`].
    pub char_start: usize,
    /// Number of characters.
    pub chars: usize,
    /// The source bytes.
    pub span: Span,
    /// Its highlight kind (`None`: plain text in original mode).
    pub kind: Option<TokenKind>,
}

impl RowText {
    fn push(&mut self, bytes: &[u8], span: Span, kind: Option<TokenKind>) {
        let s = String::from_utf8_lossy(span.slice(bytes));
        let n = s.chars().count();
        self.text.push_str(&s);
        self.pieces.push(Piece {
            char_start: self.chars,
            chars: n,
            span,
            kind,
        });
        self.chars += n;
    }

    fn push_plain(&mut self, text: &str) {
        self.text.push_str(text);
        self.chars += text.chars().count();
    }
}

/// The text of `row` in `mode`: pretty rows print their tokens (`": "` after a colon),
/// original rows print their bytes.
pub fn layout(row: &RawRow, bytes: &[u8], mode: RawMode) -> RowText {
    let mut out = RowText::default();
    match mode {
        RawMode::Pretty => {
            for t in &row.tokens {
                out.push(bytes, t.span, Some(t.kind));
                if t.kind == TokenKind::Punct && t.span.slice(bytes) == b":" {
                    out.push_plain(" ");
                }
            }
        }
        RawMode::Original => {
            let mut at = row.text.start;
            for t in &row.tokens {
                if t.span.start > at {
                    out.push(
                        bytes,
                        Span {
                            start: at,
                            end: t.span.start,
                        },
                        None,
                    );
                }
                out.push(bytes, t.span, Some(t.kind));
                at = t.span.end;
            }
            if at < row.text.end {
                out.push(
                    bytes,
                    Span {
                        start: at,
                        end: row.text.end,
                    },
                    None,
                );
            }
        }
    }
    out
}

impl RowText {
    /// Source offset of character `char_index` (the row end for positions past the text).
    pub fn offset_of_char(&self, bytes: &[u8], char_index: usize, row_end: u64) -> u64 {
        for p in &self.pieces {
            if char_index < p.char_start + p.chars {
                let k = char_index.saturating_sub(p.char_start);
                return p.span.start + byte_of_char(p.span.slice(bytes), k) as u64;
            }
            if char_index < p.char_start {
                return p.span.start; // in inserted text (": "): snap to the next piece
            }
        }
        self.pieces.last().map_or(row_end, |p| p.span.end)
    }

    /// Character range of the source span `sel` within this row's text, if it overlaps.
    pub fn chars_of(&self, bytes: &[u8], sel: Span) -> Option<(usize, usize)> {
        let mut first = None;
        let mut last = None;
        for p in &self.pieces {
            let s = sel.start.max(p.span.start);
            let e = sel.end.min(p.span.end);
            if s >= e {
                continue;
            }
            let src = p.span.slice(bytes);
            let a = p.char_start + char_of_byte(src, (s - p.span.start) as usize);
            let b = p.char_start + char_of_byte(src, (e - p.span.start) as usize);
            first.get_or_insert(a);
            last = Some(b);
        }
        Some((first?, last?))
    }
}

/// Byte offset of character `k` in the lossy decoding of `src`.
fn byte_of_char(src: &[u8], k: usize) -> usize {
    let mut chars = 0;
    let mut bytes = 0;
    for chunk in src.utf8_chunks() {
        for (i, _) in chunk.valid().char_indices() {
            if chars == k {
                return bytes + i;
            }
            chars += 1;
        }
        bytes += chunk.valid().len();
        if !chunk.invalid().is_empty() {
            if chars == k {
                return bytes;
            }
            chars += 1;
            bytes += chunk.invalid().len();
        }
    }
    src.len()
}

/// Character index (lossy decoding) of byte `b` in `src`.
fn char_of_byte(src: &[u8], b: usize) -> usize {
    let mut chars = 0;
    let mut bytes = 0;
    for chunk in src.utf8_chunks() {
        let valid = chunk.valid();
        if b < bytes + valid.len() {
            return chars + valid[..b - bytes].chars().count();
        }
        chars += valid.chars().count();
        bytes += valid.len();
        if !chunk.invalid().is_empty() {
            if b < bytes + chunk.invalid().len() {
                return chars;
            }
            chars += 1;
            bytes += chunk.invalid().len();
        }
    }
    chars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_byte_mapping_is_lossy_aware() {
        let src = b"a\xC3\xA9\xFFb";
        // lossy: "aé�b"
        assert_eq!(byte_of_char(src, 0), 0);
        assert_eq!(byte_of_char(src, 1), 1);
        assert_eq!(byte_of_char(src, 2), 3);
        assert_eq!(byte_of_char(src, 3), 4);
        assert_eq!(byte_of_char(src, 4), 5);
        for (b, c) in [(0, 0), (1, 1), (3, 2), (4, 3), (5, 4)] {
            assert_eq!(char_of_byte(src, b), c, "byte {b}");
        }
    }
}
