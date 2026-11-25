//! Portable byte-at-a-time classifier (fallback, and the reference for the SIMD ones).

use super::RawMasks;

const QUOTE: u8 = 1;
const BACKSLASH: u8 = 2;
const STRUCTURAL: u8 = 4;
const WHITESPACE: u8 = 8;
const NEWLINE: u8 = 16;
const CONTROL: u8 = 32;

/// Class flags for every byte value.
static CLASS: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut b = 0;
    while b < 0x20 {
        t[b] = CONTROL;
        b += 1;
    }
    t[b'"' as usize] = QUOTE;
    t[b'\\' as usize] = BACKSLASH;
    t[b'{' as usize] = STRUCTURAL;
    t[b'}' as usize] = STRUCTURAL;
    t[b'[' as usize] = STRUCTURAL;
    t[b']' as usize] = STRUCTURAL;
    t[b':' as usize] = STRUCTURAL;
    t[b',' as usize] = STRUCTURAL;
    t[b' ' as usize] = WHITESPACE;
    t[b'\t' as usize] = WHITESPACE | CONTROL;
    t[b'\r' as usize] = WHITESPACE | CONTROL;
    t[b'\n' as usize] = WHITESPACE | NEWLINE | CONTROL;
    t
};

pub(super) fn classify(block: &[u8; 64]) -> RawMasks {
    let mut m = RawMasks::default();
    for (i, &b) in block.iter().enumerate() {
        let c = CLASS[b as usize];
        if c == 0 {
            continue;
        }
        let bit = 1u64 << i;
        let set = |flag: u8| if c & flag != 0 { bit } else { 0 };
        m.quote |= set(QUOTE);
        m.backslash |= set(BACKSLASH);
        m.structural |= set(STRUCTURAL);
        m.whitespace |= set(WHITESPACE);
        m.newline |= set(NEWLINE);
        m.control |= set(CONTROL);
    }
    m
}
