//! Where long tokens are split into rows. Break `k` of a token starting at `s` is
//! `s + k × wrap`, moved back onto a UTF-8 character boundary and (in strings) off an
//! escape, so it can be computed from the token start alone, going forwards or
//! backwards.

use super::TokenKind;

/// Longest backslash run counted when checking whether a break would split an escape.
const MAX_BACKSLASH_RUN: usize = 4096;

/// Slack for the adjustment (at most 3 bytes for UTF-8 plus 1 for an escape).
const SLACK: usize = 8;

/// `true` for kinds whose bytes are a JSON string (escapes matter).
fn is_string(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::String | TokenKind::Key)
}

/// Break `k` (≥ 1) of the token starting at `start`; `bytes.len()` past the end.
pub(crate) fn token_break(
    bytes: &[u8],
    start: usize,
    kind: TokenKind,
    k: usize,
    wrap: usize,
) -> usize {
    let raw = start.saturating_add(k.saturating_mul(wrap));
    adjust(bytes, start, kind, raw)
}

/// The break after a row of the token that starts at `from` (the token start or a break).
pub(crate) fn next_break(
    bytes: &[u8],
    start: usize,
    kind: TokenKind,
    from: usize,
    wrap: usize,
) -> usize {
    let k = (from - start + SLACK) / wrap + 1;
    token_break(bytes, start, kind, k, wrap)
}

/// Index of the row of the token holding `x` (0: the row with the token start) and the
/// offset where that row starts inside the token (`start` for row 0).
pub(crate) fn row_of(
    bytes: &[u8],
    start: usize,
    kind: TokenKind,
    x: usize,
    wrap: usize,
) -> (usize, usize) {
    // Breaks sit at most SLACK bytes before their raw position.
    let mut k = (x.saturating_sub(start) + SLACK) / wrap;
    while k > 0 {
        let b = token_break(bytes, start, kind, k, wrap);
        if b <= x {
            return (k, b);
        }
        k -= 1;
    }
    (0, start)
}

fn adjust(bytes: &[u8], start: usize, kind: TokenKind, raw: usize) -> usize {
    if raw >= bytes.len() {
        return bytes.len();
    }
    let mut b = raw;
    let mut n = 0;
    while n < 3 && b > start + 1 && bytes[b] & 0xC0 == 0x80 {
        b -= 1;
        n += 1;
    }
    if is_string(kind) {
        let mut run = 0;
        while run < MAX_BACKSLASH_RUN && b - run > start + 1 && bytes[b - run - 1] == b'\\' {
            run += 1;
        }
        if run % 2 == 1 {
            b -= 1;
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breaks_avoid_utf8_and_escapes() {
        // "ééé…": every second byte is a continuation byte.
        let mut s = b"\"".to_vec();
        for _ in 0..100 {
            s.extend_from_slice("é".as_bytes());
        }
        s.push(b'"');
        for k in 1..10 {
            let b = token_break(&s, 0, TokenKind::String, k, 17);
            assert!(s[b] & 0xC0 != 0x80, "break {k} at {b} splits a character");
            assert!((17 * k - b) < SLACK);
        }
        // An escape right before the raw break: the break moves before the backslash.
        let s = br#""abcdefghijklmno\"qrstuvwxyz""#;
        let raw = 17; // byte 16 is the backslash, 17 the escaped quote
        assert_eq!(s[raw - 1], b'\\');
        assert_eq!(token_break(s, 0, TokenKind::String, 1, raw), raw - 1);
        // An even run is not an escape.
        let s = br#""abcdefghijklmn\\qrstuvwxyz""#;
        assert_eq!(token_break(s, 0, TokenKind::String, 1, 17), 17);
        // Numbers don't care about backslashes.
        assert_eq!(token_break(s, 0, TokenKind::Number, 1, 17), 17);
    }

    #[test]
    fn next_and_row_of_agree() {
        let mut s = b"\"".to_vec();
        for i in 0..2000u32 {
            s.extend_from_slice(if i % 7 == 0 {
                "\\n".as_bytes()
            } else {
                "€".as_bytes()
            });
        }
        s.push(b'"');
        let wrap = 23;
        let mut breaks = vec![0];
        let mut from = 0;
        while from < s.len() {
            from = next_break(&s, 0, TokenKind::String, from, wrap);
            breaks.push(from);
        }
        for x in 0..s.len() {
            let (k, b) = row_of(&s, 0, TokenKind::String, x, wrap);
            assert_eq!(b, breaks[k], "x = {x}");
            assert!(b <= x && x < breaks[k + 1], "x = {x}");
        }
    }
}
