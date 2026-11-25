//! Index benchmarks on a 100 MB `orders` fixture (generated into `target/bench-fixtures`).
//!
//! `cargo bench -p braceray-core -- --quick` · results go to docs/PERF.md.

use std::path::PathBuf;
use std::time::Duration;

use braceray_core::index::{self, IndexOptions, scalar};
use braceray_core::nav::Navigator;
use braceray_core::{CancelToken, Progress};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

fn fixture_100m() -> Vec<u8> {
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
    std::fs::read(&path).expect("read fixture")
}

fn million_array() -> Vec<u8> {
    let mut bytes = b"[".to_vec();
    for i in 0..1_000_000u32 {
        if i > 0 {
            bytes.extend_from_slice(b", ");
        }
        bytes.extend_from_slice(i.to_string().as_bytes());
    }
    bytes.push(b']');
    bytes
}

fn benches(c: &mut Criterion) {
    let bytes = fixture_100m();
    let cancel = CancelToken::new();
    let progress = Progress::default();
    let mut g = c.benchmark_group("index_100m");
    g.throughput(Throughput::Bytes(bytes.len() as u64));
    g.sample_size(10);
    g.measurement_time(Duration::from_secs(10));
    g.bench_function("scalar_index", |b| {
        b.iter(|| scalar::build(&bytes, None, &cancel, &progress).expect("index"))
    });
    for (name, threads) in [
        ("simd_index_1thread", Some(1)),
        ("simd_index_parallel", None),
    ] {
        let opts = IndexOptions {
            threads,
            ..IndexOptions::default()
        };
        g.bench_function(name, |b| {
            b.iter(|| index::build_bytes(&bytes, &opts, &cancel, &progress).expect("index"))
        });
    }
    g.finish();

    let array = million_array();
    let idx =
        index::build_bytes(&array, &IndexOptions::default(), &cancel, &progress).expect("index");
    c.bench_function("children_of_1m_array", |b| {
        b.iter(|| {
            // A fresh navigator: includes building the checkpoint table.
            let nav = Navigator::new(&array, &idx);
            let root = nav.root_container().expect("root");
            nav.children(&root, 500_000..500_050)
        })
    });
}

criterion_group!(index_benches, benches);
criterion_main!(index_benches);
