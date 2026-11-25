//! Cheap checks on the first bytes of a file: BOM, encoding, binary data, the first JSON
//! byte and an NDJSON sniff. All functions are pure and work on byte slices.

use crate::error::{Encoding, Error};

/// Size of the sample used for the NUL-pattern and binary checks.
pub(crate) const ENCODING_SAMPLE: usize = 4 * 1024;

/// Size of the sample used by [`sniff`].
const SNIFF_SAMPLE: usize = 64 * 1024;

/// What kind of JSON a file contains. Refined in step 16 (NDJSON support).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Sniff {
    /// One JSON document.
    Json,
    /// Newline-delimited JSON: one value per line (JSON Lines).
    NdJson,
    /// The content doesn't start with a JSON value.
    Unknown,
}

/// JSON whitespace (RFC 8259): space, tab, line feed, carriage return.
#[inline]
pub(crate) fn is_json_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// Bytes that can start a JSON value.
#[inline]
fn is_value_start(b: u8) -> bool {
    matches!(
        b,
        b'{' | b'[' | b'"' | b'-' | b'0'..=b'9' | b't' | b'f' | b'n'
    )
}

/// Checks the byte-order mark. Returns the number of bytes to skip (3 for a UTF-8 BOM,
/// otherwise 0), or the unsupported encoding the BOM announces.
pub(crate) fn check_bom(bytes: &[u8]) -> Result<usize, Encoding> {
    // UTF-32 LE must be tested before UTF-16 LE: its BOM starts with the same two bytes.
    match bytes {
        [0xFF, 0xFE, 0x00, 0x00, ..] => Err(Encoding::Utf32Le),
        [0x00, 0x00, 0xFE, 0xFF, ..] => Err(Encoding::Utf32Be),
        [0xFF, 0xFE, ..] => Err(Encoding::Utf16Le),
        [0xFE, 0xFF, ..] => Err(Encoding::Utf16Be),
        [0xEF, 0xBB, 0xBF, ..] => Ok(3),
        _ => Ok(0),
    }
}

/// Fewest control bytes that make a sample binary, so that one stray control character in
/// a tiny file is left to the JSON validator (which reports its exact position).
const MIN_BINARY_CONTROL: usize = 8;

/// Detects UTF-16/32 without a BOM (from where the NUL bytes sit) and binary data
/// (more than 1 % NUL or control bytes other than tab / LF / CR, and at least
/// [`MIN_BINARY_CONTROL`] of them).
pub(crate) fn check_sample(sample: &[u8]) -> Result<(), Encoding> {
    // NUL count and byte count for each position modulo 4.
    let mut nul = [0usize; 4];
    let mut total = [0usize; 4];
    let mut control = 0usize;
    for (i, &b) in sample.iter().enumerate() {
        total[i % 4] += 1;
        if b == 0 {
            nul[i % 4] += 1;
        }
        if b < 0x20 && !is_json_ws(b) {
            control += 1;
        }
    }
    if control == 0 {
        return Ok(());
    }

    // `part / whole >= pct / 100`, in integers.
    let at_least = |part: usize, whole: usize, pct: usize| whole > 0 && part * 100 >= whole * pct;
    let at_most = |part: usize, whole: usize, pct: usize| part * 100 <= whole * pct;

    if sample.len() >= 8 {
        // UTF-32: code points are < 0x110000, so the high byte (and nearly always the one
        // next to it) is NUL.
        if at_least(nul[2] + nul[3], total[2] + total[3], 90) && at_most(nul[0], total[0], 10) {
            return Err(Encoding::Utf32Le);
        }
        if at_least(nul[0] + nul[1], total[0] + total[1], 90) && at_most(nul[3], total[3], 10) {
            return Err(Encoding::Utf32Be);
        }
    }
    // UTF-16: ASCII characters have a NUL high byte (odd positions in LE, even in BE).
    let (even_nul, even) = (nul[0] + nul[2], total[0] + total[2]);
    let (odd_nul, odd) = (nul[1] + nul[3], total[1] + total[3]);
    if at_least(odd_nul, odd, 25) && even_nul * 4 <= odd_nul {
        return Err(Encoding::Utf16Le);
    }
    if at_least(even_nul, even, 25) && odd_nul * 4 <= even_nul {
        return Err(Encoding::Utf16Be);
    }
    if control * 100 > sample.len() && control >= MIN_BINARY_CONTROL {
        return Err(Encoding::Binary);
    }
    Ok(())
}

/// Returns the offset of the first non-whitespace byte of `content` and checks that it can
/// start a JSON value.
pub(crate) fn first_value_byte(content: &[u8]) -> Result<usize, Error> {
    if content.is_empty() {
        return Err(Error::EmptyFile);
    }
    let pos = content
        .iter()
        .position(|&b| !is_json_ws(b))
        .ok_or(Error::WhitespaceOnly)?;
    let found = content[pos];
    if is_value_start(found) {
        Ok(pos)
    } else {
        Err(Error::NotJson {
            offset: pos as u64,
            found,
        })
    }
}

/// Guesses whether `content` is one JSON document or NDJSON, from its first 64 KiB.
///
/// NDJSON: the first line is a complete-looking container (`{…}` or `[…]` on one line) and
/// another line starting with `{` or `[` follows. A pretty-printed document never has that
/// shape, because its first line ends with `{` or `[`.
pub(crate) fn sniff(content: &[u8]) -> Sniff {
    let Some(start) = content.iter().position(|&b| !is_json_ws(b)) else {
        return Sniff::Unknown;
    };
    if !is_value_start(content[start]) {
        return Sniff::Unknown;
    }
    let sample = &content[start..content.len().min(start + SNIFF_SAMPLE)];
    let mut lines = sample
        .split(|&b| b == b'\n')
        .map(trim_ws)
        .filter(|l| !l.is_empty());
    let (Some(first), Some(second)) = (lines.next(), lines.next()) else {
        return Sniff::Json;
    };
    let opens = |l: &[u8]| matches!(l.first(), Some(b'{' | b'['));
    let closes = matches!(first.last(), Some(b'}' | b']'));
    if opens(first) && closes && opens(second) {
        Sniff::NdJson
    } else {
        Sniff::Json
    }
}

fn trim_ws(line: &[u8]) -> &[u8] {
    let start = line
        .iter()
        .position(|&b| !is_json_ws(b))
        .unwrap_or(line.len());
    let end = line
        .iter()
        .rposition(|&b| !is_json_ws(b))
        .map_or(start, |e| e + 1);
    &line[start..end.max(start)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }
    fn utf16be(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_be_bytes).collect()
    }
    fn utf32le(s: &str) -> Vec<u8> {
        s.chars().flat_map(|c| (c as u32).to_le_bytes()).collect()
    }
    fn utf32be(s: &str) -> Vec<u8> {
        s.chars().flat_map(|c| (c as u32).to_be_bytes()).collect()
    }
    const TEXT: &str = r#"{"name": "Արամ", "city": "Ереван", "tags": ["日本", "😀"], "n": -1.5e3}"#;

    #[test]
    fn boms() {
        assert_eq!(check_bom(b"\xEF\xBB\xBF{}"), Ok(3));
        assert_eq!(check_bom(b"{}"), Ok(0));
        assert_eq!(check_bom(b""), Ok(0));
        assert_eq!(check_bom(b"\xFF\xFE{\0"), Err(Encoding::Utf16Le));
        assert_eq!(check_bom(b"\xFE\xFF\0{"), Err(Encoding::Utf16Be));
        assert_eq!(check_bom(b"\xFF\xFE\0\0{\0\0\0"), Err(Encoding::Utf32Le));
        assert_eq!(check_bom(b"\0\0\xFE\xFF\0\0\0{"), Err(Encoding::Utf32Be));
    }

    #[test]
    fn nul_patterns_without_bom() {
        assert_eq!(check_sample(&utf16le(TEXT)), Err(Encoding::Utf16Le));
        assert_eq!(check_sample(&utf16be(TEXT)), Err(Encoding::Utf16Be));
        assert_eq!(check_sample(&utf32le(TEXT)), Err(Encoding::Utf32Le));
        assert_eq!(check_sample(&utf32be(TEXT)), Err(Encoding::Utf32Be));
        // Shifted by one byte (odd start) is still recognised as UTF-16 of the other order.
        assert!(check_sample(&utf16le(TEXT)[1..]).is_err());
    }

    #[test]
    fn utf8_passes() {
        assert_eq!(check_sample(TEXT.as_bytes()), Ok(()));
        assert_eq!(check_sample(b"{\r\n\t\"a\": 1\r\n}"), Ok(()));
        assert_eq!(check_sample(b""), Ok(()));
    }

    #[test]
    fn binary() {
        let mut data: Vec<u8> = (0..4096u32).map(|i| (i * 7919 % 251) as u8).collect();
        assert_eq!(check_sample(&data), Err(Encoding::Binary));
        // A single stray control byte in 4 KiB of text is under the 1 % threshold.
        data = TEXT.repeat(60).into_bytes();
        data[100] = 0x01;
        assert_eq!(check_sample(&data), Ok(()));
        // 2 % control bytes is binary.
        for i in (0..data.len()).step_by(50) {
            data[i] = 0x02;
        }
        assert_eq!(check_sample(&data), Err(Encoding::Binary));
        // A short file with one raw control character is left to the validator.
        assert_eq!(check_sample(b"[\"a\x01b\"]"), Ok(()));
        assert_eq!(check_sample(&[0x01; 8]), Err(Encoding::Binary));
    }

    #[test]
    fn first_byte() {
        assert_eq!(first_value_byte(b"  \n\t{}").unwrap(), 4);
        for ok in [
            &b"[]"[..],
            b"\"s\"",
            b"-1",
            b"0",
            b"9",
            b"true",
            b"false",
            b"null",
        ] {
            assert_eq!(first_value_byte(ok).unwrap(), 0);
        }
        assert!(matches!(first_value_byte(b""), Err(Error::EmptyFile)));
        assert!(matches!(
            first_value_byte(b" \r\n\t "),
            Err(Error::WhitespaceOnly)
        ));
        assert!(matches!(
            first_value_byte(b"  <html>"),
            Err(Error::NotJson {
                offset: 2,
                found: b'<'
            })
        ));
        // Form feed and NBSP are not JSON whitespace.
        assert!(matches!(
            first_value_byte(b"\x0c{}"),
            Err(Error::NotJson { offset: 0, .. })
        ));
    }

    #[test]
    fn sniffing() {
        assert_eq!(sniff(b"{\"a\":1}"), Sniff::Json);
        assert_eq!(sniff(b"[\n  1,\n  2\n]\n"), Sniff::Json);
        assert_eq!(sniff(b"{\n  \"a\": {}\n}\n"), Sniff::Json);
        assert_eq!(sniff(b"{\"a\":1}\n{\"a\":2}\n"), Sniff::NdJson);
        assert_eq!(sniff(b"\n[1,2]\r\n\r\n[3]\r\n"), Sniff::NdJson);
        assert_eq!(sniff(b"   "), Sniff::Unknown);
        assert_eq!(sniff(b"<xml/>"), Sniff::Unknown);
        assert_eq!(sniff(b"42"), Sniff::Json);
    }

    #[test]
    fn trim() {
        assert_eq!(trim_ws(b"  a b \r"), b"a b");
        assert_eq!(trim_ws(b" \r "), b"");
        assert_eq!(trim_ws(b""), b"");
    }
}
