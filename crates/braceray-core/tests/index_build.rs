//! The parallel builder must equal the scalar reference exactly: node table, line table
//! and validation result, for every classifier and chunk size.

mod common;

use std::fs;

use braceray_core::index::{self, Index, IndexOptions, scalar, simd};
use braceray_core::{CancelToken, Progress, Validation};
use common::*;
use proptest::prelude::*;

const CHUNK_SIZES: [usize; 5] = [64, 65, 127, 4096, 8 << 20];

fn reference(bytes: &[u8], max_depth: Option<u32>) -> Index {
    scalar::build(bytes, max_depth, &CancelToken::new(), &Progress::default()).unwrap()
}

fn fast_with(imp: simd::Simd, bytes: &[u8], opts: &IndexOptions) -> Index {
    index::build_bytes_with(imp, bytes, opts, &CancelToken::new(), &Progress::default()).unwrap()
}

/// Builds with every classifier × chunk size and compares with the reference.
fn check_equivalent(bytes: &[u8], name: &str) -> Result<(), TestCaseError> {
    let want = reference(bytes, None);
    for imp in simd::available() {
        for chunk_size in CHUNK_SIZES {
            let opts = IndexOptions {
                chunk_size,
                ..IndexOptions::default()
            };
            let got = fast_with(imp, bytes, &opts);
            prop_assert_eq!(
                &got,
                &want,
                "{} with {} chunk {}: {:?}",
                name,
                imp.name,
                chunk_size,
                String::from_utf8_lossy(&bytes[..bytes.len().min(200)])
            );
        }
    }
    Ok(())
}

#[test]
fn golden_fixtures() {
    for dir in ["valid", "invalid", "generated", "encoding"] {
        for entry in fs::read_dir(fixture(dir)).unwrap() {
            let path = entry.unwrap().path();
            let name = path.display().to_string();
            check_equivalent(&fs::read(&path).unwrap(), &name).unwrap();
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
        let idx = index::build_bytes(
            &bytes,
            &IndexOptions::default(),
            &CancelToken::new(),
            &Progress::default(),
        )
        .unwrap();
        let Validation::Invalid(e) = idx.validation() else {
            panic!("{file} should be invalid");
        };
        assert_eq!(e.offset, case["error_offset"].as_u64().unwrap(), "{file}");
        assert_eq!(e.line, case["line"].as_u64().unwrap(), "{file}");
    }
}

#[test]
fn deep_and_wide_shapes() {
    let dir = tempfile::tempdir().unwrap();
    let deep = dir.path().join("deep.json");
    gen_fixtures::shapes::generate_deep(&deep, 100_000).unwrap();
    let wide = dir.path().join("wide.json");
    gen_fixtures::shapes::generate_wide(&wide, 20_000, 1, false).unwrap();
    let string = dir.path().join("string.json");
    gen_fixtures::shapes::generate_string(&string, 300_000, 1).unwrap();
    for path in [&deep, &wide, &string] {
        let bytes = fs::read(path).unwrap();
        for chunk_size in [64, 4096, 8 << 20] {
            let opts = IndexOptions {
                chunk_size,
                ..IndexOptions::default()
            };
            let got = fast_with(simd::best(), &bytes, &opts);
            assert!(got.is_valid(), "{path:?}");
            assert_eq!(got, reference(&bytes, None), "{path:?} chunk {chunk_size}");
        }
    }
    let idx = fast_with(
        simd::best(),
        &fs::read(&deep).unwrap(),
        &IndexOptions::default(),
    );
    assert_eq!(idx.nodes().len(), 100_000);
}

#[test]
fn orders_pretty_and_minified() {
    let dir = tempfile::tempdir().unwrap();
    for minified in [false, true] {
        let path = dir.path().join(format!("orders_{minified}.json"));
        let opts = gen_fixtures::orders::RecordsOpts {
            size: 4 << 20,
            seed: 7,
            minified,
        };
        gen_fixtures::orders::generate_orders(&path, opts).unwrap();
        let bytes = fs::read(&path).unwrap();
        let want = reference(&bytes, None);
        for chunk_size in [64, 127, 4096, 1 << 20] {
            let opts = IndexOptions {
                chunk_size,
                threads: Some(4),
                ..IndexOptions::default()
            };
            let got = index::build_bytes(&bytes, &opts, &CancelToken::new(), &Progress::default())
                .unwrap();
            assert_eq!(got, want, "minified {minified} chunk {chunk_size}");
        }
    }
}

#[test]
fn adaptive_mode_on_tiny_objects() {
    let mut bytes = b"[".to_vec();
    for i in 0..200_000 {
        if i > 0 {
            bytes.push(b',');
        }
        bytes.extend_from_slice(br#"{"a":1}"#);
    }
    bytes.push(b']');
    // 200,001 nodes × 24 B = 4.8 MB; allow 1 MB.
    let opts = IndexOptions {
        chunk_size: 64 << 10,
        memory_budget: 1 << 20,
        threads: None,
    };
    let got = index::build_bytes(&bytes, &opts, &CancelToken::new(), &Progress::default()).unwrap();
    assert!(got.is_valid());
    assert_eq!(got.max_stored_depth(), Some(0));
    assert_eq!(got.nodes().len(), 1);
    assert_eq!(got.nodes()[0].child_count(), 200_000);
    assert_eq!(got, reference(&bytes, Some(0)));
    assert!(got.memory_bytes() <= 1 << 20);

    // Mixed depths: only the levels that fit are kept.
    let mut bytes = b"[".to_vec();
    for i in 0..5_000 {
        if i > 0 {
            bytes.push(b',');
        }
        bytes.extend_from_slice(br#"{"a":[[1],[2],[3],[4]]}"#);
    }
    bytes.push(b']');
    let opts = IndexOptions {
        chunk_size: 4096,
        memory_budget: 24 * 20_000,
        threads: None,
    };
    let got = index::build_bytes(&bytes, &opts, &CancelToken::new(), &Progress::default()).unwrap();
    let depth = got.max_stored_depth().unwrap();
    assert!(depth >= 1, "depth {depth}");
    assert_eq!(got, reference(&bytes, Some(depth)));
}

#[test]
fn progress_reaches_the_total() {
    let bytes = br#"{"a": [1, 2, 3], "b": "x"}"#;
    let p = Progress::default();
    index::build_bytes(bytes, &IndexOptions::default(), &CancelToken::new(), &p).unwrap();
    assert_eq!(p.done(), p.total());
    assert_eq!(p.total(), 2 * bytes.len() as u64);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn generated_json_equals_reference(tree in json_tree(), layout in layout()) {
        let (bytes, _) = serialize(&tree, &layout);
        check_equivalent(&bytes, "generated")?;
    }

    #[test]
    fn mutated_json_equals_reference(
        tree in json_tree(),
        layout in layout(),
        op in 0u8..3,
        pos in any::<usize>(),
        byte in mutation_byte(),
    ) {
        let (bytes, _) = serialize(&tree, &layout);
        check_equivalent(&mutate(&bytes, op, pos, byte), "mutated")?;
    }

    #[test]
    fn random_bytes_equal_reference(bytes in prop::collection::vec(
        prop_oneof![
            8 => prop::sample::select(&b"{}[],:\" \\n0-1.etrufalsn\\\\u"[..]),
            1 => any::<u8>(),
        ],
        0..300,
    )) {
        check_equivalent(&bytes, "random")?;
    }
}

/// Files over 4 MiB with an error in the first MiB take the early-error path (I-018); the
/// result must still equal the reference, and errors after the first MiB, or a valid
/// prefix, go through the normal build.
#[test]
fn early_error_path_equals_reference() {
    let mut valid = b"[".to_vec();
    while valid.len() < 5 << 20 {
        valid.extend_from_slice(b"{\"k\": \"v\\n\", \"n\": [1, 2.5e3, null]},\n");
    }
    valid.extend_from_slice(b"0]\n");
    let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
    // `[]` then spaces then NUL bytes (the 5 GiB sparse fixture in small).
    let mut sparse = b"[]".to_vec();
    sparse.resize(8194, b' ');
    sparse.resize(5 << 20, 0);
    cases.push(("sparse", sparse));
    // Not JSON from the first byte: CSV.
    cases.push(("csv", b"a,b,c\n1,2,3\n".repeat(500_000)));
    // Error at 1000 bytes in, inside a string that is still open at the probe limit.
    let mut early = valid.clone();
    early[1000] = b'}';
    cases.push(("early", early));
    let mut open_string = valid.clone();
    open_string[100] = b'"';
    cases.push(("string over the probe", open_string));
    // Error just past the probe.
    let mut late = valid.clone();
    let at = (1 << 20) + 17;
    late[at] = b'@';
    cases.push(("late", late));
    cases.push(("valid", valid));
    for (name, bytes) in cases {
        let want = reference(&bytes, None);
        let got = index::build_bytes(
            &bytes,
            &IndexOptions::default(),
            &CancelToken::new(),
            &Progress::default(),
        )
        .unwrap();
        assert_eq!(got, want, "{name}");
    }
}
