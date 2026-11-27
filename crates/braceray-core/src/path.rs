//! Text forms of a [`PathSegment`] path. Step 10 needs JSONPath for the results list;
//! step 11 adds jq and JavaScript.

use std::fmt::Write as _;

use crate::nav::PathSegment;

/// The JSONPath of `path` (keys are spans into `bytes`): `$.orders[3].customer.email`.
/// Keys that aren't plain identifiers use the bracket form: `$['first name']`.
///
/// # Example
/// ```
/// use braceray_core::nav::{PathSegment, Span};
/// use braceray_core::path::jsonpath;
///
/// let bytes = br#"{"a b": [{"id": 1}]}"#;
/// let key = |s: u64, e: u64| PathSegment::Key(Span { start: s, end: e });
/// let path = [key(1, 6), PathSegment::Index(0), key(10, 14)];
/// assert_eq!(jsonpath(bytes, &path), "$['a b'][0].id");
/// ```
pub fn jsonpath(bytes: &[u8], path: &[PathSegment]) -> String {
    let mut out = String::from("$");
    for seg in path {
        match seg {
            PathSegment::Index(i) => {
                let _ = write!(out, "[{i}]");
            }
            PathSegment::Key(span) => {
                let name = decode_key(span.slice(bytes));
                if is_identifier(&name) {
                    out.push('.');
                    out.push_str(&name);
                } else {
                    out.push_str("['");
                    for c in name.chars() {
                        match c {
                            '\'' => out.push_str("\\'"),
                            '\\' => out.push_str("\\\\"),
                            c if c.is_control() => {
                                let _ = write!(out, "\\u{:04x}", c as u32);
                            }
                            c => out.push(c),
                        }
                    }
                    out.push_str("']");
                }
            }
        }
    }
    out
}

/// `true` for names JSONPath's dot form accepts unquoted (ASCII letters, digits, `_`,
/// not starting with a digit).
fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The text of a raw JSON string (quotes included, as in a key span), with escapes
/// decoded. Invalid escapes and bytes are kept as they are (lossy UTF-8).
///
/// # Example
/// ```
/// use braceray_core::path::decode_key;
/// assert_eq!(decode_key(br#""caf\u00e9 \"x\"""#), "café \"x\"");
/// ```
pub fn decode_key(raw: &[u8]) -> String {
    let inner = raw.strip_prefix(b"\"").unwrap_or(raw);
    let inner = inner.strip_suffix(b"\"").unwrap_or(inner);
    if !inner.contains(&b'\\') {
        return String::from_utf8_lossy(inner).into_owned();
    }
    let mut out: Vec<u8> = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        let b = inner[i];
        if b != b'\\' || i + 1 >= inner.len() {
            out.push(b);
            i += 1;
            continue;
        }
        let simple = match inner[i + 1] {
            b'"' => Some(b'"'),
            b'\\' => Some(b'\\'),
            b'/' => Some(b'/'),
            b'b' => Some(0x08),
            b'f' => Some(0x0c),
            b'n' => Some(b'\n'),
            b'r' => Some(b'\r'),
            b't' => Some(b'\t'),
            _ => None,
        };
        if let Some(c) = simple {
            out.push(c);
            i += 2;
            continue;
        }
        if inner[i + 1] == b'u'
            && let Some((c, used)) = unicode_escape(&inner[i..])
        {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            i += used;
            continue;
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Decodes `\uXXXX` (or a surrogate pair `\uD83D\uDE00`) at the start of `s`.
fn unicode_escape(s: &[u8]) -> Option<(char, usize)> {
    let hex = |s: &[u8]| -> Option<u32> {
        let t = std::str::from_utf8(s.get(2..6)?).ok()?;
        u32::from_str_radix(t, 16).ok()
    };
    let hi = hex(s)?;
    if (0xD800..0xDC00).contains(&hi) {
        if s.get(6..8) == Some(b"\\u")
            && let Some(lo) = hex(&s[6..])
            && (0xDC00..0xE000).contains(&lo)
        {
            let c = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
            return Some((char::from_u32(c)?, 12));
        }
        return Some(('\u{FFFD}', 6));
    }
    if (0xDC00..0xE000).contains(&hi) {
        return Some(('\u{FFFD}', 6));
    }
    Some((char::from_u32(hi)?, 6))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav::Span;

    fn key(bytes: &[u8], name: &str) -> PathSegment {
        let quoted = format!("\"{name}\"");
        let s = bytes
            .windows(quoted.len())
            .position(|w| w == quoted.as_bytes())
            .unwrap() as u64;
        PathSegment::Key(Span {
            start: s,
            end: s + quoted.len() as u64,
        })
    }

    #[test]
    fn identifiers_use_dots_others_brackets() {
        let bytes = br#"{"a": {"_b2": {"2x": {"it's": {"a\\b": {"": {"\u00e9": 0}}}}}}}"#;
        let path = [
            key(bytes, "a"),
            key(bytes, "_b2"),
            key(bytes, "2x"),
            key(bytes, "it's"),
            key(bytes, "a\\\\b"),
            key(bytes, ""),
            key(bytes, "\\u00e9"),
            PathSegment::Index(7),
        ];
        assert_eq!(
            jsonpath(bytes, &path),
            r"$.a._b2['2x']['it\'s']['a\\b']['']['é'][7]"
        );
        assert_eq!(jsonpath(bytes, &[]), "$");
    }

    #[test]
    fn decoding_escapes() {
        assert_eq!(decode_key(br#""a\nb\t\/""#), "a\nb\t/");
        assert_eq!(decode_key(br#""\ud83d\ude00""#), "😀");
        assert_eq!(decode_key(br#""\ud83d x""#), "\u{FFFD} x");
        assert_eq!(decode_key(br#""\q\u12""#), "\\q\\u12");
        assert_eq!(decode_key(b"\"tail\\\""), "tail\\");
        assert_eq!(
            jsonpath(
                b"\"\\u0001\"",
                &[PathSegment::Key(Span { start: 0, end: 8 })]
            ),
            "$['\\u0001']"
        );
    }
}
