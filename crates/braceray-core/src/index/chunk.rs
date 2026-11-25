//! Per-chunk work of the parallel builder (phases A and C of architecture.md §3.2).
//!
//! Phase A ([`summarize`]) scans a chunk assuming it starts outside a string with no
//! pending escape. [`start_states`] then fixes every chunk's real start state from these
//! summaries. Phase C ([`tokenize`]) re-scans each chunk from its real state and emits
//! one `u32` per token: kind in the top 4 bits, offset (relative to the chunk) below.

use super::simd::{self, RawMasks, ScanState, Simd};
use crate::job::{CancelToken, Progress};

/// Largest chunk the 28-bit token offset can address.
pub(crate) const MAX_CHUNK: usize = 1 << 28;
const OFFSET_MASK: u32 = (1 << 28) - 1;
/// Blocks between cancellation checks (1 MiB).
const CHECK_BLOCKS: usize = (1 << 20) / 64;

/// A token as classified by a SIMD worker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Tok {
    OpenObject,
    OpenArray,
    CloseObject,
    CloseArray,
    Colon,
    Comma,
    /// A string with no backslash or control byte, closed in the same chunk.
    CleanString,
    /// A scalar run that is exactly `true`, `false`, `null` or a valid number.
    Scalar,
    /// A clean string directly followed by `:` (merged, to save one token per member).
    Key,
}

const K_COLON: u32 = 4;
const K_CLEAN_STRING: u32 = 6;
const K_SUSPICIOUS: u32 = 8;
const K_KEY: u32 = 9;

/// Decodes a token: `(Some(kind) | None for "needs the reference parser", offset)`.
#[inline]
pub(crate) fn decode(t: u32) -> (Option<Tok>, usize) {
    let tok = match t >> 28 {
        0 => Some(Tok::OpenObject),
        1 => Some(Tok::OpenArray),
        2 => Some(Tok::CloseObject),
        3 => Some(Tok::CloseArray),
        4 => Some(Tok::Colon),
        5 => Some(Tok::Comma),
        6 => Some(Tok::CleanString),
        7 => Some(Tok::Scalar),
        9 => Some(Tok::Key),
        _ => None,
    };
    (tok, (t & OFFSET_MASK) as usize)
}

#[inline]
fn encode(kind: u32, rel: usize) -> u32 {
    (kind << 28) | rel as u32
}

/// Phase A result for one chunk.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Summary {
    len: usize,
    /// Escape carry out of the chunk, assuming no carry in.
    escape_carry: u64,
    /// Parity of unescaped quotes, assuming no carry in.
    quote_parity: bool,
    /// Offset of the first byte that isn't a backslash (`len` if there is none).
    lead_end: usize,
    /// Whether that byte is a quote.
    lead_is_quote: bool,
}

/// Phase A. `None` if cancelled.
pub(crate) fn summarize(
    simd: &Simd,
    chunk: &[u8],
    cancel: &CancelToken,
    progress: &Progress,
) -> Option<Summary> {
    let mut carry = 0u64;
    let mut quotes = 0u32;
    let (blocks, rest) = chunk.as_chunks::<64>();
    for (i, block) in blocks.iter().enumerate() {
        if i % CHECK_BLOCKS == CHECK_BLOCKS - 1 && cancel.is_cancelled() {
            return None;
        }
        let raw = simd.classify(block);
        let escaped = simd::escaped_mask(raw.backslash, &mut carry);
        quotes ^= (raw.quote & !escaped).count_ones() & 1;
    }
    if !rest.is_empty() {
        let raw = classify_partial(simd, rest);
        let escaped = simd::escaped_mask(raw.backslash, &mut carry);
        quotes ^= (raw.quote & !escaped).count_ones() & 1;
        // The carry belongs to the byte after the last real one, not after bit 63.
        carry = (escaped >> rest.len()) & 1;
    }
    progress.add(chunk.len() as u64);
    let lead_end = chunk
        .iter()
        .position(|&b| b != b'\\')
        .unwrap_or(chunk.len());
    Some(Summary {
        len: chunk.len(),
        escape_carry: carry,
        quote_parity: quotes == 1,
        lead_end,
        lead_is_quote: chunk.get(lead_end) == Some(&b'"'),
    })
}

/// Phase B: the real start state of every chunk (sequential, O(#chunks)).
pub(crate) fn start_states(summaries: &[Summary]) -> Vec<ScanState> {
    let mut states = Vec::with_capacity(summaries.len());
    let (mut escape, mut in_string) = (0u64, false);
    for s in summaries {
        states.push(ScanState {
            escaped_carry: escape,
            in_string_carry: if in_string { u64::MAX } else { 0 },
        });
        let (mut parity, mut carry) = (s.quote_parity, s.escape_carry);
        if escape == 1 {
            // An escape carried in flips the escape status of the byte after the leading
            // backslash run, and the carry out if the whole chunk is that run.
            if s.lead_end < s.len {
                parity ^= s.lead_is_quote;
            } else {
                carry ^= 1;
            }
        }
        in_string ^= parity;
        escape = carry;
    }
    states
}

fn classify_partial(simd: &Simd, rest: &[u8]) -> RawMasks {
    let mut padded = [b' '; 64];
    padded[..rest.len()].copy_from_slice(rest);
    simd.classify(&padded).truncate(rest.len())
}

/// `true` for bytes that can be part of a scalar run (outside strings).
#[inline]
fn is_scalar_byte(b: u8) -> bool {
    !matches!(
        b,
        b' ' | b'\t' | b'\n' | b'\r' | b'{' | b'}' | b'[' | b']' | b':' | b',' | b'"'
    )
}

/// Phase C: appends the tokens of `bytes[start..end]` to `out`, starting from `state`.
/// Returns `false` if cancelled.
pub(crate) fn tokenize(
    simd: &Simd,
    bytes: &[u8],
    start: usize,
    end: usize,
    mut state: ScanState,
    out: &mut Vec<u32>,
    cancel: &CancelToken,
) -> bool {
    let chunk = &bytes[start..end];
    // Is the byte before the chunk part of a scalar run that continues into it?
    let mut prev_scalar =
        u64::from(state.in_string_carry == 0 && start > 0 && is_scalar_byte(bytes[start - 1]));
    let (blocks, rest) = chunk.as_chunks::<64>();
    let mut emit = |raw: RawMasks, valid: u64, base: usize, state: &mut ScanState| {
        let b = simd::finish_block(simd, raw, state);
        let opens = b.string_quotes() & b.in_string;
        let structurals = b.structurals();
        let scalar = !b.in_string & !raw.whitespace & !raw.structural & !raw.quote & valid;
        let scalar_starts = scalar & !((scalar << 1) | prev_scalar);
        prev_scalar = scalar >> 63;
        let dirty = (raw.backslash | raw.control) & b.in_string;
        simd::for_each_bit(structurals | opens | scalar_starts | dirty, |i| {
            let bit = 1u64 << i;
            let rel = base + i as usize;
            if dirty & bit != 0 {
                // Inside the string that the last token opened (no token is emitted
                // inside a string); a string continued from an earlier chunk has none.
                if let Some(last) = out.last_mut()
                    && *last >> 28 == K_CLEAN_STRING
                {
                    *last = encode(K_SUSPICIOUS, (*last & OFFSET_MASK) as usize);
                }
            } else if opens & bit != 0 {
                out.push(encode(K_CLEAN_STRING, rel));
            } else if structurals & bit != 0 {
                let kind = match chunk[rel] {
                    b'{' => 0,
                    b'[' => 1,
                    b'}' => 2,
                    b']' => 3,
                    b':' => K_COLON,
                    _ => 5,
                };
                match out.last_mut() {
                    Some(last) if kind == K_COLON && *last >> 28 == K_CLEAN_STRING => {
                        *last = encode(K_KEY, (*last & OFFSET_MASK) as usize);
                    }
                    _ => out.push(encode(kind, rel)),
                }
            } else {
                let from = start + rel;
                let len = bytes[from..]
                    .iter()
                    .position(|&c| !is_scalar_byte(c))
                    .unwrap_or(bytes.len() - from);
                let ok = is_valid_scalar(&bytes[from..from + len]);
                out.push(encode(if ok { 7 } else { K_SUSPICIOUS }, rel));
            }
        });
    };
    for (i, block) in blocks.iter().enumerate() {
        if i % CHECK_BLOCKS == CHECK_BLOCKS - 1 && cancel.is_cancelled() {
            return false;
        }
        emit(simd.classify(block), u64::MAX, i * 64, &mut state);
    }
    if !rest.is_empty() {
        let valid = (1u64 << rest.len()) - 1;
        emit(
            classify_partial(simd, rest),
            valid,
            blocks.len() * 64,
            &mut state,
        );
    }
    // A string still open at the chunk end continues into the next chunk: let the
    // reference parser read it.
    if state.in_string_carry != 0
        && let Some(last) = out.last_mut()
        && *last >> 28 == K_CLEAN_STRING
    {
        *last = encode(K_SUSPICIOUS, (*last & OFFSET_MASK) as usize);
    }
    !cancel.is_cancelled()
}

/// `true` if `run` is exactly a JSON literal or number (what the reference parser accepts
/// as one whole scalar token).
pub(crate) fn is_valid_scalar(run: &[u8]) -> bool {
    match run {
        b"true" | b"false" | b"null" => true,
        [b'-' | b'0'..=b'9', ..] => number_len(run) == Some(run.len()),
        _ => false,
    }
}

/// Length of the number at the start of `s` (JSON grammar), or `None` if invalid.
fn number_len(s: &[u8]) -> Option<usize> {
    let mut i = usize::from(s.first() == Some(&b'-'));
    match s.get(i) {
        Some(b'0') => i += 1,
        Some(b'1'..=b'9') => i += digit_run(&s[i..]),
        _ => return None,
    }
    if s.get(i) == Some(&b'.') {
        i += 1;
        let n = digit_run(&s[i..]);
        if n == 0 {
            return None;
        }
        i += n;
    }
    if let Some(b'e' | b'E') = s.get(i) {
        i += 1;
        if let Some(b'+' | b'-') = s.get(i) {
            i += 1;
        }
        let n = digit_run(&s[i..]);
        if n == 0 {
            return None;
        }
        i += n;
    }
    Some(i)
}

fn digit_run(s: &[u8]) -> usize {
    s.iter().take_while(|b| b.is_ascii_digit()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_runs() {
        for ok in [
            &b"true"[..],
            b"false",
            b"null",
            b"0",
            b"-0",
            b"12",
            b"1.5e-3",
            b"-9E+10",
        ] {
            assert!(is_valid_scalar(ok), "{:?}", String::from_utf8_lossy(ok));
        }
        for bad in [
            &b""[..],
            b"tru",
            b"truex",
            b"01",
            b"-",
            b"1.",
            b"1e",
            b"1e+",
            b".1",
            b"1.2.3",
            b"+1",
            b"nul",
            b"NaN",
            b"0x1",
            b"\\",
        ] {
            assert!(!is_valid_scalar(bad), "{:?}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn token_encoding() {
        assert_eq!(decode(encode(0, 5)), (Some(Tok::OpenObject), 5));
        assert_eq!(
            decode(encode(7, MAX_CHUNK - 1)),
            (Some(Tok::Scalar), MAX_CHUNK - 1)
        );
        assert_eq!(decode(encode(K_SUSPICIOUS, 9)), (None, 9));
        assert_eq!(decode(encode(K_KEY, 3)), (Some(Tok::Key), 3));
    }
}
