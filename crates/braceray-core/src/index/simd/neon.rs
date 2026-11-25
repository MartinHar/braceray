//! NEON classifier (aarch64; NEON is part of the baseline, so no runtime detection).

use std::arch::aarch64::*;

use super::{RawMasks, Simd, prefix_xor_ladder};

pub(super) fn simd() -> Simd {
    Simd {
        name: "neon",
        classify,
        prefix_xor: prefix_xor_ladder,
    }
}

/// Packs four 16-lane compare results (0xFF / 0x00 per lane) into a 64-bit mask.
#[inline(always)]
fn movemask(v: [uint8x16_t; 4]) -> u64 {
    const WEIGHTS: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
    // SAFETY: `WEIGHTS` is 16 readable bytes; NEON is always available on aarch64.
    unsafe {
        let w = vld1q_u8(WEIGHTS.as_ptr());
        let t0 = vandq_u8(v[0], w);
        let t1 = vandq_u8(v[1], w);
        let t2 = vandq_u8(v[2], w);
        let t3 = vandq_u8(v[3], w);
        let s0 = vpaddq_u8(t0, t1);
        let s1 = vpaddq_u8(t2, t3);
        let s = vpaddq_u8(s0, s1);
        let s = vpaddq_u8(s, s);
        vgetq_lane_u64(vreinterpretq_u64_u8(s), 0)
    }
}

fn classify(block: &[u8; 64]) -> RawMasks {
    // SAFETY: the four 16-byte loads stay inside the 64-byte array; NEON is always
    // available on aarch64.
    unsafe {
        let p = block.as_ptr();
        let v = [
            vld1q_u8(p),
            vld1q_u8(p.add(16)),
            vld1q_u8(p.add(32)),
            vld1q_u8(p.add(48)),
        ];
        let eq = |c: u8| v.map(|x| vceqq_u8(x, vdupq_n_u8(c)));
        // `[`|0x20 = `{` and `]`|0x20 = `}`; no other byte maps onto those two.
        let folded = v.map(|x| vorrq_u8(x, vdupq_n_u8(0x20)));
        let structural: [uint8x16_t; 4] = std::array::from_fn(|i| {
            let a = vceqq_u8(folded[i], vdupq_n_u8(b'{'));
            let b = vceqq_u8(folded[i], vdupq_n_u8(b'}'));
            let c = vceqq_u8(v[i], vdupq_n_u8(b':'));
            let d = vceqq_u8(v[i], vdupq_n_u8(b','));
            vorrq_u8(vorrq_u8(a, b), vorrq_u8(c, d))
        });
        let newline = eq(b'\n');
        let space = eq(b' ');
        let tab = eq(b'\t');
        let cr = eq(b'\r');
        let whitespace: [uint8x16_t; 4] = std::array::from_fn(|i| {
            vorrq_u8(vorrq_u8(newline[i], space[i]), vorrq_u8(tab[i], cr[i]))
        });
        let control = v.map(|x| vcltq_u8(x, vdupq_n_u8(0x20)));
        RawMasks {
            quote: movemask(eq(b'"')),
            backslash: movemask(eq(b'\\')),
            structural: movemask(structural),
            whitespace: movemask(whitespace),
            newline: movemask(newline),
            control: movemask(control),
        }
    }
}
