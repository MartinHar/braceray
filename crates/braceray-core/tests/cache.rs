//! Index cache (step 05): hits, staleness, corruption, versions, settings, eviction and
//! concurrent writers. Every test uses its own temp cache dir.

mod common;

use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use braceray_core::cache::{
    CacheSettings, IndexCache, IndexSource, SkipReason, StoreOutcome, clear_index_cache,
    load_or_build,
};
use braceray_core::index::{IndexOptions, build_index};
use braceray_core::nav::Navigator;
use braceray_core::{CancelToken, Document, Index, Progress};
use proptest::prelude::*;

/// Settings that cache files of any size.
fn all_sizes() -> CacheSettings {
    CacheSettings {
        index_cache_min_file_bytes: 0,
        ..CacheSettings::default()
    }
}

struct Env {
    dir: tempfile::TempDir,
    cache: IndexCache,
}

impl Env {
    fn new(settings: CacheSettings) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let cache = IndexCache::new(dir.path().join("cache"), settings);
        Env { dir, cache }
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.dir.path().join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    fn cache_files(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = fs::read_dir(self.cache.dir())
            .map(|r| r.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    fn entries(&self) -> Vec<PathBuf> {
        self.cache_files()
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "brx"))
            .collect()
    }
}

fn build(doc: &Document, opts: &IndexOptions) -> Index {
    build_index(doc, opts, &CancelToken::new(), &Progress::default()).unwrap()
}

/// `load_or_build`, waiting for the background write.
fn open(path: &Path, cache: &IndexCache) -> (Arc<Index>, IndexSource) {
    let doc = Document::open(path).unwrap();
    let opts = IndexOptions::default();
    let l = load_or_build(
        &doc,
        &opts,
        Some(cache),
        &CancelToken::new(),
        &Progress::default(),
    )
    .unwrap();
    if let Some(w) = l.pending_write {
        w.join().unwrap().unwrap();
    }
    // Distinct `last_access` times (milliseconds) for the eviction order.
    std::thread::sleep(Duration::from_millis(3));
    (l.index, l.source)
}

fn orders() -> Vec<u8> {
    fs::read(common::fixture("generated/orders_16k.json")).unwrap()
}

#[test]
fn hit_equals_build_and_navigates() {
    let env = Env::new(all_sizes());
    let path = env.file("orders.json", &orders());
    let (built, src) = open(&path, &env.cache);
    assert_eq!(src, IndexSource::Built);
    assert!(!built.is_from_cache());
    assert_eq!(env.entries().len(), 1);

    let (cached, src) = open(&path, &env.cache);
    assert_eq!(src, IndexSource::Cache);
    assert!(cached.is_from_cache());
    assert_eq!(*cached, *built);

    // Navigation over the mapped node table gives the same answers.
    let doc = Document::open(&path).unwrap();
    let (a, b) = (
        Navigator::new(doc.bytes(), &built),
        Navigator::new(doc.bytes(), &cached),
    );
    for off in (0..doc.len()).step_by(997) {
        let (la, lb) = (a.node_at_offset(off), b.node_at_offset(off));
        assert_eq!(a.path_of(&la), b.path_of(&lb), "offset {off}");
    }
    // A cached index isn't written again.
    assert_eq!(
        env.cache
            .store(&doc, &cached, &IndexOptions::default())
            .unwrap(),
        StoreOutcome::Skipped(SkipReason::FromCache)
    );
}

#[test]
fn every_golden_fixture_round_trips() {
    let env = Env::new(all_sizes());
    let mut names = Vec::new();
    for dir in ["valid", "invalid", "generated"] {
        for e in fs::read_dir(common::fixture(dir)).unwrap().flatten() {
            let p = e.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if name.ends_with("manifest.json") {
                continue;
            }
            names.push(p);
        }
    }
    let mut invalid = 0;
    for p in names {
        let Ok(doc) = Document::open(&p) else {
            continue;
        };
        let opts = IndexOptions::default();
        let built = build(&doc, &opts);
        invalid += usize::from(!built.is_valid());
        assert!(matches!(
            env.cache.store(&doc, &built, &opts).unwrap(),
            StoreOutcome::Written(_)
        ));
        let cached = env
            .cache
            .load(&doc, &opts, &CancelToken::new())
            .unwrap_or_else(|| panic!("no cache hit for {}", p.display()));
        assert_eq!(cached, built, "{}", p.display());
    }
    assert!(invalid >= 12, "only {invalid} invalid fixtures");
}

#[test]
fn adaptive_mode_round_trips() {
    let env = Env::new(all_sizes());
    let mut json = b"[".to_vec();
    for i in 0..50_000 {
        if i > 0 {
            json.push(b',');
        }
        json.extend_from_slice(br#"{"a":[1]}"#);
    }
    json.push(b']');
    let path = env.file("tiny.json", &json);
    let doc = Document::open(&path).unwrap();
    let opts = IndexOptions {
        memory_budget: 64 << 10,
        ..IndexOptions::default()
    };
    let built = build(&doc, &opts);
    assert!(built.max_stored_depth().is_some());
    env.cache.store(&doc, &built, &opts).unwrap();
    let cached = env.cache.load(&doc, &opts, &CancelToken::new()).unwrap();
    assert_eq!(cached, built);
    // Other options → stale: the entry is ignored (and deleted).
    assert!(
        env.cache
            .load(&doc, &IndexOptions::default(), &CancelToken::new())
            .is_none()
    );
    assert!(env.entries().is_empty());
}

#[test]
fn modified_source_is_rebuilt() {
    let env = Env::new(all_sizes());
    let path = env.file("orders.json", &orders());
    assert_eq!(open(&path, &env.cache).1, IndexSource::Built);
    assert_eq!(open(&path, &env.cache).1, IndexSource::Cache);

    // Append a byte (whitespace keeps it valid).
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    let (idx, src) = open(&path, &env.cache);
    assert_eq!(src, IndexSource::Built);
    assert_eq!(idx.len(), orders().len() as u64 + 1);
    assert_eq!(open(&path, &env.cache).1, IndexSource::Cache);

    // Touch the mtime only.
    let f = OpenOptions::new().write(true).open(&path).unwrap();
    f.set_modified(SystemTime::now() + Duration::from_secs(10))
        .unwrap();
    drop(f);
    assert_eq!(open(&path, &env.cache).1, IndexSource::Built);
    assert_eq!(open(&path, &env.cache).1, IndexSource::Cache);
}

/// Damages the single entry with `damage`, then checks it's detected, deleted and rebuilt.
fn check_damage(name: &str, damage: impl Fn(&Path, u64)) {
    let env = Env::new(all_sizes());
    let path = env.file("orders.json", &orders());
    let (built, _) = open(&path, &env.cache);
    let entry = env.entries().pop().unwrap();
    let len = fs::metadata(&entry).unwrap().len();
    damage(&entry, len);

    let doc = Document::open(&path).unwrap();
    let opts = IndexOptions::default();
    assert!(
        env.cache.load(&doc, &opts, &CancelToken::new()).is_none(),
        "{name}: damaged entry was used"
    );
    assert!(!entry.exists(), "{name}: damaged entry not deleted");
    let (rebuilt, src) = open(&path, &env.cache);
    assert_eq!(src, IndexSource::Built, "{name}");
    assert_eq!(*rebuilt, *built, "{name}");
    assert_eq!(open(&path, &env.cache).1, IndexSource::Cache, "{name}");
}

fn flip(entry: &Path, at: u64) {
    let mut bytes = fs::read(entry).unwrap();
    bytes[at as usize] ^= 0x40;
    fs::write(entry, bytes).unwrap();
}

#[test]
fn truncated_entry_is_rebuilt() {
    for keep in [0, 1, 100, 255, 256, 300] {
        check_damage(&format!("truncate to {keep}"), |p, _| {
            OpenOptions::new()
                .write(true)
                .open(p)
                .unwrap()
                .set_len(keep)
                .unwrap()
        });
    }
    check_damage("truncate last byte", |p, len| {
        OpenOptions::new()
            .write(true)
            .open(p)
            .unwrap()
            .set_len(len - 1)
            .unwrap()
    });
    check_damage("half", |p, len| {
        OpenOptions::new()
            .write(true)
            .open(p)
            .unwrap()
            .set_len(len / 2)
            .unwrap()
    });
    check_damage("extra byte", |p, _| {
        let mut f = OpenOptions::new().append(true).open(p).unwrap();
        f.write_all(b"x").unwrap();
    });
}

#[test]
fn flipped_bytes_are_detected() {
    // Header fields, node table, line table and the checksum itself.
    for at in [0u64, 9, 13, 20, 40, 81, 100, 150, 255, 256, 300, 1000] {
        check_damage(&format!("flip {at}"), |p, _| flip(p, at));
    }
    check_damage("flip middle", |p, len| flip(p, len / 2));
    check_damage("flip line table", |p, len| flip(p, len - 12));
    check_damage("flip checksum", |p, len| flip(p, len - 1));
    check_damage("garbage", |p, len| {
        fs::write(p, vec![0xA5u8; len as usize]).unwrap()
    });
}

/// Overwrites bytes at `at` without changing the length.
fn patch(entry: &Path, at: u64, bytes: &[u8]) {
    let mut f = OpenOptions::new().write(true).open(entry).unwrap();
    f.seek(SeekFrom::Start(at)).unwrap();
    f.write_all(bytes).unwrap();
}

#[test]
fn other_format_or_app_version_is_rebuilt() {
    check_damage("format version 0", |p, _| patch(p, 8, &0u32.to_le_bytes()));
    check_damage("format version 2", |p, _| patch(p, 8, &2u32.to_le_bytes()));
    check_damage("app version", |p, _| {
        patch(p, 16, b"0.0.0-old\0\0\0\0\0\0\0")
    });
}

#[test]
fn disabled_cache_writes_nothing() {
    let env = Env::new(CacheSettings {
        index_cache_enabled: false,
        index_cache_min_file_bytes: 0,
        ..CacheSettings::default()
    });
    let path = env.file("orders.json", &orders());
    for _ in 0..2 {
        assert_eq!(open(&path, &env.cache).1, IndexSource::Built);
    }
    let doc = Document::open(&path).unwrap();
    let opts = IndexOptions::default();
    let idx = build(&doc, &opts);
    assert_eq!(
        env.cache.store(&doc, &idx, &opts).unwrap(),
        StoreOutcome::Skipped(SkipReason::Disabled)
    );
    assert!(!env.cache.dir().exists(), "cache dir was created");

    // An entry written while enabled isn't read while disabled.
    let mut on = env.cache.clone();
    on.set_settings(all_sizes());
    on.store(&doc, &idx, &opts).unwrap();
    assert!(env.cache.load(&doc, &opts, &CancelToken::new()).is_none());
    assert!(on.load(&doc, &opts, &CancelToken::new()).is_some());
}

#[test]
fn small_files_are_not_cached_by_default() {
    let env = Env::new(CacheSettings::default());
    let path = env.file("orders.json", &orders());
    let doc = Document::open(&path).unwrap();
    let l = load_or_build(
        &doc,
        &IndexOptions::default(),
        Some(&env.cache),
        &CancelToken::new(),
        &Progress::default(),
    )
    .unwrap();
    assert!(l.pending_write.is_none());
    let idx = build(&doc, &IndexOptions::default());
    assert_eq!(
        env.cache
            .store(&doc, &idx, &IndexOptions::default())
            .unwrap(),
        StoreOutcome::Skipped(SkipReason::TooSmall)
    );
    assert!(!env.cache.dir().exists());
}

#[test]
fn eviction_keeps_total_under_limit() {
    let body = orders();
    let probe = Env::new(all_sizes());
    let p = probe.file("a.json", &body);
    open(&p, &probe.cache);
    let entry_size = probe.cache.total_bytes();
    assert!(entry_size > 0);

    let limit = entry_size * 5 / 2; // room for two entries
    let env = Env::new(CacheSettings {
        index_cache_max_bytes: limit,
        ..all_sizes()
    });
    let paths: Vec<PathBuf> = (0..6)
        .map(|i| env.file(&format!("f{i}.json"), &body))
        .collect();
    for p in &paths {
        open(p, &env.cache);
        assert!(env.cache.total_bytes() <= limit);
    }
    assert_eq!(env.entries().len(), 2);
    // Least recently used went first: the last two are still cached.
    assert_eq!(open(&paths[5], &env.cache).1, IndexSource::Cache);
    assert_eq!(open(&paths[4], &env.cache).1, IndexSource::Cache);
    // Re-opening f0 rebuilds it and evicts the least recently used (f5).
    assert_eq!(open(&paths[0], &env.cache).1, IndexSource::Built);
    assert_eq!(open(&paths[4], &env.cache).1, IndexSource::Cache);
    assert_eq!(open(&paths[5], &env.cache).1, IndexSource::Built);
    assert!(env.cache.total_bytes() <= limit);

    // An entry bigger than the whole limit isn't written.
    let tiny = Env::new(CacheSettings {
        index_cache_max_bytes: entry_size - 1,
        ..all_sizes()
    });
    let p = tiny.file("a.json", &body);
    let doc = Document::open(&p).unwrap();
    let idx = build(&doc, &IndexOptions::default());
    assert_eq!(
        tiny.cache
            .store(&doc, &idx, &IndexOptions::default())
            .unwrap(),
        StoreOutcome::Skipped(SkipReason::TooLarge)
    );
}

#[test]
fn clear_frees_everything() {
    let env = Env::new(all_sizes());
    assert_eq!(clear_index_cache(&env.cache).unwrap(), 0); // no dir yet
    for i in 0..3 {
        let p = env.file(&format!("f{i}.json"), &orders());
        open(&p, &env.cache);
    }
    let total = env.cache.total_bytes();
    assert_eq!(env.entries().len(), 3);
    let freed = clear_index_cache(&env.cache).unwrap();
    assert!(freed >= total, "freed {freed} < {total}");
    assert!(env.cache_files().is_empty(), "{:?}", env.cache_files());
}

#[test]
fn concurrent_writers_and_readers_never_see_a_broken_entry() {
    let env = Env::new(all_sizes());
    let path = env.file("orders.json", &orders());
    let doc = Document::open(&path).unwrap();
    let opts = IndexOptions::default();
    let built = build(&doc, &opts);

    std::thread::scope(|s| {
        for _ in 0..6 {
            s.spawn(|| {
                // Each "instance" has its own `IndexCache` value on the same dir.
                let cache = env.cache.clone();
                for _ in 0..10 {
                    let out = cache.store(&doc, &built, &opts).unwrap();
                    assert!(matches!(out, StoreOutcome::Written(_)));
                }
            });
        }
        for _ in 0..6 {
            s.spawn(|| {
                let cache = env.cache.clone();
                for _ in 0..40 {
                    // A reader never deletes a good entry, and never gets a wrong one.
                    if let Some(idx) = cache.load(&doc, &opts, &CancelToken::new()) {
                        assert_eq!(idx, built);
                    }
                }
            });
        }
    });
    let loaded = env.cache.load(&doc, &opts, &CancelToken::new()).unwrap();
    assert_eq!(loaded, built);
    let leftovers: Vec<_> = env
        .cache_files()
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert_eq!(env.entries().len(), 1);
}

#[test]
fn cancelled_load_keeps_the_entry() {
    let env = Env::new(all_sizes());
    let path = env.file("orders.json", &orders());
    open(&path, &env.cache);
    let doc = Document::open(&path).unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(
        env.cache
            .load(&doc, &IndexOptions::default(), &cancel)
            .is_none()
    );
    assert_eq!(env.entries().len(), 1);
    assert_eq!(open(&path, &env.cache).1, IndexSource::Cache);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Random valid and mutated (mostly invalid) documents survive a cache round trip,
    /// including every error message the parser produces.
    #[test]
    fn random_documents_round_trip(
        tree in common::json_tree(),
        layout in common::layout(),
        mutation in proptest::option::of((0u8..3, any::<usize>(), common::mutation_byte())),
    ) {
        let (mut bytes, _) = common::serialize(&tree, &layout);
        if let Some((op, pos, byte)) = mutation {
            bytes = common::mutate(&bytes, op, pos, byte);
        }
        let env = Env::new(all_sizes());
        let path = env.file("doc.json", &bytes);
        let Ok(doc) = Document::open(&path) else { return Ok(()) };
        let opts = IndexOptions::default();
        let built = build(&doc, &opts);
        env.cache.store(&doc, &built, &opts).unwrap();
        let cached = env.cache.load(&doc, &opts, &CancelToken::new());
        prop_assert_eq!(cached.as_ref(), Some(&built));
    }
}
