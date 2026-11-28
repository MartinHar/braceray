use proptest::prelude::*;
use serde::Serialize;
use serde_json::{Value, json};

use super::*;

fn all(bytes: &[u8]) -> Span {
    Span {
        start: 0,
        end: bytes.len() as u64,
    }
}

fn run(bytes: &[u8], style: CopyStyle) -> String {
    render(
        bytes,
        all(bytes),
        style,
        &CancelToken::new(),
        &Progress::default(),
    )
    .unwrap()
}

fn serde_pretty(v: &Value, indent: &[u8]) -> String {
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(indent);
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    v.serialize(&mut ser).unwrap();
    String::from_utf8(out).unwrap()
}

fn value_strategy() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        any::<String>().prop_map(Value::String),
        "[ -~\\\\\"\\t\\n]{0,10}".prop_map(Value::String),
    ];
    leaf.prop_recursive(6, 64, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::vec(("[ -~]{0,6}", inner), 0..6)
                .prop_map(|kv| Value::Object(kv.into_iter().collect())),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn matches_serde_exactly(v in value_strategy()) {
        let compact = serde_json::to_string(&v).unwrap();
        let pretty = serde_json::to_string_pretty(&v).unwrap();
        prop_assert_eq!(run(compact.as_bytes(), CopyStyle::Pretty { indent: 2 }), pretty.clone());
        prop_assert_eq!(run(compact.as_bytes(), CopyStyle::Minified), compact.clone());
        prop_assert_eq!(run(pretty.as_bytes(), CopyStyle::Minified), compact.clone());
        prop_assert_eq!(run(pretty.as_bytes(), CopyStyle::Pretty { indent: 2 }), pretty.clone());
        prop_assert_eq!(run(compact.as_bytes(), CopyStyle::Pretty { indent: 4 }), serde_pretty(&v, b"    "));
        prop_assert_eq!(run(pretty.as_bytes(), CopyStyle::Raw), pretty);
        if let Value::String(s) = &v {
            prop_assert_eq!(&run(compact.as_bytes(), CopyStyle::StringContents), s);
        }
    }
}

#[test]
fn odd_whitespace_and_numbers_parse_the_same() {
    let src =
        "\r\n\t{ \"a\" :\t[ 1.50e+3 , -0 , 1E40 ,\r\n\"x\\\"y\" ] ,\"b\":{ } , \"c\" : [ ] }\n";
    let want: Value = serde_json::from_str(src).unwrap();
    for style in [CopyStyle::Pretty { indent: 2 }, CopyStyle::Minified] {
        let out = run(src.as_bytes(), style);
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), want, "{out}");
    }
    // Numbers are kept as written.
    let min = run(src.as_bytes(), CopyStyle::Minified);
    assert_eq!(min, r#"{"a":[1.50e+3,-0,1E40,"x\"y"],"b":{},"c":[]}"#);
}

#[test]
fn string_contents_decode_and_other_values_stay_raw() {
    assert_eq!(
        run(
            br#" "a\nb\u00e9\ud83d\ude00\/" "#,
            CopyStyle::StringContents
        ),
        "a\nbé😀/"
    );
    assert_eq!(run(b"[1, 2]", CopyStyle::StringContents), "[1, 2]");
    assert_eq!(run(b"\"\"", CopyStyle::StringContents), "");
}

#[test]
fn broken_input_doesnt_panic() {
    for src in [
        &b"{\"a\": \"unterminated"[..],
        b"]]]}}",
        b"\"\\",
        b"{\"a\":",
        b"[\xff\xfe]",
        b"",
    ] {
        for style in [
            CopyStyle::Raw,
            CopyStyle::Pretty { indent: 2 },
            CopyStyle::Minified,
            CopyStyle::StringContents,
        ] {
            let _ = run(src, style);
        }
    }
    assert_eq!(run(b"[\xff]", CopyStyle::Minified), "[\u{FFFD}]");
}

fn big_array(bytes: usize) -> Vec<u8> {
    let mut out = b"[".to_vec();
    while out.len() < bytes {
        out.extend_from_slice(br#"{"k": "value \"x\"", "n": [1, 2, 3]}, "#);
    }
    out.extend_from_slice(b"null]");
    out
}

#[test]
fn cancel_stops_and_progress_counts_input() {
    let src = big_array(3 * CHECK_EVERY);
    let cancel = CancelToken::new();
    cancel.cancel();
    for style in [
        CopyStyle::Raw,
        CopyStyle::Pretty { indent: 2 },
        CopyStyle::Minified,
    ] {
        let r = render(&src, all(&src), style, &cancel, &Progress::default());
        assert!(matches!(r, Err(Error::Cancelled)), "{style:?}");
    }
    let mut s = b"\"".to_vec();
    s.extend(std::iter::repeat_n(b'a', 2 * CHECK_EVERY));
    s.push(b'"');
    let r = render(
        &s,
        all(&s),
        CopyStyle::StringContents,
        &cancel,
        &Progress::default(),
    );
    assert!(matches!(r, Err(Error::Cancelled)));

    for style in [CopyStyle::Minified, CopyStyle::StringContents] {
        let input = if style == CopyStyle::Minified {
            &src
        } else {
            &s
        };
        let progress = Progress::default();
        render(input, all(input), style, &CancelToken::new(), &progress).unwrap();
        assert_eq!(progress.done(), input.len() as u64);
        assert_eq!(progress.total(), input.len() as u64);
    }
}

#[test]
fn export_in_pieces_equals_render() {
    let src = big_array(2 * CHECK_EVERY + 123);
    for style in [
        CopyStyle::Raw,
        CopyStyle::Pretty { indent: 2 },
        CopyStyle::Minified,
    ] {
        let mut out = Vec::new();
        let (c, p) = (CancelToken::new(), Progress::default());
        write_to(&src, all(&src), style, &mut out, "x".as_ref(), &c, &p).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            run(&src, style),
            "{style:?}"
        );
    }
    // A failing writer names the file.
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (c, p) = (CancelToken::new(), Progress::default());
    let err = write_to(
        &src,
        all(&src),
        CopyStyle::Minified,
        &mut Broken,
        "out.json".as_ref(),
        &c,
        &p,
    )
    .unwrap_err();
    assert!(err.to_string().contains("out.json"), "{err}");
}

#[test]
fn sub_span_only() {
    let bytes = br#"{"a": {"b": [1,2]}, "c": 3}"#;
    let span = Span { start: 6, end: 18 };
    assert_eq!(
        render(
            bytes,
            span,
            CopyStyle::Minified,
            &CancelToken::new(),
            &Progress::default()
        )
        .unwrap(),
        r#"{"b":[1,2]}"#
    );
}
