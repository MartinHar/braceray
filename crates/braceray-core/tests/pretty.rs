//! The pretty printer and original-mode rows against full passes and serde: round trips,
//! the row layout not depending on where formatting starts, seeking, stepping backwards.

mod common;

use braceray_core::index::{self, Index, IndexOptions, LineTable};
use braceray_core::nav::{Navigator, Span};
use braceray_core::pretty::lex::{LexState, state_in_row, tokenize};
use braceray_core::pretty::original::{OrigRow, next_row, prev_row, row_at};
use braceray_core::pretty::{
    Generator, PrettyLine, PrettyOptions, PrettyPos, Seek, Token, TokenKind, anchor_before,
    prev_line, seek,
};
use braceray_core::{CancelToken, Progress};
use common::*;
use proptest::prelude::*;

fn build(bytes: &[u8], budget: u64) -> Index {
    let opts = IndexOptions {
        chunk_size: 4096,
        memory_budget: budget,
        threads: None,
    };
    index::build_bytes(bytes, &opts, &CancelToken::new(), &Progress::default()).unwrap()
}

fn opts(wrap: usize) -> PrettyOptions {
    PrettyOptions { wrap, indent: 2 }
}

fn all_rows(bytes: &[u8], o: &PrettyOptions) -> Vec<PrettyLine> {
    Generator::new(bytes, PrettyPos::START, o).collect()
}

/// Rows joined into text: `\n` between rows, nothing before a continuation row.
fn text_of(bytes: &[u8], rows: &[PrettyLine], indent: u8) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        if i > 0 && !r.is_continuation() {
            out.push(b'\n');
        }
        r.write_text(bytes, indent, &mut out);
    }
    out
}

/// The bytes without whitespace outside strings.
fn squash(bytes: &[u8]) -> Vec<u8> {
    let (mut out, mut in_str, mut esc) = (Vec::new(), false, false);
    for &b in bytes {
        if in_str {
            out.push(b);
            match (esc, b) {
                (true, _) => esc = false,
                (false, b'\\') => esc = true,
                (false, b'"') => in_str = false,
                _ => {}
            }
        } else if !b" \t\r\n".contains(&b) {
            in_str = b == b'"';
            out.push(b);
        }
    }
    out
}

fn found(s: Seek) -> PrettyPos {
    match s {
        Seek::Found(p) => p,
        Seek::More(_) => panic!("unlimited budget returned More"),
    }
}

/// Checks everything about the pretty rows of `bytes` (a valid document).
fn check_pretty(bytes: &[u8], wrap: usize, budget: u64, offsets: &[usize]) {
    let o = opts(wrap);
    let rows = all_rows(bytes, &o);
    // Round trip.
    let text = text_of(bytes, &rows, 2);
    assert_eq!(
        squash(&text),
        squash(bytes),
        "pretty text:\n{}",
        String::from_utf8_lossy(&text)
    );
    // serde rejects some valid numbers (`0.1E310`: out of range); compare values otherwise.
    if let Ok(source) = serde_json::from_slice::<serde_json::Value>(bytes) {
        assert_eq!(serde_value(&text), source);
    }
    // Tiling: consecutive spans; only whitespace outside them.
    assert!(rows.first().is_none_or(|r| r.pos.offset == 0));
    for w in rows.windows(2) {
        assert_eq!(w[0].next, w[1].pos);
    }
    let covered = rows.last().map_or(0, |r| r.next.offset as usize);
    assert!(bytes[covered..].iter().all(|b| b.is_ascii_whitespace()));
    for r in &rows {
        assert!(r.next.offset > r.pos.offset, "empty row {r:?}");
        for t in &r.tokens {
            assert!(t.span.start >= r.pos.offset && t.span.end <= r.next.offset);
        }
        // Rows never split a UTF-8 character.
        assert!(std::str::from_utf8(r.source_span().slice(bytes)).is_ok());
    }

    let index = build(bytes, budget);
    let nav = Navigator::new(bytes, &index);
    let row_of = |x: usize| {
        rows.iter()
            .rposition(|r| r.pos.offset as usize <= x)
            .unwrap_or(0)
    };
    let check_offsets = |nav: &Navigator<'_>| {
        for &x in offsets {
            let x = x.min(bytes.len());
            let a = anchor_before(nav, x as u64);
            assert!(a.offset as usize <= x);
            assert!(
                rows.iter().any(|r| r.pos == a) || (a == PrettyPos::START && rows.is_empty()),
                "anchor {a:?} for {x} is not a row start"
            );
            let got = found(seek(bytes, a, x as u64, usize::MAX, &o));
            assert_eq!(got, rows[row_of(x)].pos, "seek to {x}");
            // A tiny budget resumes to the same result.
            let mut s = seek(bytes, a, x as u64, 8, &o);
            let mut rounds = 0;
            while let Seek::More(state) = s {
                s = state.resume(bytes, 8, &o);
                rounds += 1;
                assert!(rounds < 1_000_000);
            }
            assert_eq!(found(s), got);
        }
    };
    if !rows.is_empty() {
        check_offsets(&nav);
        // With the root's checkpoint table cached, anchors may come from it.
        if let Some(root) = nav.root_container() {
            nav.prepare(&root, &CancelToken::new()).unwrap();
            check_offsets(&nav);
        }
        // Stepping backwards from every row gives the previous row.
        assert_eq!(prev_line(&nav, rows[0].pos, usize::MAX, &o), None);
        for w in rows.windows(2) {
            let p = found(prev_line(&nav, w[1].pos, usize::MAX, &o).unwrap());
            assert_eq!(p, w[0].pos, "row before {:?}", w[1].pos);
        }
    }
}

/// Original-mode rows: tiling, line numbers, row_at / prev_row against a forward pass.
fn check_original(bytes: &[u8], wrap: usize) {
    if bytes.is_empty() {
        return;
    }
    let lines = LineTable::build(bytes);
    let mut rows: Vec<OrigRow> = vec![row_at(bytes, &lines, 0, wrap)];
    while let Some(r) = next_row(bytes, rows.last().unwrap(), wrap) {
        rows.push(r);
        assert!(rows.len() <= bytes.len());
    }
    assert_eq!(rows[0].start, 0);
    assert_eq!(rows.last().unwrap().end as usize, bytes.len());
    for w in rows.windows(2) {
        assert_eq!(w[0].end, w[1].start);
        assert_eq!(prev_row(bytes, &lines, &w[1], wrap), Some(w[0]));
    }
    assert_eq!(prev_row(bytes, &lines, &rows[0], wrap), None);
    for r in &rows {
        let naive_line = 1 + bytes[..r.start as usize]
            .iter()
            .filter(|&&b| b == b'\n')
            .count() as u64;
        assert_eq!(r.line, naive_line);
        assert!(std::str::from_utf8(&bytes[r.start as usize..r.end as usize]).is_ok());
        assert!(r.end - r.start <= wrap as u64 + 5);
        for x in r.start..r.end {
            assert_eq!(row_at(bytes, &lines, x, wrap), *r, "offset {x}");
        }
    }
    // Lexing row by row with the carried state equals lexing everything at once.
    let mut whole = Vec::new();
    tokenize(
        bytes,
        Span {
            start: 0,
            end: bytes.len() as u64,
        },
        LexState::default(),
        &mut whole,
    );
    let mut parts = Vec::new();
    let mut state = LexState::default();
    for r in &rows {
        state = tokenize(
            bytes,
            Span {
                start: r.start,
                end: r.end,
            },
            state,
            &mut parts,
        );
    }
    assert_eq!(merge(parts), merge(whole));
}

/// Joins non-punctuation tokens that touch (a token split across rows; in valid JSON two
/// whole tokens never touch without punctuation or whitespace between them).
fn merge(tokens: Vec<Token>) -> Vec<(u64, u64)> {
    let mut out: Vec<(u64, u64)> = Vec::new();
    let mut last_kind = None;
    for t in tokens {
        match out.last_mut() {
            Some(l)
                if l.1 == t.span.start
                    && t.kind != TokenKind::Punct
                    && last_kind != Some(TokenKind::Punct) =>
            {
                l.1 = t.span.end
            }
            _ => out.push((t.span.start, t.span.end)),
        }
        last_kind = Some(t.kind);
    }
    out
}

#[test]
fn golden_fixtures() {
    let mut files = Vec::new();
    for dir in ["valid", "generated"] {
        for e in std::fs::read_dir(fixture(dir)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "json") {
                files.push(p);
            }
        }
    }
    assert!(files.len() > 10);
    for p in files {
        let bytes = std::fs::read(&p).unwrap();
        let offsets: Vec<usize> = (0..bytes.len()).step_by(bytes.len() / 300 + 1).collect();
        for wrap in [16, 61, 400] {
            check_pretty(&bytes, wrap, u64::MAX, &offsets);
            check_original(&bytes, wrap);
        }
        // Adaptive mode (only shallow containers stored).
        check_pretty(&bytes, 400, (bytes.len() as u64 / 50).max(1), &offsets);
    }
}

#[test]
fn layout_of_known_input() {
    let bytes = br#"{"a":[1,{"b":null},[]],"c":"x\"y","d":{}}"#;
    let rows = all_rows(bytes, &opts(400));
    let text = String::from_utf8(text_of(bytes, &rows, 2)).unwrap();
    let expected = r#"{
  "a": [
    1,
    {
      "b": null
    },
    []
  ],
  "c": "x\"y",
  "d": {}
}"#;
    assert_eq!(text, expected);
    let kinds: Vec<Vec<TokenKind>> = rows
        .iter()
        .map(|r| r.tokens.iter().map(|t| t.kind).collect())
        .collect();
    use TokenKind as K;
    assert_eq!(kinds[1], [K::Key, K::Punct, K::Punct]);
    assert_eq!(kinds[4], [K::Key, K::Punct, K::Literal]);
    assert_eq!(kinds[8], [K::Key, K::Punct, K::String, K::Punct]);
    assert_eq!(kinds[9], [K::Key, K::Punct, K::Punct, K::Punct]);
    let depths: Vec<u32> = rows.iter().map(|r| r.depth).collect();
    assert_eq!(depths, [0, 1, 2, 2, 3, 2, 2, 1, 1, 1, 0]);
}

#[test]
fn long_string_rows_and_state() {
    // {"data": "<5000 bytes with escapes and multi-byte characters>"}
    let mut body = String::new();
    for i in 0..5000 {
        body.push_str(match i % 5 {
            0 => "\\\"",
            1 => "é",
            2 => "\\\\",
            3 => "😀",
            _ => "ab",
        });
    }
    let doc = format!(r#"{{"data": "{body}", "n": 1}}"#);
    let bytes = doc.as_bytes();
    let o = opts(50);
    let rows = all_rows(bytes, &o);
    assert!(rows.len() > 100);
    assert!(rows.iter().filter(|r| r.is_continuation()).count() > 90);
    check_pretty(bytes, 50, u64::MAX, &(0..bytes.len()).collect::<Vec<_>>());
    // state_in_row agrees with a scan from the start at every offset.
    let index = build(bytes, u64::MAX);
    let nav = Navigator::new(bytes, &index);
    let mut state = LexState::default();
    for x in 0..bytes.len() {
        if x % 7 != 0 {
            let mut sink = Vec::new();
            let one = Span {
                start: x as u64,
                end: x as u64 + 1,
            };
            state = tokenize(bytes, one, state, &mut sink);
            continue;
        }
        let row = found(seek(
            bytes,
            anchor_before(&nav, x as u64),
            x as u64,
            usize::MAX,
            &o,
        ));
        let line = Generator::new(bytes, row, &o).next().unwrap();
        assert_eq!(state_in_row(bytes, &line, x as u64), state, "offset {x}");
        let mut sink = Vec::new();
        state = tokenize(
            bytes,
            Span {
                start: x as u64,
                end: x as u64 + 1,
            },
            state,
            &mut sink,
        );
    }
}

#[test]
fn garbage_never_panics() {
    let mut runner =
        proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(cases(1000)));
    runner
        .run(
            &proptest::collection::vec(
                prop_oneof![
                    Just(b'{'),
                    Just(b'}'),
                    Just(b'['),
                    Just(b']'),
                    Just(b'"'),
                    Just(b'\\'),
                    Just(b','),
                    Just(b':'),
                    Just(b' '),
                    Just(b'\n'),
                    any::<u8>()
                ],
                0..300,
            ),
            |bytes| {
                let o = opts(16);
                let rows: Vec<_> = Generator::new(&bytes, PrettyPos::START, &o)
                    .take(10_000)
                    .collect();
                for w in rows.windows(2) {
                    prop_assert_eq!(w[0].next, w[1].pos);
                    prop_assert!(w[0].next.offset > w[0].pos.offset);
                }
                if let Ok(index) = index::build_bytes(
                    &bytes,
                    &IndexOptions::default(),
                    &CancelToken::new(),
                    &Progress::default(),
                ) {
                    let nav = Navigator::new(&bytes, &index);
                    for x in [0, bytes.len() / 3, bytes.len() / 2, bytes.len()] {
                        let a = anchor_before(&nav, x as u64);
                        prop_assert!(a.offset as usize <= x);
                        let _ = seek(&bytes, a, x as u64, usize::MAX, &o);
                    }
                    if let Some(r) = rows.last() {
                        let _ = prev_line(&nav, r.pos, usize::MAX, &o);
                    }
                }
                if !bytes.is_empty() {
                    let lines = LineTable::build(&bytes);
                    let mut r = row_at(&bytes, &lines, 0, 16);
                    let mut n = 0;
                    while let Some(next) = next_row(&bytes, &r, 16) {
                        prop_assert_eq!(prev_row(&bytes, &lines, &next, 16), Some(r));
                        r = next;
                        n += 1;
                        prop_assert!(n <= bytes.len());
                    }
                    let mut sink = Vec::new();
                    tokenize(
                        &bytes,
                        Span {
                            start: 0,
                            end: bytes.len() as u64,
                        },
                        LexState::default(),
                        &mut sink,
                    );
                }
                Ok(())
            },
        )
        .unwrap();
}

fn cases(default: u32) -> u32 {
    std::env::var("PRETTY_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(400)))]

    /// Pretty output re-parsed equals the source value, and rows found by seeking from an
    /// anchor or stepping backwards equal the rows of a full pass, for generated documents
    /// in every layout and several row widths.
    #[test]
    fn generated_documents(tree in json_tree(), layout in layout(), wrap in prop_oneof![Just(16usize), Just(37), Just(400)], seed in any::<u64>()) {
        let (bytes, _) = serialize(&tree, &layout);
        let n = bytes.len().max(1);
        let offsets: Vec<usize> = (0..40u64).map(|i| ((seed.wrapping_mul(i + 1) >> 7) as usize) % (n + 1)).collect();
        check_pretty(&bytes, wrap, u64::MAX, &offsets);
        check_pretty(&bytes, wrap, (bytes.len() as u64 / 8).max(1), &offsets);
        check_original(&bytes, wrap);
    }
}

/// Round trip and seeking on generated fixtures of 10 MB (create them with
/// `gen-fixtures orders --size 10m --out fixtures-data/orders_10m.json`, and `--minified`
/// for `orders_min_10m.json`; `BRACERAY_FIXTURES` overrides the directory).
#[test]
#[ignore = "needs fixtures-data/orders_10m.json and orders_min_10m.json"]
fn fixtures_10m() {
    let dir = std::env::var_os("BRACERAY_FIXTURES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-data")
        });
    for name in ["orders_10m.json", "orders_min_10m.json"] {
        let path = [dir.join(name), dir.join("run1").join(name)]
            .into_iter()
            .find(|p| p.is_file())
            .unwrap_or_else(|| panic!("{name} not found in {}", dir.display()));
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() <= 10_500_000);
        let offsets: Vec<usize> = (0..2000).map(|i| i * (bytes.len() / 2000)).collect();
        let started = std::time::Instant::now();
        check_pretty(&bytes, 400, u64::MAX, &offsets);
        // The app's 25 % budget: the minified file indexes in adaptive mode.
        check_pretty(&bytes, 400, bytes.len() as u64 / 4, &offsets);
        println!("{name}: {:?}", started.elapsed());
    }
}
