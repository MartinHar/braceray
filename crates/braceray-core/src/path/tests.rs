//! Path formatting: tables per format, decoding, and resolving formatted paths with
//! reference implementations (`serde_json_path`, `Value::pointer`, and `jq` / `node`
//! when installed).

use std::fmt::Write as _;

use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;
use crate::index::{IndexOptions, build_bytes};
use crate::nav::{Navigator, Span};
use crate::{CancelToken, Progress};

/// One step of a test path, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Key(String),
    Index(usize),
}

/// The real path segments (key spans into `bytes`) of `steps`, found by walking the
/// document with a navigator the way the tree does.
fn segments(bytes: &[u8], steps: &[Step]) -> Vec<PathSegment> {
    let index = build_bytes(
        bytes,
        &IndexOptions::default(),
        &CancelToken::new(),
        &Progress::default(),
    )
    .unwrap();
    let nav = Navigator::new(bytes, &index);
    let mut child = nav.root().unwrap();
    let mut out = Vec::new();
    for step in steps {
        let c = nav.container_of(&child).unwrap();
        let kids = nav.children(&c, 0..nav.child_count(&c));
        child = match step {
            Step::Index(i) => {
                out.push(PathSegment::Index(*i as u64));
                kids[*i]
            }
            Step::Key(k) => {
                let found = kids
                    .iter()
                    .find(|ch| decode_key(ch.key.unwrap().slice(bytes)) == *k)
                    .copied()
                    .unwrap();
                out.push(PathSegment::Key(found.key.unwrap()));
                found
            }
        };
    }
    out
}

fn fmt(bytes: &[u8], steps: &[Step], format: PathFormat) -> String {
    format_path(bytes, &segments(bytes, steps), format, DEFAULT_JS_ROOT)
}

/// Keys that need care in at least one format.
const KEYS: &[&str] = &[
    "plain",
    "_under",
    "$ref",
    "with space",
    "it's",
    "say \"hi\"",
    "a.b",
    "a/b",
    "~tilde",
    "back\\slash",
    "é",
    "日本",
    "😀",
    "",
    "123",
    "1e5",
    "-1",
    "2x",
    "class",
    "if",
    "null",
    "true",
    "tab\there",
    "nl\nx",
    "\u{1}",
    "\u{7f}",
    "\u{2028}",
    "[0]",
    "'",
    "\"",
];

/// `{"arr": [{key: i, …}], key: i, …}`: every key at the top and under `arr[0]`.
fn table_doc() -> Value {
    let mut top = serde_json::Map::new();
    let mut inner = serde_json::Map::new();
    for (i, k) in KEYS.iter().enumerate() {
        top.insert((*k).to_owned(), json!(i));
        inner.insert((*k).to_owned(), json!(i + 1000));
    }
    top.insert("arr".to_owned(), json!([Value::Object(inner)]));
    Value::Object(top)
}

fn table_paths() -> Vec<(Vec<Step>, Value)> {
    let mut out = Vec::new();
    for (i, k) in KEYS.iter().enumerate() {
        out.push((vec![Step::Key((*k).to_owned())], json!(i)));
        out.push((
            vec![
                Step::Key("arr".to_owned()),
                Step::Index(0),
                Step::Key((*k).to_owned()),
            ],
            json!(i + 1000),
        ));
    }
    out.push((vec![], table_doc()));
    out.push((vec![Step::Key("arr".to_owned()), Step::Index(0)], {
        table_doc()["arr"][0].clone()
    }));
    out
}

#[test]
fn expected_text_per_format() {
    let doc = table_doc();
    let bytes = serde_json::to_vec(&doc).unwrap();
    let cases: &[(&str, [&str; 4])] = &[
        ("plain", ["$.plain", ".plain", "data.plain", "/plain"]),
        ("_under", ["$._under", "._under", "data._under", "/_under"]),
        ("$ref", ["$['$ref']", r#".["$ref"]"#, "data.$ref", "/$ref"]),
        (
            "with space",
            [
                "$['with space']",
                r#".["with space"]"#,
                r#"data["with space"]"#,
                "/with space",
            ],
        ),
        (
            "it's",
            [r"$['it\'s']", r#".["it's"]"#, r#"data["it's"]"#, "/it's"],
        ),
        (
            "say \"hi\"",
            [
                r#"$['say "hi"']"#,
                r#".["say \"hi\""]"#,
                r#"data["say \"hi\""]"#,
                r#"/say "hi""#,
            ],
        ),
        ("a.b", ["$['a.b']", r#".["a.b"]"#, r#"data["a.b"]"#, "/a.b"]),
        (
            "a/b",
            ["$['a/b']", r#".["a/b"]"#, r#"data["a/b"]"#, "/a~1b"],
        ),
        (
            "~tilde",
            [
                "$['~tilde']",
                r#".["~tilde"]"#,
                r#"data["~tilde"]"#,
                "/~0tilde",
            ],
        ),
        (
            "back\\slash",
            [
                r"$['back\\slash']",
                r#".["back\\slash"]"#,
                r#"data["back\\slash"]"#,
                r"/back\slash",
            ],
        ),
        ("é", ["$['é']", r#".["é"]"#, r#"data["é"]"#, "/é"]),
        ("", ["$['']", r#".[""]"#, r#"data[""]"#, "/"]),
        ("123", ["$['123']", r#".["123"]"#, r#"data["123"]"#, "/123"]),
        ("class", ["$.class", ".class", r#"data["class"]"#, "/class"]),
        ("if", ["$.if", r#".["if"]"#, r#"data["if"]"#, "/if"]),
        (
            "tab\there",
            [
                r"$['tab\there']",
                r#".["tab\there"]"#,
                r#"data["tab\there"]"#,
                "/tab\there",
            ],
        ),
        (
            "\u{1}",
            [
                r"$['\u0001']",
                r#".["\u0001"]"#,
                r#"data["\u0001"]"#,
                "/\u{1}",
            ],
        ),
        (
            "\u{2028}",
            [
                "$['\u{2028}']",
                ".[\"\u{2028}\"]",
                r#"data["\u2028"]"#,
                "/\u{2028}",
            ],
        ),
    ];
    for (key, expected) in cases {
        let steps = [Step::Key((*key).to_owned())];
        for (format, want) in PathFormat::ALL.iter().zip(expected) {
            assert_eq!(fmt(&bytes, &steps, *format), *want, "{format:?} of {key:?}");
        }
    }
    // Nested: brackets after a step, and the roots.
    let nested = [
        Step::Key("arr".to_owned()),
        Step::Index(0),
        Step::Key("with space".to_owned()),
    ];
    let want = [
        "$.arr[0]['with space']",
        r#".arr[0]["with space"]"#,
        r#"data.arr[0]["with space"]"#,
        "/arr/0/with space",
    ];
    for (format, want) in PathFormat::ALL.iter().zip(want) {
        assert_eq!(fmt(&bytes, &nested, *format), want);
    }
    let roots = ["$", ".", "data", ""];
    for (format, want) in PathFormat::ALL.iter().zip(roots) {
        assert_eq!(fmt(&bytes, &[], *format), want);
    }
    let top_array = b"[[1, 2]]";
    let path = [PathSegment::Index(0), PathSegment::Index(1)];
    let want = ["$[0][1]", ".[0][1]", "data[0][1]", "/0/1"];
    for (format, want) in PathFormat::ALL.iter().zip(want) {
        assert_eq!(format_path(top_array, &path, *format, "data"), want);
    }
    assert_eq!(
        format_path(top_array, &path, PathFormat::JavaScript, "doc"),
        "doc[0][1]"
    );
}

#[test]
fn escaped_keys_are_decoded_first() {
    let bytes = br#"{"a\u0020b": {"\u00e9\"": {"\ud83d\ude00": {"x\/y": 1}}}}"#;
    let steps = [
        Step::Key("a b".to_owned()),
        Step::Key("é\"".to_owned()),
        Step::Key("😀".to_owned()),
        Step::Key("x/y".to_owned()),
    ];
    assert_eq!(
        fmt(bytes, &steps, PathFormat::JsonPath),
        r#"$['a b']['é"']['😀']['x/y']"#
    );
    assert_eq!(
        fmt(bytes, &steps, PathFormat::Pointer),
        r#"/a b/é"/😀/x~1y"#
    );
}

#[test]
fn decoding_escapes() {
    assert_eq!(decode_key(br#""a\nb\t\/""#), "a\nb\t/");
    assert_eq!(decode_key(br#""\ud83d\ude00""#), "😀");
    assert_eq!(decode_key(br#""\ud83d x""#), "\u{FFFD} x");
    assert_eq!(decode_key(br#""\q\u12""#), "\\q\\u12");
    assert_eq!(decode_key(b"\"tail\\\""), "tail\\");
    assert_eq!(
        jsonpath(
            b"\"\\u0001\"",
            &[PathSegment::Key(Span { start: 0, end: 8 })]
        ),
        "$['\\u0001']"
    );
}

#[test]
fn decoding_checks_and_spans_windows() {
    // An escape right at a check boundary, and a big plain run.
    let mut inner = vec![b'a'; CHECK_EVERY - 1];
    inner.extend_from_slice(br"\u00e9");
    inner.extend(std::iter::repeat_n(b'b', CHECK_EVERY + 3));
    let mut out = Vec::new();
    let mut checks = 0;
    decode_escapes(&inner, &mut out, |_| {
        checks += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(checks, 2);
    assert_eq!(out.len(), CHECK_EVERY - 1 + 2 + CHECK_EVERY + 3);
    assert_eq!(&out[CHECK_EVERY - 1..CHECK_EVERY + 1], "é".as_bytes());
    let err = decode_escapes(&inner, &mut Vec::new(), |_| Err(crate::Error::Cancelled));
    assert!(matches!(err, Err(crate::Error::Cancelled)));
}

/// Resolves `path` with the reference implementations; `None` if it doesn't resolve.
fn resolve(doc: &Value, path: &str, format: PathFormat) -> Option<Value> {
    match format {
        PathFormat::JsonPath => {
            let p = serde_json_path::JsonPath::parse(path)
                .unwrap_or_else(|e| panic!("{path:?} isn't valid RFC 9535 JSONPath: {e}"));
            p.query(doc).exactly_one().ok().cloned()
        }
        PathFormat::Pointer => doc.pointer(path).cloned(),
        PathFormat::Jq | PathFormat::JavaScript => unreachable!("external tools"),
    }
}

#[test]
fn table_paths_resolve_with_reference_implementations() {
    let doc = table_doc();
    let bytes = serde_json::to_vec(&doc).unwrap();
    for (steps, want) in table_paths() {
        for format in [PathFormat::JsonPath, PathFormat::Pointer] {
            let p = fmt(&bytes, &steps, format);
            assert_eq!(resolve(&doc, &p, format).as_ref(), Some(&want), "{p:?}");
        }
    }
}

fn tool(name: &str) -> bool {
    std::process::Command::new(name)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn table_paths_resolve_with_jq_when_installed() {
    if !tool("jq") {
        eprintln!("jq not installed: skipped");
        return;
    }
    let doc = table_doc();
    let bytes = serde_json::to_vec(&doc).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("doc.json");
    std::fs::write(&file, &bytes).unwrap();
    for (steps, want) in table_paths() {
        let p = fmt(&bytes, &steps, PathFormat::Jq);
        let out = std::process::Command::new("jq")
            .arg("-c")
            .arg(&p)
            .arg(&file)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "jq rejected {p:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let got: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(got, want, "{p:?}");
    }
}

#[test]
fn table_paths_resolve_with_node_when_installed() {
    if !tool("node") {
        eprintln!("node not installed: skipped");
        return;
    }
    let doc = table_doc();
    let bytes = serde_json::to_vec(&doc).unwrap();
    let cases = table_paths();
    let exprs: Vec<String> = cases
        .iter()
        .map(|(steps, _)| fmt(&bytes, steps, PathFormat::JavaScript))
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("doc.json");
    std::fs::write(&file, &bytes).unwrap();
    let script = format!(
        "\"use strict\";\nconst data = JSON.parse(require('fs').readFileSync({:?}, 'utf8'));\nconsole.log(JSON.stringify([{}]));\n",
        file.display().to_string(),
        exprs.join(",\n")
    );
    let js = dir.path().join("check.js");
    std::fs::write(&js, script).unwrap();
    let out = std::process::Command::new("node")
        .arg(&js)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "node: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got: Vec<Value> = serde_json::from_slice(&out.stdout).unwrap();
    for ((_, want), (got, expr)) in cases.iter().zip(got.iter().zip(&exprs)) {
        assert_eq!(got, want, "{expr}");
    }
}

/// `text` with every non-ASCII character written as `\uXXXX` (inside strings: the only
/// place JSON text has them).
fn escape_non_ascii(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                let _ = write!(out, "\\u{unit:04x}");
            }
        }
    }
    out
}

fn key_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z_$][a-z0-9_$]{0,6}",
        "[ -~]{0,8}",
        any::<String>().prop_map(|s| s.chars().take(8).collect()),
        Just(String::new()),
        prop::sample::select(KEYS).prop_map(str::to_owned),
    ]
}

fn step_strategy() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => key_strategy().prop_map(Step::Key),
        1 => (0usize..4).prop_map(Step::Index),
    ]
}

/// A document holding `leaf` at `steps` (with a sibling at each level).
fn doc_for(steps: &[Step], leaf: &Value) -> Value {
    let mut v = leaf.clone();
    for step in steps.iter().rev() {
        v = match step {
            Step::Key(k) => {
                let mut m = serde_json::Map::new();
                if k != "sibling" {
                    m.insert("sibling".to_owned(), json!(0));
                }
                m.insert(k.clone(), v);
                Value::Object(m)
            }
            Step::Index(i) => {
                let mut a = vec![json!(null); *i];
                a.push(v);
                a.push(json!("after"));
                Value::Array(a)
            }
        };
    }
    v
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn random_paths_resolve_to_the_value(
        steps in prop::collection::vec(step_strategy(), 0..6),
        escape in any::<bool>(),
    ) {
        let leaf = json!({"leaf": [1, 2, 3]});
        let doc = doc_for(&steps, &leaf);
        let mut text = serde_json::to_string(&doc).unwrap();
        if escape {
            text = escape_non_ascii(&text);
        }
        let bytes = text.as_bytes();
        for format in [PathFormat::JsonPath, PathFormat::Pointer] {
            let p = fmt(bytes, &steps, format);
            let got = resolve(&doc, &p, format);
            prop_assert_eq!(got.as_ref(), Some(&leaf), "{:?}", p);
        }
    }
}
