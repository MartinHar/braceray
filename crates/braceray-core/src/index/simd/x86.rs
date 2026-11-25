//! SSE2 and AVX2 classifiers plus `pclmulqdq` prefix XOR (x86_64), detected at runtime.
//!
//! The `*_checked` wrappers are safe `fn`s so they fit the `Simd` function pointers. They
//! are private and only handed out by [`available`] after the CPU feature was detected.

use std::arch::x86_64::*;

use super::{RawMasks, Simd, prefix_xor_ladder};

/// Implementations this CPU supports, slowest first.
pub(super) fn available() -> Vec<Simd> {
    let pclmul = is_x86_feature_detected!("pclmulqdq") && is_x86_feature_detected!("sse2");
    let prefix_xor: fn(u64) -> u64 = if pclmul {
        prefix_xor_clmul_checked
    } else {
        prefix_xor_ladder
    };
    let mut v = Vec::new();
    // SSE2 is part of the x86_64 baseline; the check documents the requirement.
    if is_x86_feature_detected!("sse2") {
        v.push(Simd {
            name: if pclmul { "sse2+pclmul" } else { "sse2" },
            classify: classify_sse_checked,
            prefix_xor,
        });
    }
    if is_x86_feature_detected!("avx2") {
        v.push(Simd {
            name: if pclmul { "avx2+pclmul" } else { "avx2" },
            classify: classify_avx2_checked,
            prefix_xor,
        });
    }
    v
}

fn prefix_xor_clmul_checked(x: u64) -> u64 {
    // SAFETY: only selected by `available` after detecting `pclmulqdq` and SSE2.
    unsafe { prefix_xor_clmul(x) }
}

fn classify_sse_checked(block: &[u8; 64]) -> RawMasks {
    // SAFETY: only selected by `available` after detecting SSE2.
    unsafe { classify_sse(block) }
}

fn classify_avx2_checked(block: &[u8; 64]) -> RawMasks {
    // SAFETY: only selected by `available` after detecting AVX2.
    unsafe { classify_avx2(block) }
}

/// Carry-less multiply by all ones = prefix XOR.
#[target_feature(enable = "pclmulqdq,sse2")]
unsafe fn prefix_xor_clmul(x: u64) -> u64 {
    let r = _mm_clmulepi64_si128(_mm_set_epi64x(0, x as i64), _mm_set1_epi8(-1), 0);
    _mm_cvtsi128_si64(r) as u64
}

/// Shared body of the SSE and AVX2 classifiers: `$lanes` vectors of `$width` bytes.
macro_rules! classify_body {
    ($block:expr, $vec:ty, $load:ident, $lanes:expr, $width:expr,
     $set1:ident, $cmpeq:ident, $or:ident, $min:ident, $movemask:ident, $mask_ty:ty) => {{
        let p = $block.as_ptr() as *const $vec;
        let mut m = RawMasks::default();
        for i in 0..$lanes {
            // SAFETY: lane `i` of `$lanes` × `$width` bytes lies inside the 64-byte array.
            let x = unsafe { $load(p.add(i)) };
            let shift = $width * i;
            macro_rules! eq {
                ($v:expr, $c:expr) => {
                    $cmpeq($v, $set1($c as i8))
                };
            }
            macro_rules! bits {
                ($v:expr) => {
                    u64::from($movemask($v) as $mask_ty) << shift
                };
            }
            // `[`|0x20 = `{` and `]`|0x20 = `}`; no other byte maps onto those two.
            let folded = $or(x, $set1(0x20));
            let newline = eq!(x, b'\n');
            m.quote |= bits!(eq!(x, b'"'));
            m.backslash |= bits!(eq!(x, b'\\'));
            m.structural |= bits!($or(
                $or(eq!(folded, b'{'), eq!(folded, b'}')),
                $or(eq!(x, b':'), eq!(x, b','))
            ));
            m.whitespace |= bits!($or(
                $or(eq!(x, b' '), eq!(x, b'\t')),
                $or(newline, eq!(x, b'\r'))
            ));
            m.newline |= bits!(newline);
            // x < 0x20  ⇔  min(x, 0x1F) == x (unsigned)
            m.control |= bits!($cmpeq($min(x, $set1(0x1F)), x));
        }
        m
    }};
}

#[target_feature(enable = "sse2")]
unsafe fn classify_sse(block: &[u8; 64]) -> RawMasks {
    classify_body!(
        block,
        __m128i,
        _mm_loadu_si128,
        4,
        16,
        _mm_set1_epi8,
        _mm_cmpeq_epi8,
        _mm_or_si128,
        _mm_min_epu8,
        _mm_movemask_epi8,
        u16
    )
}

#[target_feature(enable = "avx2")]
unsafe fn classify_avx2(block: &[u8; 64]) -> RawMasks {
    classify_body!(
        block,
        __m256i,
        _mm256_loadu_si256,
        2,
        32,
        _mm256_set1_epi8,
        _mm256_cmpeq_epi8,
        _mm256_or_si256,
        _mm256_min_epu8,
        _mm256_movemask_epi8,
        u32
    )
}
