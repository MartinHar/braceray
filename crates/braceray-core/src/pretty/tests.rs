//! Unit tests of the row generator: indentation, empty containers, escapes, UTF-8 at row
//! edges, scalars at the top level, invalid input.

use super::*;

fn text(bytes: &[u8], wrap: usize, indent: u8) -> Vec<String> {
    let opts = PrettyOptions { wrap, indent };
    let mut out = Vec::new();
    for line in Generator::new(bytes, PrettyPos::START, &opts) {
        let mut t = Vec::new();
        line.write_text(bytes, indent, &mut t);
        let prefix = if line.is_continuation() { "↩" } else { "" };
        out.push(format!("{prefix}{}", String::from_utf8(t).unwrap()));
    }
    out
}

#[test]
fn indentation_follows_the_option() {
    let bytes = br#"{"a":{"b":[1]}}"#;
    assert_eq!(
        text(bytes, 400, 4),
        [
            "{",
            "    \"a\": {",
            "        \"b\": [",
            "            1",
            "        ]",
            "    }",
            "}"
        ]
    );
    assert_eq!(text(bytes, 400, 0)[3], "1");
}

#[test]
fn empty_containers_stay_on_one_row() {
    let bytes = b"[ {} , [\n\n] , { \t } ,[[]]]";
    assert_eq!(
        text(bytes, 400, 2),
        ["[", "  {},", "  [],", "  {},", "  [", "    []", "  ]", "]"]
    );
}

#[test]
fn escapes_and_structural_bytes_inside_strings() {
    let bytes = br#"{"k\"ey:": "v,a]l{ue\\", "\u0041": "\"}"}"#;
    let rows = pretty_lines(bytes, PrettyPos::START, 10, &PrettyOptions::default());
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[1].tokens[0].kind, TokenKind::Key);
    assert_eq!(rows[1].tokens[0].span.slice(bytes), br#""k\"ey:""#);
    assert_eq!(rows[1].tokens[2].span.slice(bytes), br#""v,a]l{ue\\""#);
    assert_eq!(rows[2].tokens[2].span.slice(bytes), br#""\"}""#);
}

#[test]
fn long_tokens_split_on_character_boundaries() {
    // 4-byte characters around every possible row edge.
    for wrap in 16..24 {
        let mut s = String::from("[\"");
        for i in 0..40 {
            s.push(if i % 3 == 0 { '😀' } else { 'x' });
        }
        s.push_str("\", 12345678901234567890123456789012345]");
        let bytes = s.as_bytes();
        let opts = PrettyOptions { wrap, indent: 2 };
        let rows: Vec<_> = Generator::new(bytes, PrettyPos::START, &opts).collect();
        let mut joined = Vec::new();
        for r in &rows {
            let span = r.source_span().slice(bytes);
            assert!(std::str::from_utf8(span).is_ok(), "wrap {wrap}: {r:?}");
            assert!(
                span.len() <= wrap + 8,
                "wrap {wrap}: row of {} bytes",
                span.len()
            );
            for t in &r.tokens {
                joined.extend_from_slice(t.span.slice(bytes));
            }
        }
        let squashed: Vec<u8> = bytes.iter().copied().filter(|&b| b != b' ').collect();
        assert_eq!(joined, squashed, "wrap {wrap}");
        // The long number splits too, and its parts stay numbers.
        assert!(rows.iter().filter(|r| r.is_continuation()).count() >= 3);
        assert!(
            rows.iter()
                .filter(|r| r.is_continuation())
                .all(|r| matches!(r.tokens[0].kind, TokenKind::String | TokenKind::Number))
        );
    }
}

#[test]
fn top_level_scalars_and_whitespace() {
    assert_eq!(text(b"  42  \n", 400, 2), ["42"]);
    assert_eq!(text(br#""only""#, 400, 2), ["\"only\""]);
    assert_eq!(text(b"   ", 400, 2), Vec::<String>::new());
    assert_eq!(text(b"", 400, 2), Vec::<String>::new());
    let rows = pretty_lines(b" true ", PrettyPos::START, 5, &PrettyOptions::default());
    assert_eq!(rows[0].tokens[0].kind, TokenKind::Literal);
    // Leading and trailing whitespace belong to the first and last rows: rows tile the
    // whole document.
    assert_eq!(rows[0].source_span(), crate::nav::Span { start: 0, end: 6 });
}

#[test]
fn invalid_input_gets_error_tokens_and_bounded_rows() {
    let rows = pretty_lines(
        b"[nul, tru, 1 2 3, @@]",
        PrettyPos::START,
        50,
        &PrettyOptions::default(),
    );
    let kinds: Vec<Vec<TokenKind>> = rows
        .iter()
        .map(|r| r.tokens.iter().map(|t| t.kind).collect())
        .collect();
    use TokenKind as K;
    assert_eq!(kinds[1], [K::Error, K::Punct]);
    assert_eq!(kinds[2], [K::Error, K::Punct]);
    // Values without a separator get their own rows.
    assert_eq!(kinds[3], [K::Number]);
    assert_eq!(kinds[4], [K::Number]);
    assert_eq!(kinds[5], [K::Number, K::Punct]);
    assert_eq!(kinds[6], [K::Error]);
    // Unbalanced closers never underflow the depth.
    let rows = pretty_lines(b"]]]}", PrettyPos::START, 10, &PrettyOptions::default());
    assert!(rows.iter().all(|r| r.depth == 0));
    // An unterminated string ends with the input.
    let rows = pretty_lines(b"[\"abc", PrettyPos::START, 10, &PrettyOptions::default());
    assert_eq!(rows.last().unwrap().next.offset, 5);
}
