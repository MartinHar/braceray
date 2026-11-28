//! Copy benchmarks: a 10 MB `orders` object (pretty fixture, generated into
//! `target/bench-fixtures`) copied as in the file, pretty and minified, and a 10 MB
//! string's decoded contents. Target: < 300 ms each (step 11).
//!
//! `cargo bench -p braceray-core --bench copy -- --quick` · results go to docs/PERF.md.

use std::path::PathBuf;

use braceray_core::copy::{CopyStyle, render};
use braceray_core::nav::Span;
use braceray_core::{CancelToken, Progress};
use criterion::{Criterion, criterion_group, criterion_main};

fn fixture() -> Vec<u8> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/bench-fixtures");
    std::fs::create_dir_all(&dir).expect("create bench fixture dir");
    let path = dir.join("orders_10m.json");
    if !path.exists() {
        let opts = gen_fixtures::orders::RecordsOpts {
            size: 10 << 20,
            seed: 42,
            minified: false,
        };
        gen_fixtures::orders::generate_orders(&path, opts).expect("generate fixture");
    }
    std::fs::read(&path).expect("read fixture")
}

fn benches(c: &mut Criterion) {
    let bytes = fixture();
    let all = Span {
        start: 0,
        end: bytes.len() as u64,
    };
    let (cancel, progress) = (CancelToken::new(), Progress::default());
    let mut g = c.benchmark_group("copy_10m");
    for (name, style) in [
        ("raw", CopyStyle::Raw),
        ("pretty", CopyStyle::Pretty { indent: 2 }),
        ("minified", CopyStyle::Minified),
    ] {
        g.bench_function(name, |b| {
            b.iter(|| render(&bytes, all, style, usize::MAX, &cancel, &progress).expect("render"))
        });
    }
    let mut s = b"\"".to_vec();
    while s.len() < 10 << 20 {
        s.extend_from_slice(br#"plain text \"quoted\" \u00e9 \n "#);
    }
    s.push(b'"');
    let span = Span {
        start: 0,
        end: s.len() as u64,
    };
    g.bench_function("string_contents", |b| {
        b.iter(|| {
            render(
                &s,
                span,
                CopyStyle::StringContents,
                usize::MAX,
                &cancel,
                &progress,
            )
            .expect("render")
        })
    });
    g.finish();
}

criterion_group!(copy, benches);
criterion_main!(copy);
