//! Raw view benchmarks on a 100 MB minified `orders` fixture (generated into
//! `target/bench-fixtures`): one viewport of pretty rows, jumping to an offset (anchor +
//! seek), stepping back a viewport, and original-mode rows on the pretty fixture.
//!
//! `cargo bench -p braceray-core --bench pretty -- --quick` · results go to docs/PERF.md.

use std::path::PathBuf;

use braceray_core::index::{self, IndexOptions};
use braceray_core::nav::Navigator;
use braceray_core::pretty::original::{next_row, row_at};
use braceray_core::pretty::{
    PrettyOptions, PrettyPos, Seek, anchor_before, pretty_lines, prev_line, seek,
};
use braceray_core::{CancelToken, Progress};
use criterion::{Criterion, criterion_group, criterion_main};

const VIEWPORT: usize = 60;

fn fixture(name: &str, minified: bool) -> Vec<u8> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/bench-fixtures");
    std::fs::create_dir_all(&dir).expect("create bench fixture dir");
    let path = dir.join(name);
    if !path.exists() {
        let opts = gen_fixtures::orders::RecordsOpts {
            size: 100 << 20,
            seed: 42,
            minified,
        };
        gen_fixtures::orders::generate_orders(&path, opts).expect("generate fixture");
    }
    std::fs::read(&path).expect("read fixture")
}

fn found(s: Seek) -> PrettyPos {
    match s {
        Seek::Found(p) => p,
        Seek::More(_) => unreachable!("unlimited budget"),
    }
}

fn benches(c: &mut Criterion) {
    let opts = PrettyOptions::default();
    let cancel = CancelToken::new();

    let min = fixture("orders_min_100m.json", true);
    let idx = index::build_bytes(
        &min,
        &IndexOptions::default(),
        &cancel,
        &Progress::default(),
    )
    .expect("index");
    let nav = Navigator::new(&min, &idx);
    if let Some(root) = nav.root_container() {
        nav.prepare(&root, &cancel).expect("prepare");
    }
    let mid = min.len() as u64 * 63 / 100;
    let at = found(seek(&min, anchor_before(&nav, mid), mid, usize::MAX, &opts));

    c.bench_function("pretty_100m/viewport_rows", |b| {
        b.iter(|| pretty_lines(&min, at, VIEWPORT, &opts))
    });
    let mut k = 0u64;
    c.bench_function("pretty_100m/jump_to_offset", |b| {
        b.iter(|| {
            // A different offset each time (thumb drag).
            k = (k + 7_919) % 10_000;
            let x = min.len() as u64 * k / 10_000;
            found(seek(&min, anchor_before(&nav, x), x, usize::MAX, &opts))
        })
    });
    c.bench_function("pretty_100m/viewport_back", |b| {
        b.iter(|| {
            let mut p = at;
            for _ in 0..VIEWPORT {
                p = found(prev_line(&nav, p, usize::MAX, &opts).expect("not the first row"));
            }
            p
        })
    });

    let pretty = fixture("orders_100m.json", false);
    let idx = index::build_bytes(
        &pretty,
        &IndexOptions::default(),
        &cancel,
        &Progress::default(),
    )
    .expect("index");
    let lines = idx.lines();
    let mut k = 0u64;
    c.bench_function("original_100m/jump_and_viewport", |b| {
        b.iter(|| {
            k = (k + 7_919) % 10_000;
            let x = pretty.len() as u64 * k / 10_000;
            let mut r = row_at(&pretty, lines, x, opts.wrap);
            for _ in 0..VIEWPORT {
                r = next_row(&pretty, &r, opts.wrap).unwrap_or(r);
            }
            r
        })
    });
}

criterion_group!(pretty, benches);
criterion_main!(pretty);
