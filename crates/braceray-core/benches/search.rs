//! Search benchmarks on a 100 MB `orders` fixture (generated into `target/bench-fixtures`):
//! full exact counts, case-sensitive and case-insensitive, for a rare and a common word.
//!
//! `cargo bench -p braceray-core --bench search -- --quick` · results go to docs/PERF.md.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use braceray_core::job::no_repaint;
use braceray_core::search::{SearchOptions, SearchQuery, start_search};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

fn fixture_100m() -> Arc<Vec<u8>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/bench-fixtures");
    std::fs::create_dir_all(&dir).expect("create bench fixture dir");
    let path = dir.join("orders_100m.json");
    if !path.exists() {
        let opts = gen_fixtures::orders::RecordsOpts {
            size: 100 << 20,
            seed: 42,
            minified: false,
        };
        gen_fixtures::orders::generate_orders(&path, opts).expect("generate fixture");
    }
    Arc::new(std::fs::read(&path).expect("read fixture"))
}

fn count(bytes: &Arc<Vec<u8>>, q: &SearchQuery) -> u64 {
    let h = start_search(bytes.clone(), q, SearchOptions::default(), no_repaint()).expect("search");
    h.wait();
    h.count()
}

fn benches(c: &mut Criterion) {
    let bytes = fixture_100m();
    for (group, cs) in [("search_cs_100m", true), ("search_ci_100m", false)] {
        let mut g = c.benchmark_group(group);
        g.throughput(Throughput::Bytes(bytes.len() as u64));
        g.sample_size(20);
        g.measurement_time(Duration::from_secs(5));
        for (name, text) in [
            ("rare", "NEEDLE_RARE"),
            ("common", "email"),
            ("unicode", "ёлка"),
        ] {
            let q = SearchQuery {
                case_sensitive: cs,
                ..SearchQuery::new(text)
            };
            g.bench_function(name, |b| b.iter(|| count(&bytes, &q)));
        }
        g.finish();
    }
}

criterion_group!(search_benches, benches);
criterion_main!(search_benches);
