//! Search engine tests (step 09): a naive oracle against proptest inputs, generated
//! fixtures and every chunk size, the stored cap, Unicode case folding and cancellation.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use braceray_core::job::no_repaint;
use braceray_core::search::{SearchHandle, SearchOptions, SearchQuery, start_search};
use proptest::prelude::*;

const CHUNKS: [usize; 5] = [64, 65, 127, 4096, 8 << 20];

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

fn boundary_ok(hay: &[u8], s: usize, e: usize) -> bool {
    (!is_word(hay[s]) || s == 0 || !is_word(hay[s - 1]))
        && (!is_word(hay[e - 1]) || e == hay.len() || !is_word(hay[e]))
}

/// The reference: a byte-by-byte, non-overlapping, leftmost-first scan.
fn naive(hay: &[u8], needle: &[u8], cs: bool, ww: bool) -> Vec<u64> {
    let n = needle.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i + n <= hay.len() {
        let w = &hay[i..i + n];
        let eq = if cs {
            w == needle
        } else {
            w.eq_ignore_ascii_case(needle)
        };
        if eq && (!ww || boundary_ok(hay, i, i + n)) {
            out.push(i as u64);
            i += n;
        } else {
            i += 1;
        }
    }
    out
}

/// Reference for Unicode case-insensitive queries: one regex over the whole haystack.
fn regex_oracle(hay: &[u8], pattern: &str, ww: bool) -> Vec<u64> {
    let re = regex::bytes::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .unwrap();
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(m) = re.find_at(hay, pos) {
        if !ww || boundary_ok(hay, m.start(), m.end()) {
            out.push(m.start() as u64);
            pos = m.end();
        } else {
            pos = m.start() + 1;
        }
    }
    out
}

fn query(text: &str, cs: bool, ww: bool) -> SearchQuery {
    SearchQuery {
        case_sensitive: cs,
        whole_word: ww,
        ..SearchQuery::new(text)
    }
}

fn search(
    hay: Arc<Vec<u8>>,
    q: &SearchQuery,
    chunk: usize,
    cap: usize,
    threads: usize,
) -> SearchHandle {
    let opts = SearchOptions {
        chunk_size: chunk,
        max_stored_results: cap,
        threads: Some(threads),
    };
    let h = start_search(hay, q, opts, no_repaint()).unwrap();
    h.wait();
    assert!(h.is_done());
    h
}

/// Every check: count, stored prefix, and next/prev walks over the whole chain.
fn check(h: &SearchHandle, want: &[u64], cap: usize, len: u64, label: &str) {
    assert_eq!(h.count(), want.len() as u64, "count: {label}");
    let stored: Vec<u64> = (0..h.stored()).map(|i| h.get(i).unwrap().offset).collect();
    assert_eq!(stored, want[..want.len().min(cap)], "stored: {label}");
    assert_eq!(h.is_capped(), want.len() > cap, "capped: {label}");
    if want.len() > cap || want.len() < 200 {
        let mut fwd = Vec::new();
        let mut cur = h.prev_before(1).or_else(|| h.next_after(0));
        while let Some(m) = cur {
            fwd.push(m.offset);
            cur = h.next_after(m.offset);
        }
        assert_eq!(fwd, want, "next_after walk: {label}");
        let mut back = Vec::new();
        let mut off = len + 1;
        while let Some(x) = h.prev_before(off) {
            back.push(x.offset);
            off = x.offset;
        }
        back.reverse();
        assert_eq!(back, want, "prev_before walk: {label}");
    }
}

fn hay_bytes() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(prop::sample::select(b"aAaab _\"\xd1x".to_vec()), 0..700)
}

fn ascii_needle() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(b"aAb _".to_vec()), 1..6)
        .prop_map(|v| String::from_utf8(v).unwrap())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn count_and_order_match_the_naive_scan(
        hay in hay_bytes(),
        needle in ascii_needle(),
        cs in any::<bool>(),
        ww in any::<bool>(),
        chunk in prop::sample::select(vec![1usize, 2, 3, 5, 64, 65, 127, 4096, 8 << 20]),
        cap in prop::sample::select(vec![0usize, 1, 3, 10, usize::MAX]),
        threads in 1usize..5,
    ) {
        let want = naive(&hay, needle.as_bytes(), cs, ww);
        let len = hay.len() as u64;
        let h = search(Arc::new(hay), &query(&needle, cs, ww), chunk, cap, threads);
        check(&h, &want, cap, len, &format!("{needle:?} cs={cs} ww={ww} chunk={chunk} cap={cap}"));
    }

    #[test]
    fn json_inputs_match_the_naive_scan(
        tree in common::json_tree(),
        layout in common::layout(),
        pick in any::<prop::sample::Index>(),
        n in 1usize..8,
        cs in any::<bool>(),
        ww in any::<bool>(),
        chunk in prop::sample::select(CHUNKS.to_vec()),
    ) {
        let (bytes, _) = common::serialize(&tree, &layout);
        let start = pick.index(bytes.len());
        let needle = bytes[start..(start + n).min(bytes.len())].to_vec();
        let Ok(text) = String::from_utf8(needle) else { return Ok(()) };
        prop_assume!(cs || text.is_ascii());
        let want = naive(&bytes, text.as_bytes(), cs, ww);
        let len = bytes.len() as u64;
        let h = search(Arc::new(bytes), &query(&text, cs, ww), chunk, usize::MAX, 4);
        check(&h, &want, usize::MAX, len, &format!("{text:?} cs={cs} ww={ww} chunk={chunk}"));
    }

    #[test]
    fn unicode_case_insensitive_matches_the_regex_oracle(
        pieces in prop::collection::vec(
            prop::sample::select(vec!["ё", "Ё", "лка", "ЛКА", " ", "a", "և", "եվ", "ԵՎ", "Եւ", "ր"]),
            0..120,
        ),
        q in prop::sample::select(vec![
            ("ёлка", "ёлка"),
            ("Ё", "ё"),
            ("ЁЛКА", "ёлка"),
            ("Րև", "ր(?:և|եվ|եւ)"),
            ("ԵՎ", "(?:և|եվ|եւ)"),
            ("և", "(?:և|եվ|եւ)"),
        ]),
        ww in any::<bool>(),
        chunk in prop::sample::select(vec![1usize, 2, 3, 7, 64, 4096]),
    ) {
        let hay = pieces.concat().into_bytes();
        let want = regex_oracle(&hay, q.1, ww);
        let len = hay.len() as u64;
        let h = search(Arc::new(hay), &query(q.0, false, ww), chunk, usize::MAX, 3);
        check(&h, &want, usize::MAX, len, &format!("{:?} ww={ww} chunk={chunk}", q.0));
    }
}

#[test]
fn unicode_acceptance_examples() {
    let hay = Arc::new(
        r#"{"tree": "ёлка", "sun": "արև", "x": "Ёлка АРЕВ"}"#
            .as_bytes()
            .to_vec(),
    );
    let h = search(hay.clone(), &query("ЁЛКА", false, false), 64, 100, 2);
    assert_eq!(h.count(), 2);
    let h = search(hay.clone(), &query("ԱՐԵՎ", false, false), 64, 100, 2);
    assert_eq!(h.count(), 1);
    let first = h.get(0).unwrap();
    assert_eq!(
        &hay[first.offset as usize..first.end() as usize],
        "արև".as_bytes()
    );
    // Case-sensitive stays exact.
    let h = search(hay, &query("ЁЛКА", true, false), 64, 100, 2);
    assert_eq!(h.count(), 0);
}

#[test]
fn next_prev_across_a_cap_of_ten() {
    let hay = b"{\"k\": \"needle\"}, ".repeat(5000);
    let want = naive(&hay, b"needle", false, false);
    let len = hay.len() as u64;
    for chunk in CHUNKS {
        let h = search(
            Arc::new(hay.clone()),
            &query("needle", false, false),
            chunk,
            10,
            4,
        );
        assert_eq!(h.stored(), 10);
        check(&h, &want, 10, len, &format!("chunk {chunk}"));
        // From arbitrary offsets, not only match starts.
        for off in [0, 7, 100, 50_000, len - 3] {
            let next = want.iter().copied().find(|&o| o > off);
            let prev = want.iter().copied().rev().find(|&o| o < off);
            assert_eq!(h.next_after(off).map(|m| m.offset), next, "next {off}");
            assert_eq!(h.prev_before(off).map(|m| m.offset), prev, "prev {off}");
        }
    }
}

/// A temp dir with fixtures generated by `gen-fixtures` (10 MB of each kind).
fn generated_fixtures() -> (tempfile::TempDir, Vec<std::path::PathBuf>) {
    use gen_fixtures::orders::{RecordsOpts, generate_orders};
    use gen_fixtures::{logs, shapes};
    let dir = tempfile::tempdir().unwrap();
    let p = |n: &str| dir.path().join(n);
    let opts = |minified| RecordsOpts {
        size: 10 << 20,
        seed: 42,
        minified,
    };
    generate_orders(&p("orders_10m.json"), opts(false)).unwrap();
    generate_orders(&p("orders_min_10m.json"), opts(true)).unwrap();
    logs::generate_ndjson(&p("logs_10m.ndjson"), opts(true)).unwrap();
    shapes::generate_deep(&p("deep_100k.json"), 100_000).unwrap();
    shapes::generate_wide(&p("wide.json"), 200_000, 42, false).unwrap();
    shapes::generate_prim(&p("prim.json"), 1_000_000, 42, true).unwrap();
    shapes::generate_string(&p("string.json"), 10 << 20, 42).unwrap();
    let files = [
        "orders_10m.json",
        "orders_min_10m.json",
        "logs_10m.ndjson",
        "deep_100k.json",
        "wide.json",
        "prim.json",
        "string.json",
    ]
    .map(p)
    .to_vec();
    (dir, files)
}

/// Queries for the fixture checks: (text, case-sensitive, whole word).
const FIXTURE_QUERIES: [(&str, bool, bool); 6] = [
    ("needle", false, false),
    ("NEEDLE_RARE", true, false),
    ("\"id\"", true, true),
    ("e", false, true),
    ("00", true, false),
    ("[[", true, false),
];

fn check_fixture(path: &std::path::Path, chunks: &[usize]) {
    let bytes = std::fs::read(path).unwrap();
    let bytes = Arc::new(bytes);
    for (text, cs, ww) in FIXTURE_QUERIES {
        let want = naive(&bytes, text.as_bytes(), cs, ww);
        for &chunk in chunks {
            let h = search(bytes.clone(), &query(text, cs, ww), chunk, 1000, 8);
            let label = format!("{} {text:?} chunk {chunk}", path.display());
            assert_eq!(h.count(), want.len() as u64, "{label}");
            let stored: Vec<u64> = (0..h.stored()).map(|i| h.get(i).unwrap().offset).collect();
            assert_eq!(stored, want[..want.len().min(1000)], "{label}");
        }
    }
}

#[test]
fn counts_match_on_generated_fixtures() {
    let (_dir, files) = generated_fixtures();
    for f in &files {
        check_fixture(f, &CHUNKS);
    }
}

/// The 100 MB set (`gen-fixtures all --max-size 100m --out fixtures-data/100m`):
/// `cargo test -p braceray-core --release --test search -- --ignored fixtures_100m`.
#[test]
#[ignore = "needs fixtures-data/100m (gen-fixtures all --max-size 100m)"]
fn counts_match_on_fixtures_up_to_100m() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-data/100m");
    let mut n = 0;
    let mut dirs = vec![dir];
    while let Some(d) = dirs.pop() {
        for entry in std::fs::read_dir(&d).expect("fixtures-data/100m") {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name.ends_with("manifest.json") {
                continue;
            }
            check_fixture(&path, &CHUNKS);
            n += 1;
        }
    }
    assert!(n >= 10, "only {n} fixtures checked");
}

#[test]
fn cancel_stops_all_workers_quickly() {
    // 256 MiB of 'a' and the query "a": ~268 M matches, seconds of work in a debug build.
    let hay = Arc::new(vec![b'a'; 256 << 20]);
    let opts = SearchOptions {
        max_stored_results: 1000,
        ..SearchOptions::default()
    };
    let h = start_search(hay, &query("a", true, false), opts, no_repaint()).unwrap();
    std::thread::sleep(Duration::from_millis(30));
    assert!(h.is_running());
    let t = Instant::now();
    h.cancel();
    while h.is_running() {
        std::thread::yield_now();
    }
    let took = t.elapsed();
    assert!(!h.is_done());
    assert!(took < Duration::from_millis(50), "cancel took {took:?}");
    h.wait(); // returns at once
}

#[test]
fn dropping_the_handle_cancels() {
    let hay = Arc::new(vec![b'a'; 64 << 20]);
    let h = start_search(
        hay.clone(),
        &query("a", true, false),
        SearchOptions::default(),
        no_repaint(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(5));
    drop(h);
    // The workers hold the only other references; they let go once stopped.
    let t = Instant::now();
    while Arc::strong_count(&hay) > 1 {
        assert!(
            t.elapsed() < Duration::from_secs(2),
            "workers still running"
        );
        std::thread::yield_now();
    }
}

#[test]
fn searches_a_document() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bom.json");
    std::fs::write(&path, b"\xEF\xBB\xBF[\"needle\", \"NEEDLE\"]").unwrap();
    let doc = braceray_core::Document::open(&path).unwrap();
    let h = start_search(
        doc,
        &SearchQuery::new("needle"),
        SearchOptions::default(),
        no_repaint(),
    )
    .unwrap();
    h.wait();
    // Offsets are into the content after the BOM.
    let offs: Vec<u64> = (0..h.stored()).map(|i| h.get(i).unwrap().offset).collect();
    assert_eq!(offs, [2, 12]);
}
