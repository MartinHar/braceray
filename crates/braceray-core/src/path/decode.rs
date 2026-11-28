//! Decoding JSON string escapes (keys for paths, "Copy string contents").

use crate::error::Error;

/// Bytes between cancel checks in [`decode_escapes`].
pub(crate) const CHECK_EVERY: usize = 4 << 20;

/// The text of a raw JSON string (quotes included, as in a key span), with escapes
/// decoded. Invalid escapes and bytes are kept as they are (lossy UTF-8).
///
/// # Example
/// ```
/// use braceray_core::path::decode_key;
/// assert_eq!(decode_key(br#""caf\u00e9 \"x\"""#), "café \"x\"");
/// ```
pub fn decode_key(raw: &[u8]) -> String {
    let inner = strip_quotes(raw);
    if !inner.contains(&b'\\') {
        return String::from_utf8_lossy(inner).into_owned();
    }
    let mut out: Vec<u8> = Vec::with_capacity(inner.len());
    // The no-op check never fails.
    let _ = decode_escapes(inner, &mut out, |_| Ok(()));
    String::from_utf8_lossy(&out).into_owned()
}

/// `raw` without one leading and one trailing `"` (each only if present).
pub(crate) fn strip_quotes(raw: &[u8]) -> &[u8] {
    let inner = raw.strip_prefix(b"\"").unwrap_or(raw);
    inner.strip_suffix(b"\"").unwrap_or(inner)
}

/// Appends `inner` (a string body without quotes) to `out` with escapes decoded. Calls
/// `check(bytes done)` about every [`CHECK_EVERY`] input bytes and stops at its error.
pub(crate) fn decode_escapes(
    inner: &[u8],
    out: &mut Vec<u8>,
    mut check: impl FnMut(usize) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut i = 0;
    let mut next_check = CHECK_EVERY;
    while i < inner.len() {
        if i >= next_check {
            check(i)?;
            next_check = i + CHECK_EVERY;
        }
        let window_end = inner.len().min(next_check);
        let Some(rel) = memchr::memchr(b'\\', &inner[i..window_end]) else {
            out.extend_from_slice(&inner[i..window_end]);
            i = window_end;
            continue;
        };
        out.extend_from_slice(&inner[i..i + rel]);
        i += rel;
        if i + 1 >= inner.len() {
            out.push(b'\\');
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
        out.push(b'\\');
        i += 1;
    }
    Ok(())
}

/// Decodes `\uXXXX` (or a surrogate pair `😀`) at the start of `s`.
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
