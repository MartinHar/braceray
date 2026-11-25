//! The scalar reference indexer against independent oracles: the generator's own node
//! table, `serde_json` validity, the invalid-fixture manifest and big generated fixtures.

mod common;

use std::fs;

use braceray_core::index::{ContainerKind, Index, scalar};
use braceray_core::{CancelToken, Error, Progress, Validation};
use common::*;
use proptest::prelude::*;

fn index(bytes: &[u8]) -> Index {
    scalar::build(bytes, None, &CancelToken::new(), &Progress::default()).unwrap()
}

/// serde (parse and drop) recurses per nesting level, so run it on a big stack.
fn assert_matches_serde(bytes: &[u8], name: &str) {
    let (bytes, name) = (bytes.to_vec(), name.to_owned());
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || matches_serde(&bytes, &name))
        .unwrap()
        .join()
        .unwrap();
}

fn matches_serde(bytes: &[u8], name: &str) {
    let idx = index(bytes);
    assert_eq!(idx.validation(), &Validation::Valid, "{name}");
    let value = serde_value(bytes);
    let ours: Vec<_> = idx
        .nodes()
        .iter()
        .map(|n| (n.kind(), n.child_count()))
        .collect();
    assert_eq!(ours, serde_containers(&value), "{name}");
    for n in idx.nodes() {
        let (open, close) = match n.kind() {
            ContainerKind::Object => (b'{', b'}'),
            ContainerKind::Array => (b'[', b']'),
        };
        assert_eq!(bytes[n.start() as usize], open, "{name}");
        assert_eq!(bytes[n.end() as usize], close, "{name}");
    }
}

#[test]
fn golden_valid_and_generated_fixtures() {
    for dir in ["valid", "generated"] {
        for entry in fs::read_dir(fixture(dir)).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name.ends_with(".manifest.json") {
                continue;
            }
            let bytes = fs::read(&path).unwrap();
            if name.ends_with(".ndjson") {
                for line in bytes.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
                    assert_matches_serde(line, &name);
                }
            } else {
                assert_matches_serde(&bytes, &name);
            }
        }
    }
}

#[test]
fn invalid_fixtures_match_the_manifest() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture("invalid/invalid_manifest.json")).unwrap())
            .unwrap();
    for case in manifest.as_array().unwrap() {
        let file = case["file"].as_str().unwrap();
        let bytes = fs::read(fixture(&format!("invalid/{file}"))).unwrap();
        let Validation::Invalid(e) = index(&bytes).validation().clone() else {
            panic!("{file} should be invalid");
        };
        assert_eq!(
            e.offset,
            case["error_offset"].as_u64().unwrap(),
            "{file}: {e}"
        );
        assert_eq!(e.line, case["line"].as_u64().unwrap(), "{file}: {e}");
        // The manifest's column counts bytes; ours counts characters.
        let line_start = bytes[..e.offset as usize]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |p| p + 1);
        let byte_col = e.offset - line_start as u64 + 1;
        assert_eq!(byte_col, case["column"].as_u64().unwrap(), "{file}: {e}");
        let chars = String::from_utf8_lossy(&bytes[line_start..e.offset as usize])
            .chars()
            .count() as u64;
        assert_eq!(e.column, chars + 1, "{file}: {e}");
        assert_eq!(e.found, bytes.get(e.offset as usize).copied());
    }
}

#[test]
fn error_messages() {
    let idx = index(r#"{"name": "Արամ" "x": 1}"#.as_bytes());
    let Validation::Invalid(e) = idx.validation() else {
        panic!()
    };
    assert_eq!(e.column, 17);
    assert_eq!(
        e.to_string(),
        "Invalid JSON at line 1, column 17 (offset 20): expected ',' or '}', found '\"'"
    );
    // Unclosed containers end at the error and have an unknown child count.
    let idx = index(b"[[1, 2], {\"a\": [tru");
    assert_eq!(idx.nodes().len(), 4);
    let err = 19; // EOF where "true" expects its 'e'
    assert!(
        matches!(idx.validation(), Validation::Invalid(e) if e.offset == err && e.expected == "'true'")
    );
    assert_eq!(idx.nodes()[0].end(), err);
    assert_eq!(idx.nodes()[1].end(), 6);
    assert_eq!(idx.nodes()[1].child_count(), 2);
    for n in [0, 2, 3] {
        assert_eq!(idx.nodes()[n].end(), err);
        assert_eq!(idx.nodes()[n].child_count(), u32::MAX);
    }
}

#[test]
fn deep_100k_has_no_stack_overflow() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deep_100k.json");
    gen_fixtures::shapes::generate_deep(&path, 100_000).unwrap();
    let bytes = fs::read(&path).unwrap();
    let idx = index(&bytes);
    assert!(idx.is_valid());
    assert_eq!(idx.nodes().len(), 100_000);
    assert_eq!(idx.nodes().last().unwrap().depth(), 99_999);
    // A million unclosed brackets: invalid at EOF, still no recursion.
    let idx = index(&vec![b'['; 1_000_000]);
    assert_eq!(idx.nodes().len(), 1_000_000);
    assert!(matches!(idx.validation(), Validation::Invalid(e) if e.offset == 1_000_000));
}

#[test]
fn orders_10m_pretty_and_minified() {
    let dir = tempfile::tempdir().unwrap();
    for minified in [false, true] {
        let path = dir.path().join(format!("orders_{minified}.json"));
        let opts = gen_fixtures::orders::RecordsOpts {
            size: 10 << 20,
            seed: 42,
            minified,
        };
        gen_fixtures::orders::generate_orders(&path, opts).unwrap();
        assert_matches_serde(&fs::read(&path).unwrap(), "orders 10m");
    }
}

#[test]
fn adaptive_depth_limit_keeps_shallow_nodes() {
    let bytes = br#"[{"a": [1, {"b": []}]}, [[]], {}]"#;
    let full = index(bytes);
    let cut = scalar::build(bytes, Some(1), &CancelToken::new(), &Progress::default()).unwrap();
    let shallow: Vec<_> = full
        .nodes()
        .iter()
        .filter(|n| n.depth() <= 1)
        .map(|n| (n.start(), n.end(), n.depth(), n.child_count()))
        .collect();
    let got: Vec<_> = cut
        .nodes()
        .iter()
        .map(|n| (n.start(), n.end(), n.depth(), n.child_count()))
        .collect();
    assert_eq!(got, shallow);
    assert_eq!(cut.max_stored_depth(), Some(1));
    // Parents point at stored ids.
    assert!(cut.nodes()[1..].iter().all(|n| n.parent() == 0));
}

#[test]
fn cancel_and_progress() {
    let bytes = vec![b' '; 8 << 20];
    let cancel = CancelToken::new();
    cancel.cancel();
    let r = scalar::build(&bytes, None, &cancel, &Progress::default());
    assert!(matches!(r, Err(Error::Cancelled)));
    let p = Progress::default();
    let idx = scalar::build(b"[1]", None, &CancelToken::new(), &p).unwrap();
    assert!(idx.is_valid());
    assert_eq!((p.done(), p.total()), (3, 3));
}

#[test]
fn edge_cases() {
    for ok in [
        &b"0"[..],
        b"-0",
        b"-0.0e+0",
        b"1E9",
        b"\"\"",
        b" [ ] ",
        b"{\"\":{}}",
        b"[[[]]]",
        b"\"\\u12aF\"",
        b"\"\\/\"",
        b"null\n",
    ] {
        assert!(index(ok).is_valid(), "{:?}", String::from_utf8_lossy(ok));
        assert!(serde_validity(ok).is_ok());
    }
    for bad in [
        &b""[..],
        b"01",
        b"-",
        b"1.",
        b".5",
        b"1e",
        b"+1",
        b"[1,]",
        b"{,}",
        b"{\"a\"}",
        b"\"\\x\"",
        b"\"\\u12g4\"",
        b"tru",
        b"nul",
        b"[1 2]",
        b"{} {}",
        b"\x0c[]",
        b"[}",
        b"{]",
        b"\"a\nb\"",
        b"'a'",
        b"[1,,2]",
        b"{\"a\":1,}",
        b"NaN",
        b"-Infinity",
    ] {
        assert!(!index(bad).is_valid(), "{:?}", String::from_utf8_lossy(bad));
        assert!(
            serde_validity(bad).is_err(),
            "{:?}",
            String::from_utf8_lossy(bad)
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn generated_json_gives_the_generator_node_table(tree in json_tree(), layout in layout()) {
        let (bytes, expected) = serialize(&tree, &layout);
        let idx = index(&bytes);
        prop_assert_eq!(idx.validation(), &Validation::Valid);
        prop_assert_eq!(idx.nodes(), &expected[..]);
        prop_assert!(serde_validity(&bytes).is_ok());
    }

    #[test]
    fn mutated_json_validity_agrees_with_serde(
        tree in json_tree(),
        layout in layout(),
        op in 0u8..3,
        pos in any::<usize>(),
        byte in mutation_byte(),
    ) {
        let (bytes, _) = serialize(&tree, &layout);
        let bytes = mutate(&bytes, op, pos, byte);
        let idx = index(&bytes);
        match (idx.validation(), serde_validity(&bytes)) {
            (Validation::Valid, Ok(())) => {}
            (Validation::Invalid(e), Err((line, col))) => {
                // Not after serde's position (serde reports the byte before a raw control
                // character, hence the +1).
                let serde_off = serde_offset(&bytes, line, col);
                prop_assert!(e.offset <= serde_off + 1, "{} vs serde {}:{} (offset {})", e, line, col, serde_off);
            }
            (ours, serde) => prop_assert!(
                false,
                "disagree on {:?}: ours {:?}, serde {:?}",
                String::from_utf8_lossy(&bytes),
                ours,
                serde
            ),
        }
    }
}
