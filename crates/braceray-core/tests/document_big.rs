//! Big-file checks for `Document::open` (step 03 acceptance criteria). They need the
//! 2 GB fixture, so they're ignored by default:
//!
//! ```sh
//! gen-fixtures orders --size 2g --out fixtures-data/orders_2g.json
//! cargo test -p braceray-core --release --test document_big -- --ignored --nocapture
//! ```
//! `BRACERAY_FIXTURES` overrides the fixture directory (default: `<repo>/fixtures-data`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use braceray_core::Document;

fn fixture_2g() -> Option<PathBuf> {
    let dir = std::env::var_os("BRACERAY_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-data"));
    let path = dir.join("orders_2g.json");
    if path.exists() {
        Some(path)
    } else {
        eprintln!("skipped: {} not found (run gen-fixtures)", path.display());
        None
    }
}

/// One test, so that no other test runs in parallel and disturbs the RSS measurement.
#[test]
#[ignore = "needs fixtures-data/orders_2g.json"]
fn open_2g_is_fast_and_light() {
    let Some(path) = fixture_2g() else { return };
    let rss = || memory_stats::memory_stats().map(|m| m.physical_mem as i64);

    let before = rss();
    let start = Instant::now();
    let doc = Document::open(&path).unwrap();
    let first = start.elapsed();
    let after = rss();
    assert!(doc.len() >= 2_000_000_000, "len {}", doc.len());

    // Memory: only the touched pages (≤ 4 KiB sample + 2 × 1 MiB hash window) count.
    if let (Some(before), Some(after)) = (before, after) {
        let grew = after - before;
        eprintln!("RSS before {before} B, after {after} B, grew {grew} B");
        assert!(grew < 5 * 1024 * 1024, "RSS grew by {grew} bytes");
    } else {
        eprintln!("RSS not available on this platform");
    }
    drop(doc);

    // Time: first open (possibly cold) and the best of 10 warm opens.
    let mut best = Duration::MAX;
    for _ in 0..10 {
        let start = Instant::now();
        let doc = Document::open(&path).unwrap();
        best = best.min(start.elapsed());
        drop(doc);
    }
    eprintln!("open 2 GB: first {first:?}, best warm {best:?}");
    assert!(
        first < Duration::from_millis(50),
        "first open took {first:?}"
    );
    assert!(best < Duration::from_millis(50), "warm open took {best:?}");
}

/// Baseline for the SIMD / parallel builder: scalar reference throughput and index size.
#[test]
#[ignore = "needs fixtures-data/orders_2g.json"]
fn scalar_index_2g() {
    let Some(path) = fixture_2g() else { return };
    let doc = Document::open(&path).unwrap();
    let start = Instant::now();
    let index = braceray_core::index::scalar::build(
        doc.bytes(),
        None,
        &braceray_core::CancelToken::new(),
        &braceray_core::Progress::default(),
    )
    .unwrap();
    let secs = start.elapsed().as_secs_f64();
    assert!(index.is_valid());
    let ratio = index.memory_bytes() as f64 / doc.len() as f64;
    eprintln!(
        "scalar index 2 GB: {secs:.2} s ({:.2} GB/s), {} nodes, {} index bytes ({:.1} % of file)",
        doc.len() as f64 / secs / 1e9,
        index.nodes().len(),
        index.memory_bytes(),
        ratio * 100.0
    );
}

/// Single-thread throughput of the block scanner alone (classify + escape + string masks).
#[test]
#[ignore = "needs fixtures-data/orders_2g.json"]
fn simd_scan_2g() {
    use braceray_core::index::simd;
    let Some(path) = fixture_2g() else { return };
    let doc = Document::open(&path).unwrap();
    for imp in simd::available() {
        let start = Instant::now();
        let mut state = simd::ScanState::default();
        let mut structurals = 0u64;
        simd::scan(&imp, doc.bytes(), &mut state, |_, b| {
            structurals += u64::from(b.structurals().count_ones());
        });
        let secs = start.elapsed().as_secs_f64();
        eprintln!(
            "{:>14}: {secs:.2} s ({:.2} GB/s), {structurals} structurals",
            imp.name,
            doc.len() as f64 / secs / 1e9
        );
    }
}

/// Step 04 targets: full index of 2 GB warm < 5 s, memory ≤ 25 % of the file.
#[test]
#[ignore = "needs fixtures-data/orders_2g.json"]
fn fast_index_2g() {
    use braceray_core::index::{IndexOptions, build_index};
    let Some(path) = fixture_2g() else { return };
    // `BRACERAY_BIG_FILES=orders_2g.json` measures one file only (two 2 GB files may not
    // both stay in the page cache, and then the runs aren't warm).
    let names = std::env::var("BRACERAY_BIG_FILES")
        .unwrap_or_else(|_| "orders_2g.json,orders_min_2g.json".to_owned());
    for name in names.split(',') {
        let path = path.with_file_name(name);
        if !path.exists() {
            continue;
        }
        let doc = Document::open(&path).unwrap();
        for threads in [None, Some(1)] {
            let opts = IndexOptions {
                threads,
                ..IndexOptions::default()
            };
            let start = Instant::now();
            let index = build_index(
                &doc,
                &opts,
                &braceray_core::CancelToken::new(),
                &braceray_core::Progress::default(),
            )
            .unwrap();
            let secs = start.elapsed().as_secs_f64();
            assert!(index.is_valid());
            let ratio = index.memory_bytes() as f64 / doc.len() as f64;
            eprintln!(
                "{name} threads {threads:?}: {secs:.2} s ({:.2} GB/s), {} nodes, depth limit {:?}, index {:.1} % of file",
                doc.len() as f64 / secs / 1e9,
                index.nodes().len(),
                index.max_stored_depth(),
                ratio * 100.0
            );
            assert!(
                ratio <= 0.25,
                "{name}: index is {:.1} % of the file",
                ratio * 100.0
            );
            if threads.is_none() {
                assert!(secs < 5.0, "{name}: {secs:.2} s");
            }
        }
    }
}
