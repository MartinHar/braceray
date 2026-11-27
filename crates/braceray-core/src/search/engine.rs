//! Match engines: which finder runs for a query, whole-word filtering and the longest
//! possible match (architecture.md §4).

use aho_corasick::{AhoCorasick, Input, MatchKind};
use memchr::memmem;
use regex::bytes::{Regex, RegexBuilder};

use crate::error::Error;

/// The finder chosen for a query.
enum Finder {
    /// Case-sensitive literal (or a literal without ASCII letters).
    Exact(memmem::Finder<'static>),
    /// ASCII query, ASCII case-insensitive.
    AsciiCi(AhoCorasick),
    /// Non-ASCII query, Unicode case-insensitive (simple case folding).
    Unicode(Regex),
}

/// A compiled query: finds the leftmost acceptable match starting at or after a position.
pub(crate) struct Engine {
    finder: Finder,
    whole_word: bool,
    /// Upper bound of a match length in bytes (chunks overlap by `max_len - 1`).
    max_len: usize,
}

/// `true` for bytes that belong to a word: ASCII letters and digits, `_`, and every
/// byte of a multi-byte UTF-8 character.
pub(crate) fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

impl Engine {
    /// Compiles `text`. `None` for an empty query (nothing to search).
    pub(crate) fn new(
        text: &str,
        case_sensitive: bool,
        whole_word: bool,
    ) -> Result<Option<Engine>, Error> {
        if text.is_empty() {
            return Ok(None);
        }
        let needle = text.as_bytes();
        let has_ascii_letters = needle.iter().any(u8::is_ascii_alphabetic);
        let (finder, max_len) = if case_sensitive || (text.is_ascii() && !has_ascii_letters) {
            let finder = memmem::Finder::new(needle).into_owned();
            (Finder::Exact(finder), needle.len())
        } else if text.is_ascii() {
            let ac = AhoCorasick::builder()
                .ascii_case_insensitive(true)
                .match_kind(MatchKind::LeftmostFirst)
                .build([needle])
                .map_err(|e| Error::SearchQuery {
                    message: e.to_string(),
                })?;
            (Finder::AsciiCi(ac), needle.len())
        } else {
            let regex = RegexBuilder::new(&unicode_ci_pattern(text))
                .case_insensitive(true)
                .unicode(true)
                .build()
                .map_err(|e| Error::SearchQuery {
                    message: e.to_string(),
                })?;
            // Every query character matches one character (≤ 4 bytes) or, for the
            // Armenian ligature rule, two query characters match ≤ 4 bytes.
            (Finder::Unicode(regex), 4 * text.chars().count())
        };
        Ok(Some(Engine {
            finder,
            whole_word,
            max_len,
        }))
    }

    /// Upper bound of a match length in bytes (≥ 1).
    pub(crate) fn max_len(&self) -> usize {
        self.max_len
    }

    /// The leftmost raw match `(start, end)` with `start >= from` and `end <= end_limit`.
    fn find_raw(&self, hay: &[u8], from: usize, end_limit: usize) -> Option<(usize, usize)> {
        if from >= end_limit {
            return None;
        }
        match &self.finder {
            Finder::Exact(f) => f
                .find(&hay[from..end_limit])
                .map(|i| (from + i, from + i + f.needle().len())),
            Finder::AsciiCi(ac) => ac
                .find(Input::new(&hay[..end_limit]).span(from..end_limit))
                .map(|m| (m.start(), m.end())),
            Finder::Unicode(re) => re
                .find_at(&hay[..end_limit], from)
                .map(|m| (m.start(), m.end())),
        }
    }

    /// The leftmost *acceptable* match `(start, end)` with `start >= from` that ends at or
    /// before `end_limit`. With whole word on, a match whose first (last) byte is a word
    /// byte must not have a word byte right before (after) it; a rejected candidate
    /// doesn't consume anything, so the search goes on at `start + 1`. The word check
    /// looks at `hay` beyond `end_limit` when needed.
    pub(crate) fn find(&self, hay: &[u8], from: usize, end_limit: usize) -> Option<(usize, usize)> {
        let mut pos = from;
        loop {
            let (s, e) = self.find_raw(hay, pos, end_limit)?;
            if !self.whole_word || is_whole_word(hay, s, e) {
                return Some((s, e));
            }
            pos = s + 1;
        }
    }
}

/// Word-boundary check for the match `hay[s..e]` (see [`Engine::find`]).
fn is_whole_word(hay: &[u8], s: usize, e: usize) -> bool {
    let left_ok = !is_word_byte(hay[s]) || s == 0 || !is_word_byte(hay[s - 1]);
    let right_ok = !is_word_byte(hay[e - 1]) || e >= hay.len() || !is_word_byte(hay[e]);
    left_ok && right_ok
}

/// The regex for a Unicode case-insensitive literal search. Characters are escaped one by
/// one; the Armenian ligature `և` (U+0587, no uppercase form) and the letter pairs it
/// stands for (`եւ`, and `եվ` in reformed spelling, as typed in capitals: `ԵՎ`) match
/// each other, so `ԱՐԵՎ` finds `արև`.
fn unicode_ci_pattern(text: &str) -> String {
    const LIGATURE: &str = "(?:և|եվ|եւ)";
    let chars: Vec<char> = text.chars().collect();
    let mut pattern = String::with_capacity(text.len() * 2);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if matches!(c, 'ե' | 'Ե') && matches!(next, Some('վ' | 'Վ' | 'ւ' | 'Ւ')) {
            pattern.push_str(LIGATURE);
            i += 2;
        } else if c == 'և' {
            pattern.push_str(LIGATURE);
            i += 1;
        } else {
            let mut buf = [0u8; 4];
            pattern.push_str(&regex::escape(c.encode_utf8(&mut buf)));
            i += 1;
        }
    }
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(engine: &Engine, hay: &[u8]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut pos = 0;
        while let Some((s, e)) = engine.find(hay, pos, hay.len()) {
            out.push((s, e));
            pos = e;
        }
        out
    }

    fn engine(text: &str, cs: bool, ww: bool) -> Engine {
        Engine::new(text, cs, ww).unwrap().unwrap()
    }

    #[test]
    fn empty_query_has_no_engine() {
        assert!(Engine::new("", false, false).unwrap().is_none());
    }

    #[test]
    fn picks_the_engine() {
        assert!(matches!(
            engine("abc", true, false).finder,
            Finder::Exact(_)
        ));
        assert!(matches!(
            engine("123", false, false).finder,
            Finder::Exact(_)
        ));
        assert!(matches!(
            engine("abc", false, false).finder,
            Finder::AsciiCi(_)
        ));
        assert!(matches!(
            engine("ёлка", false, false).finder,
            Finder::Unicode(_)
        ));
        assert!(matches!(
            engine("ёлка", true, false).finder,
            Finder::Exact(_)
        ));
    }

    #[test]
    fn non_overlapping_leftmost_first() {
        assert_eq!(all(&engine("aa", true, false), b"aaaaa"), [(0, 2), (2, 4)]);
        assert_eq!(all(&engine("AA", false, false), b"aAaaa"), [(0, 2), (2, 4)]);
    }

    #[test]
    fn case_insensitive_ascii_and_unicode() {
        assert_eq!(
            all(&engine("NeEdLe", false, false), b"xneedle NEEDLE").len(),
            2
        );
        let hay = "ёлка ЁЛКА Ёлка".as_bytes();
        assert_eq!(all(&engine("ЁЛКА", false, false), hay).len(), 3);
        assert_eq!(all(&engine("ЁЛКА", true, false), hay).len(), 1);
    }

    #[test]
    fn armenian_ligature() {
        let hay = "արև Արեվ ԱՐԵՒ".as_bytes();
        assert_eq!(all(&engine("ԱՐԵՎ", false, false), hay).len(), 3);
        assert_eq!(all(&engine("արև", false, false), hay).len(), 3);
        assert_eq!(
            all(&engine("և", false, false), "և եվ եւ ե".as_bytes()).len(),
            3
        );
    }

    #[test]
    fn whole_word() {
        let e = engine("cat", true, true);
        assert_eq!(
            all(&e, b"cat concat cats cat_ [cat] cat"),
            [(0, 3), (22, 25), (27, 30)]
        );
        assert_eq!(all(&e, b"cat"), [(0, 3)]);
        // Non-word edges don't need a boundary.
        let e = engine("\"id\"", true, true);
        assert_eq!(all(&e, b"x\"id\"y"), [(1, 5)]);
        // A byte >= 0x80 is a word byte.
        let e = engine("ёлка", false, true);
        assert_eq!(all(&e, "ёлкаы ёлка".as_bytes()).len(), 1);
        // A rejected candidate doesn't consume: "a a" at 1 is rejected ('x' before it),
        // the overlapping one at 3 is found.
        let e = engine("a a", true, true);
        assert_eq!(all(&e, b"xa a a"), [(3, 6)]);
    }

    #[test]
    fn find_respects_the_end_limit_but_checks_words_beyond_it() {
        let e = engine("cat", true, true);
        assert_eq!(e.find(b"cats", 0, 3), None);
        assert_eq!(e.find(b"cat ", 0, 3), Some((0, 3)));
        assert_eq!(e.find(b"cat", 1, 3), None);
    }

    #[test]
    fn regex_metacharacters_are_literal() {
        let e = engine("a.b(ё)*", false, false);
        assert_eq!(all(&e, "axb a.b(Ё)*".as_bytes()), [(4, 12)]);
    }
}
