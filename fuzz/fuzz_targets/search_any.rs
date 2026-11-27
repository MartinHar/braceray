//! Arbitrary bytes + needle + options → search with tiny chunks and a tiny cap. Must not
//! panic; the count, the stored prefix and next/prev walks must equal a naive
//! non-overlapping scan (ASCII-folded when case-insensitive).
#![no_main]

use std::sync::Arc;

use braceray_core::job::no_repaint;
use braceray_core::search::{SearchOptions, SearchQuery, start_search};
use libfuzzer_sys::fuzz_target;

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

fn naive(hay: &[u8], needle: &[u8], cs: bool, ww: bool) -> Vec<u64> {
    let n = needle.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i + n <= hay.len() {
        let w = &hay[i..i + n];
        let eq = if cs { w == needle } else { w.eq_ignore_ascii_case(needle) };
        let e = i + n;
        let ok = !ww
            || ((!is_word(hay[i]) || i == 0 || !is_word(hay[i - 1]))
                && (!is_word(hay[e - 1]) || e == hay.len() || !is_word(hay[e])));
        if eq && ok {
            out.push(i as u64);
            i = e;
        } else {
            i += 1;
        }
    }
    out
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 3 {
        return;
    }
    let flags = data[0];
    let nlen = (data[1] as usize % 8) + 1;
    let rest = &data[2..];
    let nlen = nlen.min(rest.len());
    let (needle, hay) = rest.split_at(nlen);
    let Ok(text) = std::str::from_utf8(needle) else { return };
    let cs = flags & 1 != 0;
    let ww = flags & 2 != 0;
    let chunk = [1, 2, 3, 7, 64, 65, 4096][(flags >> 2) as usize % 7];
    let cap = [0, 1, 5, usize::MAX][(flags >> 5) as usize % 4];
    let q = SearchQuery {
        case_sensitive: cs,
        whole_word: ww,
        ..SearchQuery::new(text)
    };
    let opts = SearchOptions {
        chunk_size: chunk,
        max_stored_results: cap,
        threads: Some(3),
    };
    let h = start_search(Arc::new(hay.to_vec()), &q, opts, no_repaint()).unwrap();
    h.wait();
    if !cs && !text.is_ascii() {
        return; // Unicode folding: covered by the proptests' regex oracle.
    }
    let want = naive(hay, needle, cs, ww);
    assert_eq!(h.count(), want.len() as u64);
    let stored: Vec<u64> = (0..h.stored()).map(|i| h.get(i).unwrap().offset).collect();
    assert_eq!(stored, want[..want.len().min(cap)]);
    let mut fwd = Vec::new();
    let mut cur = h.prev_before(1).or_else(|| h.next_after(0));
    while let Some(m) = cur {
        fwd.push(m.offset);
        cur = h.next_after(m.offset);
    }
    assert_eq!(fwd, want);
    let mut back = Vec::new();
    let mut off = hay.len() as u64 + 1;
    while let Some(m) = h.prev_before(off) {
        back.push(m.offset);
        off = m.offset;
    }
    back.reverse();
    assert_eq!(back, want);
});
