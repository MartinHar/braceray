//! `invalid`: malformed JSON with the exact expected error position.

use crate::manifest::{InvalidCase, write_json};
use crate::orders::{Mutation, order_record};
use crate::records::{Frame, Out, Stop, write_records};
use crate::size::size_label;
use std::io;
use std::path::Path;

/// Small hand-written cases: (file, bytes, error offset, description).
/// An error offset equal to the length means "unexpected end of input".
pub fn small_cases() -> Vec<(&'static str, Vec<u8>, u64, &'static str)> {
    let multiline = b"{\n  \"a\": 1,\n  \"b\": [\n    true\n    false\n  ]\n}\n".to_vec();
    let multi_off = find(&multiline, b"false");
    vec![
        (
            "invalid_truncated.json",
            br#"{"a": [1, 2, 3"#.to_vec(),
            14,
            "truncated: input ends inside an array",
        ),
        (
            "invalid_trailing_comma_array.json",
            b"[1, 2, 3,]".to_vec(),
            9,
            "trailing comma before ']'",
        ),
        (
            "invalid_trailing_comma_object.json",
            br#"{"a": 1,}"#.to_vec(),
            8,
            "trailing comma before '}'",
        ),
        (
            "invalid_missing_colon.json",
            br#"{"a" 1}"#.to_vec(),
            5,
            "missing ':' after key",
        ),
        (
            "invalid_missing_comma.json",
            b"[1 2]".to_vec(),
            3,
            "missing ',' between array elements",
        ),
        (
            "invalid_bad_literal.json",
            b"[truX]".to_vec(),
            4,
            "bad literal 'truX'",
        ),
        (
            "invalid_control_char.json",
            b"{\"s\": \"a\x01b\"}".to_vec(),
            8,
            "raw control character U+0001 inside a string",
        ),
        (
            "invalid_unclosed_string.json",
            br#"{"s": "abc"#.to_vec(),
            10,
            "unclosed string at end of input",
        ),
        (
            "invalid_mismatched_bracket.json",
            b"[1, 2}".to_vec(),
            5,
            "'[' closed by '}'",
        ),
        (
            "invalid_trailing_garbage.json",
            b"{} x".to_vec(),
            3,
            "extra data after the top-level value",
        ),
        (
            "invalid_multiline.json",
            multiline,
            multi_off,
            "missing ',' on line 5 of a pretty-printed file",
        ),
        (
            "invalid_after_multibyte.json",
            "{\"name\": \"Արամ\" \"x\": 1}".as_bytes().to_vec(),
            "{\"name\": \"Արամ\" ".len() as u64,
            "missing ',' after a value with multi-byte UTF-8 (column counts bytes)",
        ),
    ]
}

fn find(hay: &[u8], needle: &[u8]) -> u64 {
    hay.windows(needle.len())
        .position(|w| w == needle)
        .unwrap_or(0) as u64
}

/// 1-based (line, byte column) of `offset` in `bytes`.
pub fn line_col(bytes: &[u8], offset: u64) -> (u64, u64) {
    let before = &bytes[..offset as usize];
    let line = 1 + before.iter().filter(|&&b| b == b'\n').count() as u64;
    let line_start = before
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    (line, offset - line_start as u64 + 1)
}

/// Writes all invalid fixtures and `invalid_manifest.json` into `dir`.
/// `big_size` > 0 adds `invalid_end_<size>.json`: a valid-looking pretty
/// `orders` array of that size whose only error (a trailing comma) is at the very end.
pub fn generate_invalid(dir: &Path, big_size: u64, seed: u64) -> io::Result<Vec<InvalidCase>> {
    std::fs::create_dir_all(dir)?;
    let mut manifest = Vec::new();
    for (file, bytes, offset, description) in small_cases() {
        std::fs::write(dir.join(file), &bytes)?;
        let (line, column) = line_col(&bytes, offset);
        manifest.push(InvalidCase {
            file: file.to_owned(),
            error_offset: offset,
            line,
            column,
            description: description.to_owned(),
        });
    }
    if big_size > 0 {
        manifest.push(generate_invalid_end(dir, big_size, seed)?);
    }
    write_json(&dir.join("invalid_manifest.json"), &manifest)?;
    Ok(manifest)
}

fn generate_invalid_end(dir: &Path, size: u64, seed: u64) -> io::Result<InvalidCase> {
    let file = format!("invalid_end_{}.json", size_label(size));
    let path = dir.join(&file);
    let mut out = Out::create(&path, size)?;
    let frame = Frame {
        open: b"[\n  ",
        sep: b",\n  ",
        close: b",\n]\n",
    };
    let stop = Stop::Size {
        bytes: size,
        min_records: 1,
    };
    let mut newlines_in_records = 0u64;
    write_records(&mut out, &frame, stop, |i, b| {
        order_record(seed, i, true, false, Mutation::None, b);
    })?;
    let len = out.finish()?;
    // Count lines by streaming the file back (records are generated in
    // parallel, so counting while writing would need extra plumbing).
    let mut f = std::io::BufReader::with_capacity(8 << 20, std::fs::File::open(&path)?);
    loop {
        let buf = std::io::BufRead::fill_buf(&mut f)?;
        if buf.is_empty() {
            break;
        }
        newlines_in_records += buf.iter().filter(|&&b| b == b'\n').count() as u64;
        let n = buf.len();
        std::io::BufRead::consume(&mut f, n);
    }
    // The file ends with ",\n]\n": the error is the ']' (trailing comma).
    let offset = len - 2;
    Ok(InvalidCase {
        file,
        error_offset: offset,
        // Total newlines minus the final one after ']' gives the line index of ']'.
        line: newlines_in_records,
        column: 1,
        description: "trailing comma before the final ']' at the very end of a big file".to_owned(),
    })
}
