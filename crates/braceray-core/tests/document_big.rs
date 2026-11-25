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
