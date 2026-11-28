//! `Document::open` against the golden encoding fixtures and temporary files.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use braceray_core::{Document, Encoding, Error, Sniff};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

fn write_temp(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn open_err(path: &Path) -> Error {
    Document::open(path).expect_err("open should fail")
}

#[test]
fn encoding_manifest_expectations() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture("encoding/encoding_manifest.json")).unwrap())
            .unwrap();
    let entries = manifest.as_array().unwrap();
    assert_eq!(entries.len(), 9);
    for entry in entries {
        let file = entry["file"].as_str().unwrap();
        let path = fixture(&format!("encoding/{file}"));
        let result = Document::open(&path);
        match entry["expect"].as_str().unwrap() {
            "utf8" => {
                let doc = result.unwrap();
                assert_eq!(doc.bytes(), fs::read(&path).unwrap(), "{file}");
            }
            "utf8-bom" => {
                let doc = result.unwrap();
                let raw = fs::read(&path).unwrap();
                assert_eq!(&raw[..3], b"\xEF\xBB\xBF");
                assert_eq!(doc.bytes(), &raw[3..], "{file}");
                assert_eq!(doc.len(), raw.len() as u64 - 3);
                assert_eq!(doc.identity().size, raw.len() as u64);
                serde_json::from_slice::<serde_json::Value>(doc.bytes()).unwrap();
            }
            "utf16le" => assert_encoding(result, Encoding::Utf16Le, file),
            "utf16be" => assert_encoding(result, Encoding::Utf16Be, file),
            "utf32le" => assert_encoding(result, Encoding::Utf32Le, file),
            "binary" => assert_encoding(result, Encoding::Binary, file),
            "empty" => {
                let e = result.unwrap_err();
                assert!(matches!(e, Error::EmptyFile), "{file}: {e:?}");
                assert_eq!(e.to_string(), "This file is empty.");
            }
            "whitespace-only" => {
                let e = result.unwrap_err();
                assert!(matches!(e, Error::WhitespaceOnly), "{file}: {e:?}");
                assert!(e.to_string().contains("only whitespace"));
            }
            other => panic!("unknown expectation {other} for {file}"),
        }
    }
}

fn assert_encoding(result: Result<std::sync::Arc<Document>, Error>, want: Encoding, file: &str) {
    match result {
        Err(e @ Error::Encoding(got)) => {
            assert_eq!(got, want, "{file}");
            let msg = e.to_string();
            if want == Encoding::Binary {
                assert!(msg.contains("binary data"), "{file}: {msg}");
            } else {
                assert!(
                    msg.contains("Braceray supports UTF-8 JSON"),
                    "{file}: {msg}"
                );
                assert!(msg.contains("iconv") && msg.contains("Get-Content"));
            }
        }
        other => panic!("{file}: expected {want:?}, got {other:?}"),
    }
}

#[test]
fn every_valid_and_generated_fixture_opens() {
    for dir in ["valid", "generated", "invalid"] {
        for entry in fs::read_dir(fixture(dir)).unwrap() {
            let path = entry.unwrap().path();
            let doc = Document::open(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
            assert_eq!(doc.len(), fs::metadata(&path).unwrap().len());
            assert_eq!(doc.path(), path);
        }
    }
}

#[test]
fn bom_only_file_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_temp(&dir, "bom.json", b"\xEF\xBB\xBF");
    assert!(matches!(open_err(&path), Error::EmptyFile));
    let path = write_temp(&dir, "bom_ws.json", b"\xEF\xBB\xBF \n ");
    assert!(matches!(open_err(&path), Error::WhitespaceOnly));
}

#[test]
fn not_json_gives_early_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_temp(&dir, "page.html", b"\n  <!doctype html>");
    let e = open_err(&path);
    assert!(matches!(
        e,
        Error::NotJson {
            offset: 3,
            found: b'<'
        }
    ));
    assert!(e.to_string().contains("doesn't look like JSON"));
    // Scalars are valid top-level JSON values.
    for (i, ok) in ["42", "-1", "\"s\"", "true", "false", "null"]
        .iter()
        .enumerate()
    {
        let path = write_temp(&dir, &format!("scalar{i}.json"), ok.as_bytes());
        Document::open(&path).unwrap();
    }
}

#[test]
fn missing_file_and_directory() {
    let dir = tempfile::tempdir().unwrap();
    let e = open_err(&dir.path().join("nope.json"));
    assert!(matches!(e, Error::Io { .. }));
    assert!(e.to_string().starts_with("Can't open "));
    let e = open_err(dir.path());
    // Unix opens directories as files and reports NotAFile; Windows fails in File::open.
    assert!(
        matches!(e, Error::NotAFile { .. } | Error::Io { .. }),
        "{e:?}"
    );
}

#[test]
fn identity_and_sniff() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_temp(&dir, "a.json", b"{\"a\":1}\n{\"a\":2}\n");
    let b = write_temp(&dir, "b.json", b"{\"a\":1}\n{\"a\":3}\n");
    let da = Document::open(&a).unwrap();
    let db = Document::open(&b).unwrap();
    assert_eq!(da.sniff(), Sniff::NdJson);
    assert_eq!(da.identity().size, db.identity().size);
    assert_ne!(da.identity().content_hash, db.identity().content_hash);
    assert!(da.identity().canonical_path.is_absolute());
    assert!(da.identity().mtime.is_some());
    // Same file opened twice → same identity.
    assert_eq!(da.identity(), Document::open(&a).unwrap().identity());

    let pretty = Document::open(&fixture("generated/orders_16k.json")).unwrap();
    assert_eq!(pretty.sniff(), Sniff::Json);
    let ndjson = Document::open(&fixture("generated/logs_16k.ndjson")).unwrap();
    assert_eq!(ndjson.sniff(), Sniff::NdJson);
    pretty.advise_sequential();
    pretty.advise_random();
}

#[test]
fn change_detection_on_append() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_temp(&dir, "grow.json", b"[1, 2, 3]");
    let doc = Document::open(&path).unwrap();
    assert!(!doc.has_changed_on_disk());
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b" ")
        .unwrap();
    assert!(doc.has_changed_on_disk());
    // The mapping still shows the bytes from open time.
    assert_eq!(doc.bytes(), b"[1, 2, 3]");
}

#[test]
fn change_detection_on_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_temp(&dir, "touch.json", b"{}");
    let doc = Document::open(&path).unwrap();
    assert!(!doc.has_changed_on_disk());
    let f = OpenOptions::new().write(true).open(&path).unwrap();
    let mtime = doc.identity().mtime.unwrap() + Duration::from_secs(10);
    f.set_modified(mtime).unwrap();
    drop(f);
    assert!(doc.has_changed_on_disk());
}

#[test]
fn change_detection_on_delete() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_temp(&dir, "gone.json", b"{}");
    let doc = Document::open(&path).unwrap();
    // Windows refuses to delete a mapped file (documented in architecture.md §9).
    if fs::remove_file(&path).is_ok() {
        assert!(doc.has_changed_on_disk());
    }
}

/// A 5 GiB sparse file: `[]`, 8 KiB of spaces, then a hole (zeros) up to 5 GiB + 3 bytes.
/// Opening it touches only the first few KiB and 1 MiB at each end, so it's instant.
#[cfg(target_pointer_width = "64")]
#[test]
fn five_gib_sparse_file() {
    const LEN: u64 = 5 * 1024 * 1024 * 1024 + 3;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sparse_5g.json");
    let mut f = File::create(&path).unwrap();
    f.write_all(b"[]").unwrap();
    f.write_all(&[b' '; 8192]).unwrap();
    f.set_len(LEN).unwrap();
    drop(f);

    let start = std::time::Instant::now();
    let doc = Document::open(&path).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(doc.len(), LEN);
    assert_eq!(doc.identity().size, LEN);
    assert_eq!(&doc.bytes()[..3], b"[] ");
    assert_eq!(doc.bytes()[LEN as usize - 1], 0);
    assert!(!doc.has_changed_on_disk());
    eprintln!("5 GiB sparse open: {elapsed:?}");
}

#[test]
fn mtime_is_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_temp(&dir, "t.json", b"[]");
    let doc = Document::open(&path).unwrap();
    let mtime = doc.identity().mtime.unwrap();
    let age = SystemTime::now()
        .duration_since(mtime)
        .unwrap_or(Duration::ZERO);
    assert!(age < Duration::from_secs(60));
}

/// A file truncated while it's mapped: indexing the old length reads zeros past the new
/// end instead of crashing with SIGBUS, and the document reports the change (ADR 028).
#[cfg(unix)]
#[test]
fn truncated_while_mapped_reads_zeros_and_reports_change() {
    use braceray_core::index::{IndexOptions, build_index};
    use braceray_core::{CancelToken, Progress};
    let dir = tempfile::tempdir().unwrap();
    let mut body = b"[".to_vec();
    while body.len() < 4 << 20 {
        body.extend_from_slice(br#"{"a":1,"b":"text"},"#);
    }
    body.extend_from_slice(b"0]");
    let path = write_temp(&dir, "t.json", &body);
    let doc = Document::open(&path).unwrap();
    assert!(!doc.has_changed_on_disk());
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(4096)
        .unwrap();
    let index = build_index(
        &doc,
        &IndexOptions::default(),
        &CancelToken::new(),
        &Progress::default(),
    )
    .unwrap();
    assert!(!index.is_valid(), "zeros past the cut are not JSON");
    assert_eq!(doc.len(), body.len() as u64, "the mapping keeps its length");
    assert!(doc.has_changed_on_disk());
}
