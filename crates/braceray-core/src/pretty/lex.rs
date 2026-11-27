//! Highlighting tokenizer for a slice of source bytes (original-mode rows), resumable
//! across rows through [`LexState`].

use super::{PrettyLine, Token, TokenKind};
use crate::nav::Span;

/// How far past a string's closing quote to look for `:` (to tell keys from values).
const KEY_LOOKAHEAD: usize = 64;

/// Lexer state at a row boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct LexState {
    /// Inside a string.
    pub in_string: bool,
    /// The previous byte was an unpaired backslash inside a string.
    pub escaped: bool,
}

/// Tokens of `bytes[span]`, starting in `state`; returns the state after the span.
/// Whitespace gets no token. Token spans never leave `span`.
///
/// # Example
/// ```
/// use braceray_core::nav::Span;
/// use braceray_core::pretty::lex::{tokenize, LexState};
/// use braceray_core::pretty::TokenKind;
///
/// let bytes = br#"{"k": [1, true, "v\""]}"#;
/// let mut tokens = Vec::new();
/// let end = tokenize(bytes, Span { start: 0, end: bytes.len() as u64 }, LexState::default(), &mut tokens);
/// let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
/// use TokenKind::*;
/// assert_eq!(kinds, [Punct, Key, Punct, Punct, Number, Punct, Literal, Punct, String, Punct, Punct]);
/// assert_eq!(end, LexState::default());
/// ```
pub fn tokenize(bytes: &[u8], span: Span, state: LexState, out: &mut Vec<Token>) -> LexState {
    let end = (span.end as usize).min(bytes.len());
    let mut p = (span.start as usize).min(end);
    let push = |out: &mut Vec<Token>, kind, s: usize, e: usize| {
        out.push(Token {
            kind,
            span: Span::new(s, e),
        });
    };
    if state.in_string {
        let (e, closed, escaped) = string_rest(bytes, p, end, state.escaped);
        push(out, string_kind(bytes, e, closed), p, e);
        if !closed {
            return LexState {
                in_string: true,
                escaped,
            };
        }
        p = e;
    }
    while p < end {
        let b = bytes[p];
        match b {
            b' ' | b'\t' | b'\n' | b'\r' => p += 1,
            b'{' | b'}' | b'[' | b']' | b':' | b',' => {
                push(out, TokenKind::Punct, p, p + 1);
                p += 1;
            }
            b'"' => {
                let (e, closed, escaped) = string_rest(bytes, p + 1, end, false);
                push(out, string_kind(bytes, e, closed), p, e);
                if !closed {
                    return LexState {
                        in_string: true,
                        escaped,
                    };
                }
                p = e;
            }
            _ => {
                let e = bytes[p..end]
                    .iter()
                    .position(|&c| {
                        matches!(
                            c,
                            b' ' | b'\t'
                                | b'\n'
                                | b'\r'
                                | b'{'
                                | b'}'
                                | b'['
                                | b']'
                                | b':'
                                | b','
                                | b'"'
                        )
                    })
                    .map_or(end, |q| p + q);
                let kind = match &bytes[p..e] {
                    b"true" | b"false" | b"null" => TokenKind::Literal,
                    [b'-' | b'0'..=b'9', ..] => TokenKind::Number,
                    _ => TokenKind::Error,
                };
                push(out, kind, p, e);
                p = e;
            }
        }
    }
    LexState::default()
}

/// Scans a string body from `p`: (end of the scanned part, closed?, escape pending?).
fn string_rest(bytes: &[u8], mut p: usize, end: usize, mut escaped: bool) -> (usize, bool, bool) {
    if escaped && p < end {
        p += 1;
        escaped = false;
    }
    while p < end {
        let Some(rel) = memchr::memchr2(b'"', b'\\', &bytes[p..end]) else {
            return (end, false, false);
        };
        let i = p + rel;
        if bytes[i] == b'"' {
            return (i + 1, true, false);
        }
        if i + 1 >= end {
            return (end, false, true);
        }
        p = i + 2;
    }
    (end.max(p).min(end), false, escaped)
}

/// A closed string followed by `:` is a key.
fn string_kind(bytes: &[u8], after: usize, closed: bool) -> TokenKind {
    if closed {
        let tail = &bytes[after..(after + KEY_LOOKAHEAD).min(bytes.len())];
        if let Some(&c) = tail
            .iter()
            .find(|c| !matches!(c, b' ' | b'\t' | b'\n' | b'\r'))
            && c == b':'
        {
            return TokenKind::Key;
        }
    }
    TokenKind::String
}

/// The lexer state at `offset`, given the pretty row whose source span holds it.
pub fn state_in_row(bytes: &[u8], row: &PrettyLine, offset: u64) -> LexState {
    for t in &row.tokens {
        let is_str = matches!(t.kind, TokenKind::String | TokenKind::Key);
        if !is_str || offset >= t.span.end || offset < t.span.start {
            continue;
        }
        // Inside the token. At its first byte: outside, unless the row continues it.
        if offset == t.span.start && !(row.is_continuation() && t == &row.tokens[0]) {
            return LexState::default();
        }
        let body_start = if row.is_continuation() && t == &row.tokens[0] {
            t.span.start as usize
        } else {
            t.span.start as usize + 1
        };
        let run = bytes[body_start..offset as usize]
            .iter()
            .rev()
            .take_while(|&&b| b == b'\\')
            .count();
        // A closed string's closing quote is inside the span: offset at it is still in.
        return LexState {
            in_string: true,
            escaped: run % 2 == 1,
        };
    }
    LexState::default()
}
