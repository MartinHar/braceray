//! Invalid fixtures fail exactly at the manifest position (checked against
//! serde_json), and encoding fixtures look like what their manifest says.

mod common;

use common::*;
use gen_fixtures::manifest::{EncodingCase, InvalidCase};
use gen_fixtures::orders::{RecordsOpts, generate_orders};
use gen_fixtures::{encoding, invalid, logs, shapes};
use serde_json::error::Category;

/// Maps serde's (line, column) back to a byte offset.
///
/// serde_json reports the position of the last byte it consumed, with a
/// 1-based byte column. For a bad byte that is the offending byte itself. For
/// an unexpected end of input it is the last byte of the file, so the error
/// "position" (`len`) is one past it. One quirk: for a raw control character
/// inside a string serde reports the byte *before* it (it peeks the byte
/// without consuming it), so we add one back.
fn serde_error_offset(bytes: &[u8], err: &serde_json::Error) -> u64 {
    let line_start: usize = if err.line() <= 1 {
        0
    } else {
        bytes
            .iter()
            .enumerate()
            .filter(|(_, b)| **b == b'\n')
            .nth(err.line() - 2)
            .map(|(i, _)| i + 1)
            .unwrap()
    };
    let last = (line_start + err.column()).saturating_sub(1) as u64;
    if err.classify() == Category::Eof || is_control_char(err) {
        last + 1
    } else {
        last
    }
}

fn is_control_char(err: &serde_json::Error) -> bool {
    err.to_string().starts_with("control character")
}

fn check_case(bytes: &[u8], case: &InvalidCase) {
    let err = serde_json::from_slice::<serde::de::IgnoredAny>(bytes)
        .expect_err(&format!("{} must be invalid", case.file));
    assert_eq!(
        serde_error_offset(bytes, &err),
        case.error_offset,
        "{}: serde says '{err}'",
        case.file
    );
    if err.classify() != Category::Eof && !is_control_char(&err) {
        assert_eq!(
            (err.line() as u64, err.column() as u64),
            (case.line, case.column),
            "{}: line/column",
            case.file
        );
    }
    assert_eq!(
        invalid::line_col(bytes, case.error_offset),
        (case.line, case.column),
        "{}: manifest line/column",
        case.file
    );
}

#[test]
fn every_invalid_file_fails_at_the_manifest_offset() {
    let t = TempDir::new("invalid");
    let manifest = invalid::generate_invalid(&t.0, 1 << 20, 42).unwrap();
    assert_eq!(manifest.len(), invalid::small_cases().len() + 1);
    let on_disk: Vec<InvalidCase> =
        serde_json::from_slice(&read(&t.path("invalid_manifest.json"))).unwrap();
    assert_eq!(on_disk, manifest);
    for case in &manifest {
        check_case(&read(&t.path(&case.file)), case);
    }
    let big = manifest.last().unwrap();
    assert!(
        big.line > 10_000,
        "big file error should be deep in the file"
    );
}

#[test]
fn encoding_fixtures_match_their_manifest() {
    let t = TempDir::new("enc");
    let manifest: Vec<EncodingCase> = encoding::generate_encoding(&t.0, 42).unwrap();
    for case in &manifest {
        let bytes = read(&t.path(&case.file));
        let parses = serde_json::from_slice::<serde_json::Value>(&bytes).is_ok();
        match case.expect.as_str() {
            "utf8" => assert!(parses, "{}", case.file),
            "utf8-bom" => {
                assert_eq!(&bytes[..3], b"\xEF\xBB\xBF");
                assert!(!parses, "serde rejects the BOM itself");
                parse_ok(bytes[3..].to_vec()).unwrap();
            }
            "utf16le" | "utf16be" => {
                assert!(!parses);
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&pair| {
                        if case.expect == "utf16le" {
                            u16::from_le_bytes(pair)
                        } else {
                            u16::from_be_bytes(pair)
                        }
                    })
                    .collect();
                let text = String::from_utf16(&units).unwrap();
                let text = text.trim_start_matches('\u{feff}');
                serde_json::from_str::<serde_json::Value>(text).unwrap();
            }
            "utf32le" => {
                assert_eq!(&bytes[..4], b"\xFF\xFE\0\0");
                assert!(!parses);
            }
            "empty" => assert!(bytes.is_empty()),
            "whitespace-only" => {
                assert!(!bytes.is_empty());
                assert!(bytes.iter().all(|b| b" \t\r\n".contains(b)));
            }
            "binary" => {
                assert!(bytes.contains(&0));
                assert!(std::str::from_utf8(&bytes).is_err());
            }
            other => panic!("unknown expectation {other}"),
        }
    }
}

fn golden_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/braceray-core/tests/fixtures")
}

/// The committed golden copies of generated fixtures must stay identical to
/// what the generator produces (regenerate with tools/fixtures/README.md).
#[test]
fn committed_golden_fixtures_are_up_to_date() {
    let golden = golden_dir();
    let t = TempDir::new("golden");
    invalid::generate_invalid(&t.path("invalid"), 0, 42).unwrap();
    encoding::generate_encoding(&t.path("encoding"), 42).unwrap();
    let small = |minified| RecordsOpts {
        size: 16 << 10,
        seed: 42,
        minified,
    };
    let g = t.path("generated");
    generate_orders(&g.join("orders_16k.json"), small(false)).unwrap();
    generate_orders(&g.join("orders_min_16k.json"), small(true)).unwrap();
    logs::generate_ndjson(&g.join("logs_16k.ndjson"), small(true)).unwrap();
    shapes::generate_deep(&g.join("deep_1000.json"), 1000).unwrap();
    for sub in ["invalid", "encoding", "generated"] {
        for entry in std::fs::read_dir(t.path(sub)).unwrap() {
            let entry = entry.unwrap();
            let committed = golden.join(sub).join(entry.file_name());
            assert!(
                read(&entry.path()) == read(&committed),
                "{} is stale",
                committed.display()
            );
        }
    }
}

#[test]
fn golden_valid_fixtures_parse_and_stay_small() {
    let golden = golden_dir();
    let mut checked = 0;
    for sub in ["valid", "generated", "invalid", "encoding"] {
        for entry in std::fs::read_dir(golden.join(sub)).unwrap() {
            let path = entry.unwrap().path();
            let bytes = read(&path);
            assert!(bytes.len() < 50 << 10, "{} is >= 50 KB", path.display());
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if sub == "valid" || (sub == "generated" && name.ends_with(".json")) {
                parse_ok(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
                checked += 1;
            }
        }
    }
    assert!(checked >= 15, "only {checked} valid golden files found");
}
