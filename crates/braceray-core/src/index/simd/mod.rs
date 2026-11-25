//! 64-byte block classifier (the simdjson technique), with runtime CPU dispatch.
//!
//! [`RawMasks`] are per-byte character classes; [`scan_block`] adds the escape mask and
//! the in-string mask, carrying state from one block to the next in [`ScanState`].
//! The byte classification has scalar, NEON (aarch64), SSE2 and AVX2 (x86_64)
//! implementations (the "SSE4.2" path of the spec only needs SSE2, see ADR 010);
//! prefix XOR uses `pclmulqdq` when available, else a shift ladder.

mod scalar;

#[cfg(target_arch = "aarch64")]
mod neon;
#[cfg(target_arch = "x86_64")]
mod x86;

use std::sync::OnceLock;

/// Character-class bitmasks of one 64-byte block: bit `i` describes byte `i`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RawMasks {
    /// `"`
    pub quote: u64,
    /// `\`
    pub backslash: u64,
    /// `{ } [ ] : ,`
    pub structural: u64,
    /// JSON whitespace: space, tab, LF, CR
    pub whitespace: u64,
    /// LF
    pub newline: u64,
    /// bytes < 0x20 (not allowed raw inside strings)
    pub control: u64,
}

impl RawMasks {
    /// Clears every bit at or above `len` (for a partial last block).
    #[inline]
    pub fn truncate(self, len: usize) -> Self {
        let keep = if len >= 64 {
            u64::MAX
        } else {
            (1u64 << len) - 1
        };
        RawMasks {
            quote: self.quote & keep,
            backslash: self.backslash & keep,
            structural: self.structural & keep,
            whitespace: self.whitespace & keep,
            newline: self.newline & keep,
            control: self.control & keep,
        }
    }
}

/// A classifier implementation, chosen once per process by [`best`].
#[derive(Clone, Copy)]
pub struct Simd {
    /// `"scalar"`, `"neon"`, `"sse2"`, `"avx2"`, optionally with `"+pclmul"`.
    pub name: &'static str,
    classify: fn(&[u8; 64]) -> RawMasks,
    prefix_xor: fn(u64) -> u64,
}

impl std::fmt::Debug for Simd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

impl Simd {
    /// Classifies one block.
    #[inline]
    pub fn classify(&self, block: &[u8; 64]) -> RawMasks {
        (self.classify)(block)
    }

    /// Bit `i` of the result = XOR of bits `0..=i` of `x`.
    #[inline]
    pub fn prefix_xor(&self, x: u64) -> u64 {
        (self.prefix_xor)(x)
    }

    /// The portable implementation (always available).
    pub fn scalar() -> Simd {
        Simd {
            name: "scalar",
            classify: scalar::classify,
            prefix_xor: prefix_xor_ladder,
        }
    }
}

/// The fastest implementation this CPU supports (detected once, then cached).
pub fn best() -> Simd {
    static BEST: OnceLock<Simd> = OnceLock::new();
    *BEST.get_or_init(|| available().pop().unwrap_or_else(Simd::scalar))
}

/// Every implementation this CPU supports, slowest first (for equivalence tests and
/// benchmarks).
pub fn available() -> Vec<Simd> {
    #[allow(unused_mut)] // only pushed to on aarch64 / x86_64
    let mut v = vec![Simd::scalar()];
    #[cfg(target_arch = "aarch64")]
    v.push(neon::simd());
    #[cfg(target_arch = "x86_64")]
    v.extend(x86::available());
    v
}

/// Portable prefix XOR: `x ^= x << 1; x ^= x << 2; … x ^= x << 32`.
#[inline]
pub fn prefix_xor_ladder(mut x: u64) -> u64 {
    x ^= x << 1;
    x ^= x << 2;
    x ^= x << 4;
    x ^= x << 8;
    x ^= x << 16;
    x ^= x << 32;
    x
}

const EVEN_BITS: u64 = 0x5555_5555_5555_5555;
const ODD_BITS: u64 = !EVEN_BITS;

/// State carried from one block to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScanState {
    /// 1 if byte 0 of the next block is escaped (the block ended with an escape start).
    pub escaped_carry: u64,
    /// All ones if the next block starts inside a string, else 0.
    pub in_string_carry: u64,
}

/// One classified block with string context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Block {
    /// Character classes.
    pub raw: RawMasks,
    /// Bytes preceded by an escaping backslash (including escaped backslashes).
    pub escaped: u64,
    /// Bytes inside a string: the opening quote and the content, not the closing quote.
    pub in_string: u64,
}

impl Block {
    /// Quotes that open or close a string.
    #[inline]
    pub fn string_quotes(&self) -> u64 {
        self.raw.quote & !self.escaped
    }

    /// Structural characters outside strings.
    #[inline]
    pub fn structurals(&self) -> u64 {
        self.raw.structural & !self.in_string
    }
}

/// Escaped bytes of a block from its backslash mask, carrying across blocks.
///
/// A backslash run starting at `r` has escape starts at `r, r+2, …`; each escapes the
/// next byte. Runs are split into even- and odd-start runs with one addition.
#[inline]
pub fn escaped_mask(backslash: u64, carry: &mut u64) -> u64 {
    let potential = backslash & !*carry; // an escaped backslash at byte 0 starts nothing
    let run_starts = potential & !(potential << 1);
    let odd_starts = run_starts & ODD_BITS;
    // Adding a run's start bit clears the whole run (carry ripple): this isolates
    // odd-start runs.
    let odd_runs = potential & !potential.wrapping_add(odd_starts);
    let even_runs = potential & !odd_runs;
    let starts = (even_runs & EVEN_BITS) | (odd_runs & ODD_BITS);
    let escaped = (starts << 1) | *carry;
    *carry = starts >> 63;
    escaped
}

/// Classifies `block` and computes its escape and in-string masks.
#[inline]
pub fn scan_block(simd: &Simd, block: &[u8; 64], state: &mut ScanState) -> Block {
    finish_block(simd, simd.classify(block), state)
}

/// Adds the escape and in-string masks to already-classified `raw` masks.
#[inline]
pub fn finish_block(simd: &Simd, raw: RawMasks, state: &mut ScanState) -> Block {
    let escaped = escaped_mask(raw.backslash, &mut state.escaped_carry);
    let quotes = raw.quote & !escaped;
    let in_string = simd.prefix_xor(quotes) ^ state.in_string_carry;
    state.in_string_carry = ((in_string as i64) >> 63) as u64;
    Block {
        raw,
        escaped,
        in_string,
    }
}

/// Scans `bytes` block by block from `state`, calling `f(offset, block)` for each
/// 64-byte block. The last partial block is padded with spaces and its masks are
/// truncated to the real length.
pub fn scan(simd: &Simd, bytes: &[u8], state: &mut ScanState, mut f: impl FnMut(usize, &Block)) {
    let (blocks, rest) = bytes.as_chunks::<64>();
    let mut offset = 0;
    for block in blocks {
        f(offset, &scan_block(simd, block, state));
        offset += 64;
    }
    if !rest.is_empty() {
        let mut padded = [b' '; 64];
        padded[..rest.len()].copy_from_slice(rest);
        let raw = simd.classify(&padded).truncate(rest.len());
        f(offset, &finish_block(simd, raw, state));
    }
}

/// Calls `f(i)` for every set bit `i` of `mask`, lowest first.
#[inline]
pub fn for_each_bit(mut mask: u64, mut f: impl FnMut(u32)) {
    while mask != 0 {
        f(mask.trailing_zeros());
        mask &= mask - 1;
    }
}

#[cfg(test)]
mod tests;
