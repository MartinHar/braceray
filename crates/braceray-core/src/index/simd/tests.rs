//! Equivalence tests: every available implementation against byte-by-byte references.

use super::*;
use proptest::prelude::*;

/// Byte-at-a-time reference for one block's classes.
fn reference_raw(block: &[u8]) -> RawMasks {
    let mut m = RawMasks::default();
    for (i, &b) in block.iter().enumerate() {
        let bit = 1u64 << i;
        let on = |c: bool| if c { bit } else { 0 };
        m.quote |= on(b == b'"');
        m.backslash |= on(b == b'\\');
        m.structural |= on(b"{}[]:,".contains(&b));
        m.whitespace |= on(b" \t\n\r".contains(&b));
        m.newline |= on(b == b'\n');
        m.control |= on(b < 0x20);
    }
    m
}

/// Byte-at-a-time reference for escaped / in-string flags over a whole input.
fn reference_strings(bytes: &[u8]) -> (Vec<bool>, Vec<bool>) {
    let (mut escaped, mut in_string) = (Vec::new(), Vec::new());
    let (mut esc_next, mut inside) = (false, false);
    for &b in bytes {
        let esc = esc_next;
        esc_next = !esc && b == b'\\';
        if b == b'"' && !esc {
            inside = !inside;
        }
        escaped.push(esc);
        in_string.push(inside);
    }
    (escaped, in_string)
}

fn scan_all(simd: &Simd, bytes: &[u8]) -> (Vec<bool>, Vec<bool>, Vec<RawMasks>) {
    let mut state = ScanState::default();
    let (mut escaped, mut in_string, mut raws) = (Vec::new(), Vec::new(), Vec::new());
    scan(simd, bytes, &mut state, |off, block| {
        let n = (bytes.len() - off).min(64);
        for i in 0..n {
            escaped.push(block.escaped >> i & 1 == 1);
            in_string.push(block.in_string >> i & 1 == 1);
        }
        raws.push(block.raw);
    });
    (escaped, in_string, raws)
}

fn interesting_bytes(max: usize) -> impl Strategy<Value = Vec<u8>> {
    let byte = prop_oneof![
        4 => prop::sample::select(&b"\\\\\\\"\"a{}[]:, \t\r\n\x00\x1f\x7f"[..]),
        1 => any::<u8>(),
    ];
    prop::collection::vec(byte, 0..max)
}

#[test]
fn available_includes_the_native_implementation() {
    let names: Vec<_> = available().iter().map(|s| s.name).collect();
    assert_eq!(names[0], "scalar");
    #[cfg(target_arch = "aarch64")]
    assert!(names.contains(&"neon"));
    eprintln!("available: {names:?}, best: {}", best().name);
}

#[test]
fn every_byte_value_is_classified_like_the_reference() {
    let all: Vec<u8> = (0..=255).collect();
    for simd in available() {
        for block in all.chunks(64) {
            let block: [u8; 64] = block.try_into().unwrap();
            assert_eq!(
                simd.classify(&block),
                reference_raw(&block),
                "{}",
                simd.name
            );
        }
    }
}

#[test]
fn escape_runs_across_blocks() {
    // Backslash runs of every length ending at every position around a block boundary.
    for simd in available() {
        for run in 0..70 {
            for start in 0..70 {
                let mut v = vec![b'a'; 140];
                v[start..start + run].fill(b'\\');
                v[start + run] = b'"';
                let (esc, ins, _) = scan_all(&simd, &v);
                let (ref_esc, ref_ins) = reference_strings(&v);
                assert_eq!(esc, ref_esc, "{} run {run} at {start}", simd.name);
                assert_eq!(ins, ref_ins, "{} run {run} at {start}", simd.name);
            }
        }
    }
}

#[test]
fn bit_helpers() {
    let mut seen = Vec::new();
    for_each_bit(0b1010_0001 | 1 << 63, |i| seen.push(i));
    assert_eq!(seen, [0, 5, 7, 63]);
    let m = RawMasks {
        quote: u64::MAX,
        ..RawMasks::default()
    };
    assert_eq!(m.truncate(3).quote, 0b111);
    assert_eq!(m.truncate(64).quote, u64::MAX);
    assert_eq!(prefix_xor_ladder(0b1001), 0b0111);
    assert_eq!(prefix_xor_ladder(1), u64::MAX);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn implementations_agree_on_raw_masks(bytes in prop::collection::vec(any::<u8>(), 64)) {
        let block: [u8; 64] = bytes.as_slice().try_into().unwrap();
        let want = reference_raw(&block);
        for simd in available() {
            prop_assert_eq!(simd.classify(&block), want, "{}", simd.name);
        }
    }

    #[test]
    fn implementations_agree_on_string_masks(bytes in interesting_bytes(400)) {
        let (ref_esc, ref_ins) = reference_strings(&bytes);
        let scalar = scan_all(&Simd::scalar(), &bytes);
        prop_assert_eq!(&scalar.0, &ref_esc);
        prop_assert_eq!(&scalar.1, &ref_ins);
        for simd in available() {
            let got = scan_all(&simd, &bytes);
            prop_assert_eq!(&got, &scalar, "{}", simd.name);
        }
    }

    #[test]
    fn prefix_xor_implementations_agree(x in any::<u64>()) {
        let want = (0..64).fold(0u64, |acc, i| {
            let parity = (x & ((2u128 << i) - 1) as u64).count_ones() & 1;
            acc | (u64::from(parity) << i)
        });
        for simd in available() {
            prop_assert_eq!(simd.prefix_xor(x), want, "{}", simd.name);
        }
    }
}
