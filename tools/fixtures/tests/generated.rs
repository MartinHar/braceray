//! Determinism, validity, needle and diff-manifest tests for generated fixtures.

mod common;

use common::*;
use gen_fixtures::manifest::{DiffManifest, RecordsManifest, manifest_path};
use gen_fixtures::orders::{RecordsOpts, generate_orders};
use gen_fixtures::{all, diff, logs, shapes};

const MIB: u64 = 1 << 20;

fn opts(size: u64, minified: bool) -> RecordsOpts {
    RecordsOpts {
        size,
        seed: 42,
        minified,
    }
}

/// Runs `f` once on a 1-thread pool and once on a 4-thread pool.
fn on_pools(f: impl Fn(usize) + Sync) {
    for threads in [1, 4] {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| f(threads));
    }
}

#[test]
fn same_seed_gives_identical_bytes_for_every_kind() {
    let t = TempDir::new("det");
    on_pools(|n| {
        let p = |k: &str| t.path(&format!("{k}_{n}"));
        generate_orders(&p("orders"), opts(MIB, false)).unwrap();
        generate_orders(&p("orders_min"), opts(MIB, true)).unwrap();
        logs::generate_ndjson(&p("logs"), opts(MIB, true)).unwrap();
        shapes::generate_deep(&p("deep"), 1000).unwrap();
        shapes::generate_wide(&p("wide"), 20_000, 42, false).unwrap();
        shapes::generate_prim(&p("prim"), 50_000, 42, false).unwrap();
        shapes::generate_string(&p("string"), MIB, 42).unwrap();
        diff::generate_diff(&p("diff"), MIB, 50, 42).unwrap();
    });
    for k in [
        "orders",
        "orders_min",
        "logs",
        "deep",
        "wide",
        "prim",
        "string",
        "diff/diff_a_1m.json",
        "diff/diff_b_1m.json",
        "diff/diff_manifest.json",
    ] {
        let (k1, k4) = match k.split_once('/') {
            Some((d, f)) => (format!("{d}_1/{f}"), format!("{d}_4/{f}")),
            None => (format!("{k}_1"), format!("{k}_4")),
        };
        let (a, b) = (read(&t.path(&k1)), read(&t.path(&k4)));
        assert!(!a.is_empty(), "{k} is empty");
        assert!(a == b, "{k}: output differs between runs / thread counts");
    }
}

#[test]
fn different_seed_gives_different_bytes() {
    let t = TempDir::new("seed");
    generate_orders(&t.path("a"), opts(64 << 10, true)).unwrap();
    generate_orders(
        &t.path("b"),
        RecordsOpts {
            seed: 7,
            ..opts(64 << 10, true)
        },
    )
    .unwrap();
    assert_ne!(read(&t.path("a")), read(&t.path("b")));
}

#[test]
fn every_valid_kind_parses_with_serde() {
    let t = TempDir::new("valid");
    let size = 10 * MIB;
    let cases: Vec<(&str, std::path::PathBuf)> = vec![
        ("orders", t.path("orders.json")),
        ("orders_min", t.path("orders_min.json")),
        ("deep", t.path("deep.json")),
        ("wide", t.path("wide.json")),
        ("wide_min", t.path("wide_min.json")),
        ("prim", t.path("prim.json")),
        ("prim_min", t.path("prim_min.json")),
        ("string", t.path("string.json")),
    ];
    generate_orders(&cases[0].1, opts(size, false)).unwrap();
    generate_orders(&cases[1].1, opts(size, true)).unwrap();
    shapes::generate_deep(&cases[2].1, 100_000).unwrap();
    shapes::generate_wide(&cases[3].1, 200_000, 42, false).unwrap();
    shapes::generate_wide(&cases[4].1, 200_000, 42, true).unwrap();
    shapes::generate_prim(&cases[5].1, 1_000_000, 42, false).unwrap();
    shapes::generate_prim(&cases[6].1, 1_000_000, 42, true).unwrap();
    shapes::generate_string(&cases[7].1, size, 42).unwrap();
    for (name, path) in cases {
        let bytes = read(&path);
        assert!(bytes.len() > 100_000, "{name}: only {} bytes", bytes.len());
        parse_ok(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    }

    let minified = read(&t.path("orders_min.json"));
    assert!(
        !minified.contains(&b'\n'),
        "minified orders must be one line"
    );

    let deep = read(&t.path("deep.json"));
    let opens = deep.iter().filter(|&&b| b == b'{' || b == b'[').count();
    assert_eq!(opens, 100_000);

    let wide: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&read(&t.path("wide.json"))).unwrap();
    assert_eq!(wide.len(), 200_000);

    let prim: Vec<f64> = serde_json::from_slice(&read(&t.path("prim.json"))).unwrap();
    assert_eq!(prim.len(), 1_000_000);

    let s: std::collections::HashMap<String, String> =
        serde_json::from_slice(&read(&t.path("string.json"))).unwrap();
    assert!(s["data"].len() > 9 * MIB as usize);
}

#[test]
fn ndjson_lines_each_parse_and_match_record_count() {
    let t = TempDir::new("ndjson");
    let p = t.path("logs.ndjson");
    let m = logs::generate_ndjson(&p, opts(10 * MIB, true)).unwrap();
    let bytes = read(&p);
    assert_eq!(bytes.last(), Some(&b'\n'));
    let mut lines = 0;
    for line in bytes.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        let v: serde_json::Value = serde_json::from_slice(line).unwrap();
        assert!(v.is_object());
        lines += 1;
    }
    assert_eq!(lines, m.records);
}

fn check_needles(path: &std::path::Path) {
    let bytes = read(path);
    let m: RecordsManifest = serde_json::from_slice(&read(&manifest_path(path))).unwrap();
    assert_eq!(m.bytes, bytes.len() as u64);
    assert!(m.bytes >= 10 * MIB, "file is smaller than requested");
    for n in &m.needles {
        let nb = n.needle.as_bytes();
        assert_eq!(
            count(&bytes, nb),
            n.count,
            "{}: {}",
            path.display(),
            n.needle
        );
        assert_eq!(
            count_ci(&bytes, nb),
            n.count_case_insensitive,
            "{} (case-insensitive): {}",
            path.display(),
            n.needle
        );
    }
    let common = m
        .needles
        .iter()
        .find(|n| n.needle == "needle_common")
        .unwrap();
    // 1 % of records, give or take one partial hundred.
    assert!(common.count.abs_diff(m.records / 100) <= 1);
}

#[test]
fn needle_counts_match_naive_count_on_10mb_files() {
    let t = TempDir::new("needles");
    generate_orders(&t.path("orders_10m.json"), opts(10 * MIB, false)).unwrap();
    generate_orders(&t.path("orders_min_10m.json"), opts(10 * MIB, true)).unwrap();
    logs::generate_ndjson(&t.path("logs_10m.ndjson"), opts(10 * MIB, true)).unwrap();
    for f in ["orders_10m.json", "orders_min_10m.json", "logs_10m.ndjson"] {
        check_needles(&t.path(f));
    }
    let v: Vec<serde_json::Value> =
        serde_json::from_slice(&read(&t.path("orders_10m.json"))).unwrap();
    let m: RecordsManifest =
        serde_json::from_slice(&read(&manifest_path(&t.path("orders_10m.json")))).unwrap();
    assert_eq!(v.len() as u64, m.records);
}

#[test]
fn tiny_size_still_plants_every_rare_needle() {
    let t = TempDir::new("tiny");
    let p = t.path("tiny.json");
    let m = generate_orders(&p, opts(1, false)).unwrap();
    assert_eq!(m.records, 17);
    assert_eq!(count(&read(&p), b"NEEDLE_RARE"), 17);
    parse_ok(read(&p)).unwrap();
}

/// Resolves a `$[i].field` path in a parsed orders array.
fn lookup<'a>(v: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let rest = path.strip_prefix("$[")?;
    let (idx, field) = rest.split_once("].")?;
    v.get(idx.parse::<usize>().ok()?)?.get(field)
}

#[test]
fn diff_manifest_lists_exactly_the_changes() {
    let t = TempDir::new("diff");
    let m: DiffManifest = diff::generate_diff(&t.0, 4 * MIB, 40, 42).unwrap();
    let a: serde_json::Value = serde_json::from_slice(&read(&t.path(&m.file_a))).unwrap();
    let b: serde_json::Value = serde_json::from_slice(&read(&t.path(&m.file_b))).unwrap();
    let (aa, ba) = (a.as_array().unwrap(), b.as_array().unwrap());
    assert_eq!(aa.len() as u64, m.records);
    assert_eq!(ba.len(), aa.len());
    assert_eq!(m.changes.len(), 40);

    let as_json = |v: Option<&serde_json::Value>| v.map(|v| serde_json::to_string(v).unwrap());
    let norm = |s: &Option<String>| {
        s.as_ref().map(|s| {
            serde_json::to_string(&serde_json::from_str::<serde_json::Value>(s).unwrap()).unwrap()
        })
    };
    let mut changed = std::collections::HashSet::new();
    for c in &m.changes {
        assert_eq!(as_json(lookup(&a, &c.path)), norm(&c.a), "A at {}", c.path);
        assert_eq!(as_json(lookup(&b, &c.path)), norm(&c.b), "B at {}", c.path);
        let idx: usize = c.path[2..c.path.find(']').unwrap()].parse().unwrap();
        changed.insert(idx);
    }
    assert_eq!(changed.len(), 40, "changes must hit distinct records");
    let differing: Vec<usize> = (0..aa.len()).filter(|&i| aa[i] != ba[i]).collect();
    let mut expected: Vec<usize> = changed.into_iter().collect();
    expected.sort_unstable();
    assert_eq!(differing, expected, "only manifest records may differ");
}

#[test]
fn all_creates_every_fixture_type() {
    let t = TempDir::new("all");
    all::generate_all(&t.0, 2 * MIB, 42).unwrap();
    for f in [
        "orders_2m.json",
        "orders_min_2m.json",
        "orders_2m.json.manifest.json",
        "logs_2m.ndjson",
        "logs_2m.ndjson.manifest.json",
        "deep_100k.json",
        "huge_string.json",
        "invalid/invalid_manifest.json",
        "invalid/invalid_end_2m.json",
        "encoding/encoding_manifest.json",
        "encoding/utf16le.json",
        "diff/diff_a_2m.json",
        "diff/diff_b_2m.json",
        "diff/diff_manifest.json",
    ] {
        assert!(t.path(f).is_file(), "missing {f}");
    }
    let names: Vec<String> = std::fs::read_dir(&t.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().any(|n| n.starts_with("wide_")), "{names:?}");
    assert!(
        names.iter().any(|n| n.starts_with("prim_array_")),
        "{names:?}"
    );
}

#[test]
fn full_catalogue_names_match_testing_strategy() {
    let jobs = all::catalogue(3 << 30);
    let orders = jobs
        .iter()
        .filter(|j| matches!(j, all::Job::Orders { .. }))
        .count();
    assert_eq!(orders, 10, "10m/100m/1g/2g/3g × pretty/minified");
    assert!(
        jobs.iter()
            .any(|j| matches!(j, all::Job::Wide { keys: 1_000_000 }))
    );
    assert!(
        jobs.iter()
            .any(|j| matches!(j, all::Job::Prim { count: 10_000_000 }))
    );
    assert_eq!(all::count_label(100_000), "100k");
    assert_eq!(all::count_label(10_000_000), "10m");
}
