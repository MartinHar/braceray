//! The row generator: a small state machine over the bytes (offset + depth), so it can
//! start anywhere a row starts and needs no stack.

use super::wrap::next_break;
use super::{PrettyLine, PrettyOptions, PrettyPos, Token, TokenKind, TokenStart};
use crate::document::is_json_ws;
use crate::nav::Span;

/// A row never holds more tokens than this (only reachable in invalid JSON).
const MAX_TOKENS: usize = 16;

/// Formats rows one at a time from a [`PrettyPos`]. See the [module docs](super).
///
/// # Example
/// ```
/// use braceray_core::pretty::{Generator, PrettyOptions, PrettyPos};
///
/// let bytes = b"[1, [], 2]";
/// let rows: Vec<_> = Generator::new(bytes, PrettyPos::START, &PrettyOptions::default()).collect();
/// assert_eq!(rows.len(), 5); // "[", "1,", "[],", "2", "]"
/// assert_eq!(rows[3].depth, 1);
/// assert_eq!(rows[4].depth, 0);
/// assert_eq!(rows[4].next.offset, bytes.len() as u64);
/// ```
#[derive(Debug, Clone)]
pub struct Generator<'a> {
    bytes: &'a [u8],
    pos: PrettyPos,
    wrap: usize,
}

/// One token being emitted.
struct Part {
    kind: TokenKind,
    end: usize,
    finished: bool,
}

impl<'a> Generator<'a> {
    /// A generator starting at `start` (see [`pretty_lines`](super::pretty_lines)).
    pub fn new(bytes: &'a [u8], start: PrettyPos, opts: &PrettyOptions) -> Self {
        Generator {
            bytes,
            pos: start,
            wrap: opts.wrap(),
        }
    }

    /// Where the next row starts.
    pub fn pos(&self) -> PrettyPos {
        self.pos
    }

    /// Moves to another row start.
    pub fn set_pos(&mut self, pos: PrettyPos) {
        self.pos = pos;
    }

    fn skip_ws(&self, mut p: usize) -> usize {
        while p < self.bytes.len() && is_json_ws(self.bytes[p]) {
            p += 1;
        }
        p
    }

    /// Emits (part of) the token `t` from `from`, up to its end or its next break.
    fn part(&self, t: &TokenStart, from: usize) -> Part {
        let bytes = self.bytes;
        let start = t.start as usize;
        let limit = next_break(bytes, start, t.kind, from, self.wrap).min(bytes.len());
        let end = match t.kind {
            TokenKind::String | TokenKind::Key => {
                string_close(bytes, if from == start { from + 1 } else { from }, limit)
            }
            // A terminator right at the break still ends the scalar in this row (the
            // terminator isn't part of it, so the next row would be empty).
            _ => scalar_close(bytes, from, (limit + 1).min(bytes.len())),
        };
        match end {
            Some(e) => Part {
                kind: t.kind,
                end: e,
                finished: true,
            },
            None => Part {
                kind: t.kind,
                end: limit,
                finished: limit >= bytes.len(),
            },
        }
    }

    /// The next row, or `None` at the end of the bytes.
    pub fn next_line(&mut self) -> Option<PrettyLine> {
        let bytes = self.bytes;
        let end = bytes.len();
        let start = self.pos;
        let (origin, origin_depth) = match start.token {
            Some(t) => (t.line, t.line_depth),
            None => (start.offset, start.depth),
        };
        let mut p = (start.offset as usize).min(end);
        let mut depth = start.depth;
        let mut shown_depth = depth;
        let mut tokens: Vec<Token> = Vec::with_capacity(4);
        // After a complete value: another value would need a separator first.
        let mut after_value = false;

        let push = |tokens: &mut Vec<Token>, kind, s: usize, e: usize| {
            tokens.push(Token {
                kind,
                span: Span::new(s, e),
            });
        };

        if let Some(t) = start.token {
            let part = self.part(&t, p);
            push(&mut tokens, part.kind, p, part.end);
            if !part.finished {
                return Some(self.finish(start, shown_depth, tokens, part.end, depth, Some(t)));
            }
            p = part.end;
            after_value = !matches!(t.kind, TokenKind::Key);
        }

        loop {
            let q = self.skip_ws(p);
            if q >= end {
                if tokens.is_empty() {
                    self.pos = PrettyPos::line(end as u64, depth);
                    return None;
                }
                p = q;
                break;
            }
            if tokens.len() >= MAX_TOKENS {
                break;
            }
            let b = bytes[q];
            match b {
                b'{' | b'[' => {
                    if after_value {
                        break;
                    }
                    push(&mut tokens, TokenKind::Punct, q, q + 1);
                    let r = self.skip_ws(q + 1);
                    if r < end && matches!(bytes[r], b'}' | b']') {
                        push(&mut tokens, TokenKind::Punct, r, r + 1);
                        p = r + 1;
                        after_value = true;
                        continue;
                    }
                    p = q + 1;
                    depth += 1;
                    break;
                }
                b'}' | b']' => {
                    if !tokens.is_empty() {
                        p = q;
                        break;
                    }
                    depth = depth.saturating_sub(1);
                    shown_depth = depth;
                    push(&mut tokens, TokenKind::Punct, q, q + 1);
                    p = q + 1;
                    after_value = true;
                }
                b',' => {
                    push(&mut tokens, TokenKind::Punct, q, q + 1);
                    p = q + 1;
                    break;
                }
                b':' => {
                    if let Some(last) = tokens.last_mut()
                        && last.kind == TokenKind::String
                    {
                        last.kind = TokenKind::Key;
                    }
                    push(&mut tokens, TokenKind::Punct, q, q + 1);
                    p = q + 1;
                    after_value = false;
                }
                _ => {
                    if after_value {
                        p = q;
                        break;
                    }
                    let kind = match b {
                        b'"' => TokenKind::String,
                        b'-' | b'0'..=b'9' => TokenKind::Number,
                        b't' | b'f' | b'n' => TokenKind::Literal,
                        _ => TokenKind::Error,
                    };
                    let t = TokenStart {
                        start: q as u64,
                        kind,
                        line: origin,
                        line_depth: origin_depth,
                    };
                    let part = self.part(&t, q);
                    let kind = match part.kind {
                        TokenKind::Literal
                            if !matches!(&bytes[q..part.end], b"true" | b"false" | b"null") =>
                        {
                            TokenKind::Error
                        }
                        k => k,
                    };
                    // Always advance by at least one character (defensive: every byte
                    // that ends a scalar at once is handled by an arm above).
                    let e = if part.end > q {
                        part.end
                    } else {
                        char_end(bytes, q)
                    };
                    push(&mut tokens, kind, q, e);
                    if !part.finished {
                        let t = TokenStart { kind, ..t };
                        return Some(self.finish(start, shown_depth, tokens, e, depth, Some(t)));
                    }
                    p = e;
                    after_value = true;
                }
            }
        }
        Some(self.finish(start, shown_depth, tokens, p, depth, None))
    }

    fn finish(
        &mut self,
        start: PrettyPos,
        shown_depth: u32,
        tokens: Vec<Token>,
        next: usize,
        depth: u32,
        token: Option<TokenStart>,
    ) -> PrettyLine {
        self.pos = PrettyPos {
            offset: next as u64,
            depth,
            token,
        };
        PrettyLine {
            pos: start,
            next: self.pos,
            depth: shown_depth,
            tokens,
        }
    }
}

impl Iterator for Generator<'_> {
    type Item = PrettyLine;

    fn next(&mut self) -> Option<PrettyLine> {
        self.next_line()
    }
}

/// Offset after the closing quote of a string whose body continues at `from`, if it
/// closes before `limit`. `from` is never right after an unpaired backslash.
pub(crate) fn string_close(bytes: &[u8], mut from: usize, limit: usize) -> Option<usize> {
    while from < limit {
        let rel = memchr::memchr2(b'"', b'\\', &bytes[from..limit])?;
        let i = from + rel;
        if bytes[i] == b'"' {
            return Some(i + 1);
        }
        from = i + 2;
    }
    None
}

/// End of the scalar token at `from` if it ends before `limit`.
pub(crate) fn scalar_close(bytes: &[u8], from: usize, limit: usize) -> Option<usize> {
    bytes[from..limit]
        .iter()
        .position(|&b| {
            matches!(
                b,
                b' ' | b'\t' | b'\n' | b'\r' | b'{' | b'}' | b'[' | b']' | b':' | b',' | b'"'
            )
        })
        .map(|p| from + p)
}

/// Offset after the UTF-8 character starting at `p` (at least one byte).
fn char_end(bytes: &[u8], p: usize) -> usize {
    let mut e = p + 1;
    while e < bytes.len() && e < p + 4 && bytes[e] & 0xC0 == 0x80 {
        e += 1;
    }
    e
}
