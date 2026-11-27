//! Big-file search checks (step 09 acceptance criteria). They need the 2 GB fixtures:
//!
//! ```sh
//! gen-fixtures orders --size 2g --out fixtures-data/orders_2g.json
//! cargo test -p braceray-core --release --test search_big -- --ignored --nocapture
//! ```
//! `BRACERAY_FIXTURES` overrides the fixture directory (default: `<repo>/fixtures-data`).
//! `BRACERAY_BIG_FILES` picks the files (default `orders_2g.json,orders_min_2g.json`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use braceray_core::Document;
use braceray_core::job::no_repaint;
use braceray_core::search::{SearchOptions, SearchQuery, start_search};

fn fixtures() -> Vec<PathBuf> {
    let dir = std::env::var_os("BRACERAY_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-data"));
    let names = std::env::var("BRACERAY_BIG_FILES")
        .unwrap_or_else(|_| "orders_2g.json,orders_min_2g.json".to_owned());
    names
        .split(',')
        .map(|n| dir.join(n))
        .filter(|p| {
            let ok = p.exists();
            if !ok {
                eprintln!("skipped: {} not found (run gen-fixtures)", p.display());
            }
            ok
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct Needle {
    needle: String,
    count: u64,
    count_case_insensitive: u64,
}

#[derive(serde::Deserialize)]
struct Manifest {
    needles: Vec<Needle>,
}

/// Best of `runs` full searches (warm after the first).
fn timed(doc: &std::sync::Arc<Document>, q: &SearchQuery, runs: usize) -> (u64, Duration) {
    let mut best = Duration::MAX;
    let mut count = 0;
    for _ in 0..runs {
        let t = Instant::now();
        let h = start_search(doc.clone(), q, SearchOptions::default(), no_repaint()).unwrap();
        h.wait();
        best = best.min(t.elapsed());
        assert!(h.is_done());
        count = h.count();
    }
    (count, best)
}

#[test]
#[ignore = "needs fixtures-data/orders_2g.json"]
fn manifest_counts_and_speed_on_2g() {
    for path in fixtures() {
        let doc = Document::open(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let manifest = path.with_file_name(format!("{name}.manifest.json"));
        let m: Manifest = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
        // First search on a freshly opened document (page faults of the mapping included;
        // the file itself is in the OS page cache).
        let (_, first) = timed(&doc, &SearchQuery::new("NEEDLE_RARE"), 1);
        eprintln!("{name}: first search after open {first:.2?}");
        for n in &m.needles {
            let cs = SearchQuery {
                case_sensitive: true,
                ..SearchQuery::new(n.needle.clone())
            };
            let (count, t_cs) = timed(&doc, &cs, 3);
            assert_eq!(count, n.count, "{name}: {} (case-sensitive)", n.needle);
            let ci = SearchQuery::new(n.needle.clone());
            let (count_ci, t_ci) = timed(&doc, &ci, 3);
            assert_eq!(count_ci, n.count_case_insensitive, "{name}: {}", n.needle);
            eprintln!(
                "{name}: {:<14} cs {count:>8} in {t_cs:>10.2?} · ci {count_ci:>8} in {t_ci:>10.2?}",
                n.needle
            );
            assert!(
                t_cs < Duration::from_millis(1500),
                "case-sensitive {t_cs:?}"
            );
            assert!(t_ci < Duration::from_secs(3), "case-insensitive {t_ci:?}");
        }
        // Common words: many matches, whole word, Unicode.
        for (text, ww) in [
            ("email", false),
            ("id", true),
            ("e", false),
            ("ёлка", false),
        ] {
            let q = SearchQuery {
                whole_word: ww,
                ..SearchQuery::new(text)
            };
            let (count, t) = timed(&doc, &q, 2);
            eprintln!("{name}: {text:<14} ci ww={ww} {count:>10} in {t:>10.2?}");
            assert!(t < Duration::from_secs(3), "{text}: {t:?}");
        }

        // First result for a common word.
        let t = Instant::now();
        let h = start_search(
            doc.clone(),
            &SearchQuery::new("email"),
            SearchOptions::default(),
            no_repaint(),
        )
        .unwrap();
        while h.stored() == 0 {
            assert!(t.elapsed() < Duration::from_secs(5));
            std::thread::yield_now();
        }
        let first = t.elapsed();
        eprintln!("{name}: first \"email\" result after {first:?}");
        assert!(first < Duration::from_millis(200), "first result {first:?}");
        // Stop it, so its workers don't hold the pool during the cancel check below.
        h.cancel();
        h.wait();

        // Cancel of a running full-size search.
        let h = start_search(
            doc.clone(),
            &SearchQuery::new("e"),
            SearchOptions::default(),
            no_repaint(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let t = Instant::now();
        h.cancel();
        while h.is_running() {
            std::thread::yield_now();
        }
        let cancel = t.elapsed();
        eprintln!("{name}: cancel took {cancel:?}");
        assert!(cancel < Duration::from_millis(50));
    }
}
