//! Byte-level helpers for walking the children of a container. All of them are bounded by
//! `end` and return `None` instead of panicking on malformed input.

use crate::document::is_json_ws;
use crate::index::simd;

/// First non-whitespace offset in `pos..end` (or `end`).
pub(crate) fn skip_ws(bytes: &[u8], mut pos: usize, end: usize) -> usize {
    while pos < end && is_json_ws(bytes[pos]) {
        pos += 1;
    }
    pos
}

/// Offset just after the closing quote of the string starting at `pos` (a `"`).
pub(crate) fn string_end(bytes: &[u8], pos: usize, end: usize) -> Option<usize> {
    let mut i = pos + 1;
    while i < end {
        let rel = memchr::memchr2(b'"', b'\\', &bytes[i..end])?;
        i += rel;
        if bytes[i] == b'"' {
            return Some(i + 1);
        }
        i += 2; // skip the escaped byte
    }
    None
}

/// Offset just after the scalar token (number or literal) starting at `pos`.
pub(crate) fn scalar_end(bytes: &[u8], pos: usize, end: usize) -> usize {
    bytes[pos..end]
        .iter()
        .position(|&b| {
            matches!(
                b,
                b' ' | b'\t' | b'\n' | b'\r' | b'{' | b'}' | b'[' | b']' | b':' | b',' | b'"'
            )
        })
        .map_or(end, |p| pos + p)
}

/// Offset just after the bracket matching the `{` or `[` at `pos`, found with the SIMD
/// classifier (for containers the index doesn't store).
pub(crate) fn container_end(bytes: &[u8], pos: usize, end: usize) -> Option<usize> {
    let imp = simd::best();
    let mut state = simd::ScanState::default();
    let mut depth = 0u64;
    let hay = &bytes[pos..end];
    let mut found = None;
    let mut visit = |base: usize, block: &simd::Block| -> bool {
        let mut stop = false;
        simd::for_each_bit(block.structurals(), |i| {
            if stop {
                return;
            }
            match hay[base + i as usize] {
                b'{' | b'[' => depth += 1,
                b'}' | b']' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        found = Some(pos + base + i as usize + 1);
                        stop = true;
                    }
                }
                _ => {}
            }
        });
        stop
    };
    let (blocks, rest) = hay.as_chunks::<64>();
    for (k, block) in blocks.iter().enumerate() {
        if visit(k * 64, &simd::scan_block(&imp, block, &mut state)) {
            return found;
        }
    }
    if !rest.is_empty() {
        let mut padded = [b' '; 64];
        padded[..rest.len()].copy_from_slice(rest);
        let raw = imp.classify(&padded).truncate(rest.len());
        visit(
            blocks.len() * 64,
            &simd::finish_block(&imp, raw, &mut state),
        );
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        let b = br#"  "a\"b\\" 12e3, {"x": [1, "]"], "y": {}} tail"#;
        assert_eq!(skip_ws(b, 0, b.len()), 2);
        assert_eq!(string_end(b, 2, b.len()), Some(10));
        assert_eq!(scalar_end(b, 11, b.len()), 15);
        let open = b.iter().position(|&c| c == b'{').unwrap();
        let close = b.iter().rposition(|&c| c == b'}').unwrap();
        assert_eq!(container_end(b, open, b.len()), Some(close + 1));
        assert_eq!(container_end(b, open, close), None);
        assert_eq!(string_end(b"\"abc", 0, 4), None);
        assert_eq!(string_end(b"\"a\\", 0, 3), None);
    }

    #[test]
    fn container_end_across_blocks() {
        let mut v = b"[".to_vec();
        for i in 0..500 {
            v.extend_from_slice(format!("{{\"k{i}\": \"]}}\\\"\"}},").as_bytes());
        }
        v.extend_from_slice(b"0]   ");
        assert_eq!(container_end(&v, 0, v.len()), Some(v.len() - 3));
    }
}
