//! `gen-fixtures bench-big`: measures the performance targets of `docs/specs/overview.md`
//! on the 2 GB fixtures and prints Markdown rows for `docs/PERF.md`.
//!
//! Per file: open → first content (tree preview rows), full index (warm, and cold when the
//! page cache can be dropped), index memory, re-open from the index cache, plain and
//! case-insensitive search for a rare and a common needle, expand the root, peak RSS.

pub mod machine;
mod report;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use braceray_core::cache::{CacheSettings, IndexCache, IndexSource, load_or_build};
use braceray_core::index::{IndexOptions, build_index};
use braceray_core::job::no_repaint;
use braceray_core::nav::{NavCache, Navigator};
use braceray_core::search::{SearchOptions, SearchQuery, start_search};
use braceray_core::tree::TreeModel;
use braceray_core::tree::preview::{PREVIEW_BYTES, Preview};
use braceray_core::{CancelToken, Document, Progress};

use crate::manifest::{RecordsManifest, manifest_path};
use crate::orders::{RecordsOpts, generate_orders};
pub use report::Report;

/// Options of one `bench-big` run.
#[derive(Debug, Clone)]
pub struct BenchOpts {
    /// Directory with (or for) the fixtures.
    pub dir: PathBuf,
    /// Fixture files to measure, generated when missing.
    pub files: Vec<String>,
    /// Size of generated fixtures.
    pub size: u64,
    /// Repetitions for the warm numbers (best is reported).
    pub runs: usize,
    /// Also try cold numbers (drops the page cache; needs root).
    pub cold: bool,
}

/// One measured value with its target.
#[derive(Debug, Clone)]
pub struct Row {
    pub benchmark: String,
    pub fixture: String,
    pub result: String,
    /// `Some(true)` met, `Some(false)` missed, `None` no target.
    pub met: Option<bool>,
    pub notes: String,
}

impl Row {
    fn new(benchmark: &str, fixture: &str, result: String) -> Self {
        Self {
            benchmark: benchmark.to_owned(),
            fixture: fixture.to_owned(),
            result,
            met: None,
            notes: String::new(),
        }
    }

    fn target(mut self, met: bool, target: &str) -> Self {
        self.met = Some(met);
        self.notes = format!("target {target}");
        self
    }

    fn note(mut self, note: impl Into<String>) -> Self {
        let note = note.into();
        if self.notes.is_empty() {
            self.notes = note;
        } else {
            self.notes = format!("{}; {note}", self.notes);
        }
        self
    }
}

fn io_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

fn log(msg: &str) {
    if !crate::records::QUIET.load(std::sync::atomic::Ordering::Relaxed) {
        eprintln!("bench-big: {msg}");
    }
}

fn ms(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1e3;
    if ms >= 1000.0 {
        format!("{:.2} s", ms / 1e3)
    } else if ms >= 10.0 {
        format!("{ms:.0} ms")
    } else if ms >= 0.1 {
        format!("{ms:.2} ms")
    } else {
        format!("{:.1} µs", ms * 1e3)
    }
}

/// Runs every measurement and returns the report.
pub fn run(opts: &BenchOpts) -> io::Result<Report> {
    std::fs::create_dir_all(&opts.dir)?;
    let mut report = Report::new(machine::MachineInfo::detect());
    for name in &opts.files {
        let path = opts.dir.join(name);
        ensure_fixture(&path, opts.size)?;
        report.rows.extend(bench_file(&path, opts)?);
    }
    Ok(report)
}

/// Generates an `orders` fixture (pretty, or minified when the name contains `_min`).
fn ensure_fixture(path: &Path, size: u64) -> io::Result<()> {
    if path.exists() && manifest_path(path).exists() {
        return Ok(());
    }
    let minified = path
        .file_name()
        .is_some_and(|n| n.to_string_lossy().contains("_min"));
    log(&format!("generating {}", path.display()));
    generate_orders(
        path,
        RecordsOpts {
            size,
            seed: crate::DEFAULT_SEED,
            minified,
        },
    )
    .map(drop)
}

fn bench_file(path: &Path, opts: &BenchOpts) -> io::Result<Vec<Row>> {
    let name = crate::manifest::file_name(path);
    let manifest: RecordsManifest =
        serde_json::from_slice(&std::fs::read(manifest_path(path))?).map_err(io_err)?;
    let peak = machine::PeakRss::start();
    let rss_before = machine::rss();
    let mut rows = Vec::new();
    let cancel = CancelToken::new();
    let idx_opts = IndexOptions::default();

    // Cold numbers first, before the warm runs pull the file into the page cache.
    if opts.cold {
        if machine::drop_page_cache() {
            log(&format!("{name}: cold first content"));
            let t = first_content(path, &cancel)?;
            rows.push(
                Row::new("open → first content (cold)", &name, ms(t))
                    .target(t < Duration::from_secs(1), "< 1 s"),
            );
            machine::drop_page_cache();
            log(&format!("{name}: cold index"));
            let t = Instant::now();
            let doc = Document::open(path).map_err(io_err)?;
            build_index(&doc, &idx_opts, &cancel, &Progress::default()).map_err(io_err)?;
            let t = t.elapsed();
            rows.push(
                Row::new("full index (cold)", &name, ms(t))
                    .target(t < Duration::from_secs(10), "< 10 s")
                    .note("page cache dropped before the run"),
            );
        } else {
            log("cold: can't drop the page cache here (needs root); skipped");
            rows.push(
                Row::new("cold numbers", &name, "not measured".to_owned())
                    .note("dropping the page cache needs root (`sudo purge` / drop_caches)"),
            );
        }
    }

    log(&format!("{name}: warm first content"));
    // One untimed pass pulls the file into the page cache.
    std::hint::black_box(Document::open(path).map_err(io_err)?.bytes().len());
    let t = best_of(opts.runs, || first_content(path, &cancel).map(drop))?;
    rows.push(
        Row::new("open → first content (warm)", &name, ms(t))
            .target(t < Duration::from_secs(1), "< 1 s")
            .note(format!(
                "`Document::open` + {} MiB preview index + 50 tree rows",
                PREVIEW_BYTES >> 20
            )),
    );

    log(&format!("{name}: warm index"));
    let doc = Document::open(path).map_err(io_err)?;
    let mut index = None;
    let t = best_of(opts.runs, || {
        let built = build_index(&doc, &idx_opts, &cancel, &Progress::default()).map_err(io_err)?;
        index = Some(built);
        Ok(())
    })?;
    let index = index.ok_or_else(|| io_err("no index built"))?;
    let gbps = doc.len() as f64 / t.as_secs_f64() / 1e9;
    rows.push(
        Row::new("full index (warm, all cores)", &name, ms(t))
            .target(t < Duration::from_secs(5), "< 5 s")
            .note(format!("{gbps:.2} GB/s, {} nodes", index.nodes().len())),
    );
    let ratio = index.memory_bytes() as f64 / doc.len() as f64;
    rows.push(
        Row::new(
            "index memory",
            &name,
            format!(
                "{:.1} % ({:.0} MB)",
                ratio * 100.0,
                index.memory_bytes() as f64 / 1e6
            ),
        )
        .target(ratio <= 0.25, "≤ 25 %"),
    );

    rows.push(reopen_from_cache(path, &name, opts.runs)?);
    rows.extend(searches(&doc, &name, &manifest, opts.runs)?);
    rows.extend(expand_root(&doc, &index, &name, &cancel)?);

    let peak = peak.finish();
    if peak > 0 {
        let before = rss_before.unwrap_or(0);
        rows.push(
            Row::new("peak RSS", &name, format!("{:.0} MB", peak as f64 / 1e6)).note(format!(
                "resident set incl. mapped file pages; {:.0} MB before this file",
                before as f64 / 1e6
            )),
        );
    }
    Ok(rows)
}

fn best_of(runs: usize, mut f: impl FnMut() -> io::Result<()>) -> io::Result<Duration> {
    let mut best = Duration::MAX;
    for _ in 0..runs.max(1) {
        let t = Instant::now();
        f()?;
        best = best.min(t.elapsed());
    }
    Ok(best)
}

/// What the app does before it shows the first tree rows of a file it has no index for.
fn first_content(path: &Path, cancel: &CancelToken) -> io::Result<Duration> {
    let t = Instant::now();
    let doc = Document::open(path).map_err(io_err)?;
    let preview = Preview::build(doc.bytes(), PREVIEW_BYTES, cancel).map_err(io_err)?;
    let cache = NavCache::new(64 << 20);
    let nav = preview.navigator(doc.bytes(), &cache);
    let mut tree = TreeModel::new(&nav, braceray_core::tree::DEFAULT_GROUP_SIZE);
    tree.expand(&nav, 0);
    let rows = tree.rows(&nav, 0..50);
    std::hint::black_box(rows);
    Ok(t.elapsed())
}

fn reopen_from_cache(path: &Path, name: &str, runs: usize) -> io::Result<Row> {
    log(&format!("{name}: cache"));
    let dir = std::env::temp_dir().join(format!("braceray-bench-cache-{}", std::process::id()));
    let cache = IndexCache::new(dir.clone(), CacheSettings::default());
    let (opts, cancel, progress) = (
        IndexOptions::default(),
        CancelToken::new(),
        Progress::default(),
    );
    let result = (|| {
        let doc = Document::open(path).map_err(io_err)?;
        let built = load_or_build(&doc, &opts, Some(&cache), &cancel, &progress).map_err(io_err)?;
        if let Some(w) = built.pending_write {
            w.join()
                .map_err(|_| io_err("cache write panicked"))?
                .map_err(io_err)?;
        }
        drop(doc);
        let mut hit = true;
        let t = best_of(runs.max(3), || {
            let doc = Document::open(path).map_err(io_err)?;
            let again =
                load_or_build(&doc, &opts, Some(&cache), &cancel, &progress).map_err(io_err)?;
            hit &= again.source == IndexSource::Cache;
            Ok(())
        })?;
        let row = Row::new("re-open from index cache (warm)", name, ms(t))
            .target(t < Duration::from_secs(1) && hit, "< 1 s")
            .note(format!(
                "`Document::open` + cache load incl. checksum; entry {:.0} MB",
                cache.total_bytes() as f64 / 1e6
            ));
        Ok(if hit { row } else { row.note("cache MISSED") })
    })();
    // Best effort: the directory is ours and only holds this run's entry.
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn searches(
    doc: &Arc<Document>,
    name: &str,
    manifest: &RecordsManifest,
    runs: usize,
) -> io::Result<Vec<Row>> {
    log(&format!("{name}: search"));
    let mut rows = Vec::new();
    for needle in manifest
        .needles
        .iter()
        .filter(|n| n.needle == "NEEDLE_RARE" || n.needle == "needle_common")
    {
        for case_sensitive in [true, false] {
            let q = SearchQuery {
                case_sensitive,
                ..SearchQuery::new(needle.needle.clone())
            };
            let expected = if case_sensitive {
                needle.count
            } else {
                needle.count_case_insensitive
            };
            let mut count = 0;
            let t = best_of(runs, || {
                let h = start_search(doc.clone(), &q, SearchOptions::default(), no_repaint())
                    .map_err(io_err)?;
                h.wait();
                count = h.count();
                Ok(())
            })?;
            let (label, limit, target) = if case_sensitive {
                ("plain search, exact count (warm)", 1.5, "< 1.5 s")
            } else {
                ("case-insensitive search (warm)", 3.0, "< 3 s")
            };
            let row = Row::new(
                &format!("{label}: `{}`", needle.needle),
                name,
                format!("{} ({count} matches)", ms(t)),
            )
            .target(t.as_secs_f64() < limit && count == expected, target);
            rows.push(if count == expected {
                row.note("count = manifest")
            } else {
                row.note(format!("COUNT MISMATCH: manifest says {expected}"))
            });
        }
    }
    // First stored result of a common word.
    let t = Instant::now();
    let h = start_search(
        doc.clone(),
        &SearchQuery::new("email"),
        SearchOptions::default(),
        no_repaint(),
    )
    .map_err(io_err)?;
    while h.stored() == 0 && h.is_running() {
        std::thread::yield_now();
    }
    let first = t.elapsed();
    h.cancel();
    h.wait();
    rows.push(
        Row::new("first search result (`email`)", name, ms(first))
            .target(first < Duration::from_millis(200), "< 200 ms")
            .note("core only, from the search start"),
    );
    Ok(rows)
}

fn expand_root(
    doc: &Document,
    index: &braceray_core::Index,
    name: &str,
    cancel: &CancelToken,
) -> io::Result<Vec<Row>> {
    log(&format!("{name}: expand root"));
    let nav = Navigator::new(doc.bytes(), index);
    let Some(root) = nav.root_container() else {
        return Ok(vec![Row::new(
            "expand root",
            name,
            "no container root".to_owned(),
        )]);
    };
    let t = Instant::now();
    let mut tree = TreeModel::new(&nav, braceray_core::tree::DEFAULT_GROUP_SIZE);
    tree.expand(&nav, 0);
    std::hint::black_box(tree.rows(&nav, 0..50));
    let expand = t.elapsed();
    let t = Instant::now();
    nav.prepare(&root, cancel).map_err(io_err)?;
    let prepare = t.elapsed();
    Ok(vec![
        Row::new("expand root + 50 rows", name, ms(expand))
            .target(expand < Duration::from_millis(200), "< 200 ms")
            .note(format!("{} rows after expand", tree.row_count())),
        Row::new("checkpoint table of the root (worker)", name, ms(prepare))
            .note("runs in the background in the app; rows show \"loading…\" meanwhile"),
    ])
}
