//! `all`: the whole fixture catalogue from `testing-strategy.md`, capped by `--max-size`.

use crate::orders::{RecordsOpts, generate_orders};
use crate::size::size_label;
use crate::{diff, encoding, invalid, logs, shapes};
use rayon::prelude::*;
use std::io;
use std::path::{Path, PathBuf};

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

/// One fixture job of the catalogue.
#[derive(Debug, Clone)]
pub enum Job {
    Orders { size: u64, minified: bool },
    Ndjson { size: u64 },
    Deep { depth: u64 },
    Wide { keys: u64 },
    Prim { count: u64 },
    Str { size: u64 },
    Invalid { big_size: u64 },
    Encoding,
    Diff { size: u64, changes: u64 },
}

/// The catalogue, with every size capped at `max_size`. Sizes that collapse
/// onto the same capped value are generated once.
pub fn catalogue(max_size: u64) -> Vec<Job> {
    let mut jobs = Vec::new();
    let mut order_sizes: Vec<u64> = [10 * MIB, 100 * MIB, GIB, 2 * GIB, 3 * GIB]
        .into_iter()
        .map(|s| s.min(max_size))
        .collect();
    order_sizes.dedup();
    for &size in &order_sizes {
        jobs.push(Job::Orders {
            size,
            minified: false,
        });
        jobs.push(Job::Orders {
            size,
            minified: true,
        });
    }
    let mut ndjson_sizes: Vec<u64> = [100 * MIB, GIB]
        .into_iter()
        .map(|s| s.min(max_size))
        .collect();
    ndjson_sizes.dedup();
    jobs.extend(ndjson_sizes.into_iter().map(|size| Job::Ndjson { size }));
    jobs.push(Job::Deep { depth: 100_000 });
    // Caps: ~50 bytes per key; ~10.7 bytes per number (10m numbers ≈ 107 MB).
    jobs.push(Job::Wide {
        keys: 1_000_000.min(max_size / 50),
    });
    jobs.push(Job::Prim {
        count: 10_000_000.min(max_size / 10),
    });
    jobs.push(Job::Str {
        size: (200 * MIB).min(max_size),
    });
    jobs.push(Job::Invalid {
        big_size: GIB.min(max_size),
    });
    jobs.push(Job::Encoding);
    jobs.push(Job::Diff {
        size: GIB.min(max_size),
        changes: 1000,
    });
    jobs
}

/// Runs one job inside `out_dir` (file names follow the catalogue).
pub fn run_job(job: &Job, out_dir: &Path, seed: u64) -> io::Result<()> {
    let p = |name: String| -> PathBuf { out_dir.join(name) };
    match *job {
        Job::Orders { size, minified } => {
            let name = if minified {
                format!("orders_min_{}.json", size_label(size))
            } else {
                format!("orders_{}.json", size_label(size))
            };
            generate_orders(
                &p(name),
                RecordsOpts {
                    size,
                    seed,
                    minified,
                },
            )?;
        }
        Job::Ndjson { size } => {
            logs::generate_ndjson(
                &p(format!("logs_{}.ndjson", size_label(size))),
                RecordsOpts {
                    size,
                    seed,
                    minified: true,
                },
            )?;
        }
        Job::Deep { depth } => {
            shapes::generate_deep(&p(format!("deep_{}.json", count_label(depth))), depth)?;
        }
        Job::Wide { keys } => {
            shapes::generate_wide(
                &p(format!("wide_{}.json", count_label(keys))),
                keys,
                seed,
                false,
            )?;
        }
        Job::Prim { count } => {
            shapes::generate_prim(
                &p(format!("prim_array_{}.json", count_label(count))),
                count,
                seed,
                false,
            )?;
        }
        Job::Str { size } => {
            shapes::generate_string(&p("huge_string.json".to_owned()), size, seed)?;
        }
        Job::Invalid { big_size } => {
            invalid::generate_invalid(&out_dir.join("invalid"), big_size, seed)?;
        }
        Job::Encoding => {
            encoding::generate_encoding(&out_dir.join("encoding"), seed)?;
        }
        Job::Diff { size, changes } => {
            diff::generate_diff(&out_dir.join("diff"), size, changes, seed)?;
        }
    }
    Ok(())
}

/// Generates the whole catalogue. Jobs run in parallel; each one is still
/// deterministic on its own.
pub fn generate_all(out_dir: &Path, max_size: u64, seed: u64) -> io::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    crate::records::SHOW_PERCENT.store(false, std::sync::atomic::Ordering::Relaxed);
    catalogue(max_size)
        .par_iter()
        .try_for_each(|job| run_job(job, out_dir, seed))
}

/// `100k`, `1m`, `10m` for counts (decimal), else the plain number.
pub fn count_label(n: u64) -> String {
    match n {
        n if n >= 1_000_000 && n % 1_000_000 == 0 => format!("{}m", n / 1_000_000),
        n if n >= 1_000 && n % 1_000 == 0 => format!("{}k", n / 1_000),
        n => n.to_string(),
    }
}
