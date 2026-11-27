//! Tree row model benchmarks on a 10,000,000-element array.
//!
//! `cargo bench -p braceray-core --bench tree -- --quick` · results go to docs/PERF.md.

use braceray_core::index::{self, IndexOptions};
use braceray_core::nav::Navigator;
use braceray_core::tree::TreeModel;
use braceray_core::{CancelToken, Progress};
use criterion::{Criterion, criterion_group, criterion_main};

fn ten_million() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(90 << 20);
    bytes.push(b'[');
    for i in 0..10_000_000u32 {
        if i > 0 {
            bytes.push(b',');
        }
        bytes.extend_from_slice(i.to_string().as_bytes());
    }
    bytes.push(b']');
    bytes
}

fn benches(c: &mut Criterion) {
    let bytes = ten_million();
    let cancel = CancelToken::new();
    let idx = index::build_bytes(
        &bytes,
        &IndexOptions::default(),
        &cancel,
        &Progress::default(),
    )
    .expect("index");
    let nav = Navigator::new(&bytes, &idx);
    let root = nav.root_container().expect("root");

    c.bench_function("tree_10m/expand_root", |b| {
        b.iter(|| {
            let mut tree = TreeModel::new(&nav, 1000);
            tree.expand(&nav, 0)
        })
    });

    // Prepared once, as the app does on a worker.
    nav.prepare(&root, &cancel).expect("prepare");
    let mut tree = TreeModel::new(&nav, 1000);
    tree.expand(&nav, 0); // 10 groups of 1,000,000
    tree.expand(&nav, 6); // group 5 → 1000 groups of 1000
    tree.expand(&nav, 7 + 500); // [5,500,000 … 5,500,999]
    let first = 7 + 500 + 1 + 400;
    // Each call starts before the cursor the previous one left, so this is the
    // parse-from-checkpoint case (children 5,500,400..5,500,450, checkpoint 5,499,904).
    c.bench_function("tree_10m/rows_for_viewport_50", |b| {
        b.iter(|| tree.rows(&nav, first..first + 50))
    });
    c.bench_function("tree_10m/prepare_root", |b| {
        b.iter(|| {
            let fresh = Navigator::new(&bytes, &idx);
            fresh.prepare(&root, &cancel)
        })
    });
}

criterion_group!(tree_benches, benches);
criterion_main!(tree_benches);
