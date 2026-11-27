//! Step 10 core checks: the JSONPath of each of the first 1000 matches equals the path a
//! serde walk of the document gives; `matches_in` agrees with the stored matches (also
//! beyond a tiny cap); `SearchNav` works from another thread.

use std::sync::Arc;

use braceray_core::index::{IndexOptions, build_bytes};
use braceray_core::job::no_repaint;
use braceray_core::nav::{Navigator, PathSegment};
use braceray_core::path::{decode_key, jsonpath};
use braceray_core::search::{Match, SearchHandle, SearchOptions, SearchQuery, start_search};
use braceray_core::{CancelToken, Progress};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    Key(String),
    Index(u64),
}

/// Paths of every occurrence of `needle` in keys and string values, in document order,
/// from a serde walk (explicit stack). A key match has its member's path. `needle` must
/// not occur in literals, numbers or JSON punctuation, and not be changed by escaping.
fn serde_paths(doc: &Value, needle: &str) -> Vec<Vec<Seg>> {
    let mut out = Vec::new();
    let count = |s: &str| s.matches(needle).count();
    // (value, its path); children pushed in reverse so they pop in document order.
    let mut stack: Vec<(&Value, Vec<Seg>)> = vec![(doc, Vec::new())];
    while let Some((v, path)) = stack.pop() {
        match v {
            Value::String(s) => out.extend(std::iter::repeat_n(path.clone(), count(s))),
            Value::Array(items) => {
                for (i, item) in items.iter().enumerate().rev() {
                    let mut p = path.clone();
                    p.push(Seg::Index(i as u64));
                    stack.push((item, p));
                }
            }
            Value::Object(map) => {
                // Keys come before their values: push each member as a marker for the
                // key, then the value, in reverse.
                let members: Vec<_> = map.iter().collect();
                for (k, item) in members.into_iter().rev() {
                    let mut p = path.clone();
                    p.push(Seg::Key(k.clone()));
                    stack.push((item, p.clone()));
                    // A key marker: a string value holding the key at the same path.
                    if count(k) > 0 {
                        out_key_marker(&mut stack, k, p);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Keys are pushed as synthetic string values so they pop right before their value.
fn out_key_marker<'a>(stack: &mut Vec<(&'a Value, Vec<Seg>)>, key: &str, path: Vec<Seg>) {
    // Leak a tiny string value per matching key (test only, small inputs).
    let v: &'a Value = Box::leak(Box::new(Value::String(key.to_owned())));
    stack.push((v, path));
}

fn our_paths(bytes: &[u8], search: &SearchHandle, n: usize) -> (Vec<Vec<Seg>>, Vec<String>) {
    let index = build_bytes(
        bytes,
        &IndexOptions::default(),
        &CancelToken::new(),
        &Progress::default(),
    )
    .unwrap();
    let nav = Navigator::new(bytes, &index);
    let mut segs = Vec::new();
    let mut texts = Vec::new();
    for i in 0..search.stored().min(n) {
        let m = search.get(i).unwrap();
        let path = nav.path_of(&nav.node_at_offset(m.offset));
        texts.push(jsonpath(bytes, &path));
        segs.push(
            path.iter()
                .map(|s| match s {
                    PathSegment::Key(span) => Seg::Key(decode_key(span.slice(bytes))),
                    PathSegment::Index(i) => Seg::Index(*i),
                })
                .collect(),
        );
    }
    (segs, texts)
}

fn search(bytes: Vec<u8>, needle: &str, options: SearchOptions) -> SearchHandle {
    let mut q = SearchQuery::new(needle);
    q.case_sensitive = true;
    let s = start_search(Arc::new(bytes), &q, options, no_repaint()).unwrap();
    s.wait();
    s
}

fn check_paths(bytes: &[u8], needle: &str) -> usize {
    let doc: Value = serde_json::from_slice(bytes).unwrap();
    let expected = serde_paths(&doc, needle);
    let s = search(bytes.to_vec(), needle, SearchOptions::default());
    assert_eq!(s.count() as usize, expected.len(), "count of {needle:?}");
    let (ours, texts) = our_paths(bytes, &s, 1000);
    let n = ours.len();
    assert_eq!(ours[..], expected[..n], "paths of {needle:?}");
    assert!(texts.iter().all(|t| t.starts_with('$')));
    n
}

#[test]
fn paths_of_the_first_1000_matches_equal_a_serde_walk() {
    let dir = tempfile::tempdir().unwrap();
    for minified in [false, true] {
        let path = dir.path().join(format!("orders_{minified}.json"));
        gen_fixtures::orders::generate_orders(
            &path,
            gen_fixtures::orders::RecordsOpts {
                size: 600_000,
                seed: 7,
                minified,
            },
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // "o" and "id" never occur in literals or numbers.
        for needle in ["o", "id", "needle_common", "@"] {
            let n = check_paths(&bytes, needle);
            assert!(n > 0, "no matches of {needle:?}");
        }
        assert_eq!(check_paths(&bytes, "o"), 1000);
    }
}

#[test]
fn paths_in_hand_written_shapes() {
    let doc = r#"{"o": [1, "oo", {"k o": ["o", [["o"]], {"": "o"}], "öo": "o", "é o": 1}],
        "x": {"y": {"z": [[], {}, "foo", {"o\"o": "bo"}]}}, "o2": "o"}"#
        .as_bytes();
    check_paths(doc, "o");
    let top = br#""a string with o and o""#;
    check_paths(top, "o");
}

#[test]
fn matches_in_agrees_with_the_store_and_beyond_a_cap() {
    let mut bytes = b"[".to_vec();
    for i in 0..5000 {
        bytes.extend_from_slice(format!("\"x{i} ab\", ").as_bytes());
    }
    bytes.extend_from_slice(b"0]");
    let all = search(bytes.clone(), "ab", SearchOptions::default());
    let every: Vec<Match> = (0..all.stored()).map(|i| all.get(i).unwrap()).collect();
    assert_eq!(every.len(), 5000);
    for cap in [0, 1, 10, 4999, 5_000_000] {
        for chunk in [64, 4096, 8 << 20] {
            let s = search(
                bytes.clone(),
                "ab",
                SearchOptions {
                    chunk_size: chunk,
                    max_stored_results: cap,
                    threads: None,
                },
            );
            for (a, b) in [(0, 50), (100, 4000), (30_000, 30_100), (0, u64::MAX)] {
                let got = s.matches_in(a, b, usize::MAX);
                let want: Vec<Match> = every
                    .iter()
                    .copied()
                    .filter(|m| m.offset >= a && m.offset < b)
                    .collect();
                assert_eq!(got, want, "cap {cap}, chunk {chunk}, range {a}..{b}");
            }
            assert_eq!(s.matches_in(0, u64::MAX, 3), every[..3]);
            // Next / previous from another thread.
            let nav = s.nav();
            let last = every[every.len() - 1];
            let (next, prev) =
                std::thread::spawn(move || (nav.next_after(0), nav.prev_before(u64::MAX)))
                    .join()
                    .unwrap();
            assert_eq!(next, Some(every[0]));
            assert_eq!(prev, Some(last));
        }
    }
}

#[test]
fn searches_run_while_the_global_pool_is_busy() {
    // Occupy every global rayon thread; a search must still finish (own pool, I-014).
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let threads = rayon::current_num_threads();
    let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for _ in 0..threads {
        let (r, st) = (release.clone(), started.clone());
        rayon::spawn(move || {
            st.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            while !r.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });
    }
    while started.load(std::sync::atomic::Ordering::SeqCst) < threads {
        std::thread::yield_now();
    }
    let t = std::time::Instant::now();
    let s = search(
        b"[\"needle\", \"needle\"]".to_vec(),
        "needle",
        SearchOptions::default(),
    );
    release.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(s.count(), 2);
    assert!(t.elapsed() < std::time::Duration::from_secs(5));
}
