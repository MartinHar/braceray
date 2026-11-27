//! Index cache benchmarks on the 100 MB `orders` fixture (generated into
//! `target/bench-fixtures`): loading an entry (map + checksum + checks) and writing one.
//!
//! `cargo bench -p braceray-core -- --quick` · results go to docs/PERF.md.

use std::path::PathBuf;
use std::time::Duration;

use braceray_core::cache::{CacheSettings, IndexCache};
use braceray_core::index::{IndexOptions, build_index};
use braceray_core::{CancelToken, Document, Progress};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

fn fixture_100m() -> PathBuf {
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
    path
}

fn benches(c: &mut Criterion) {
    let doc = Document::open(&fixture_100m()).expect("open fixture");
    let (opts, cancel) = (IndexOptions::default(), CancelToken::new());
    let index = build_index(&doc, &opts, &cancel, &Progress::default()).expect("index");
    let dir = tempfile::tempdir().expect("temp dir");
    let cache = IndexCache::new(dir.path().to_owned(), CacheSettings::default());
    cache.store(&doc, &index, &opts).expect("store");

    let mut g = c.benchmark_group("cache_100m");
    g.throughput(Throughput::Bytes(doc.len()));
    g.sample_size(20);
    g.measurement_time(Duration::from_secs(5));
    g.bench_function("cache_load", |b| {
        b.iter(|| cache.load(&doc, &opts, &cancel).expect("cache hit"))
    });
    g.bench_function("cache_store", |b| {
        b.iter(|| cache.store(&doc, &index, &opts).expect("store"))
    });
    g.finish();
}

criterion_group!(cache_benches, benches);
criterion_main!(cache_benches);
