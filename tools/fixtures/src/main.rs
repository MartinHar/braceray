//! `gen-fixtures`: deterministic test-data generator. See `tools/fixtures/README.md`.

use clap::{Args, Parser, Subcommand};
use gen_fixtures::orders::RecordsOpts;
use gen_fixtures::size::parse_size;
use gen_fixtures::{
    DEFAULT_SEED, all, bench_big, diff, encoding, invalid, logs, orders, records, shapes,
};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "gen-fixtures",
    version,
    about = "Deterministic JSON test fixtures for Braceray"
)]
struct Cli {
    /// Don't print progress.
    #[arg(long, short, global = true)]
    quiet: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args)]
struct Common {
    /// Output file (or directory for multi-file kinds).
    #[arg(long)]
    out: PathBuf,
    /// PRNG seed. Same seed ⇒ byte-identical output.
    #[arg(long, default_value_t = DEFAULT_SEED)]
    seed: u64,
}

#[derive(Subcommand)]
enum Cmd {
    /// The whole catalogue into a directory.
    All {
        #[arg(long)]
        out: PathBuf,
        /// Cap for every fixture size (e.g. 100m). Default: 3g (full catalogue).
        #[arg(long, default_value = "3g", value_parser = parse_size)]
        max_size: u64,
        #[arg(long, default_value_t = DEFAULT_SEED)]
        seed: u64,
    },
    /// Top-level array of realistic order objects (+ needle manifest).
    Orders {
        #[command(flatten)]
        c: Common,
        #[arg(long, value_parser = parse_size)]
        size: u64,
        /// Single line instead of 2-space pretty printing.
        #[arg(long)]
        minified: bool,
    },
    /// JSON Lines log records (+ needle manifest).
    Ndjson {
        #[command(flatten)]
        c: Common,
        #[arg(long, value_parser = parse_size)]
        size: u64,
    },
    /// Nesting depth stress test.
    Deep {
        #[command(flatten)]
        c: Common,
        #[arg(long, default_value_t = 100_000)]
        depth: u64,
    },
    /// One object with many keys.
    Wide {
        #[command(flatten)]
        c: Common,
        #[arg(long, default_value_t = 1_000_000)]
        keys: u64,
        #[arg(long)]
        minified: bool,
    },
    /// Array of numbers.
    Prim {
        #[command(flatten)]
        c: Common,
        #[arg(long, default_value_t = 10_000_000)]
        count: u64,
        #[arg(long)]
        minified: bool,
    },
    /// One huge string value.
    String {
        #[command(flatten)]
        c: Common,
        #[arg(long, value_parser = parse_size)]
        size: u64,
    },
    /// Malformed files + invalid_manifest.json (into a directory).
    Invalid {
        #[command(flatten)]
        c: Common,
        /// Size of the "error at the very end" file (0 = skip).
        #[arg(long, default_value = "1g", value_parser = parse_size)]
        big_size: u64,
    },
    /// BOM / UTF-16 / UTF-32 / empty / binary files (into a directory).
    Encoding {
        #[command(flatten)]
        c: Common,
    },
    /// Two files with known changes + diff_manifest.json (into a directory).
    Diff {
        #[command(flatten)]
        c: Common,
        #[arg(long, value_parser = parse_size)]
        size: u64,
        #[arg(long, default_value_t = 1000)]
        changes: u64,
    },
    /// Measure the performance targets on the big fixtures (generated when missing) and
    /// print Markdown rows for docs/PERF.md. Build with --release.
    BenchBig {
        /// Fixture directory.
        #[arg(long, default_value = "fixtures-data")]
        dir: PathBuf,
        /// Comma-separated `orders` fixtures (`_min` in the name ⇒ minified).
        #[arg(
            long,
            default_value = "orders_2g.json,orders_min_2g.json",
            value_delimiter = ','
        )]
        files: Vec<String>,
        /// Size of fixtures it has to generate.
        #[arg(long, default_value = "2g", value_parser = parse_size)]
        size: u64,
        /// Repetitions of each warm measurement (the best is reported).
        #[arg(long, default_value_t = 3)]
        runs: usize,
        /// Also measure cold (drops the OS page cache: needs root, e.g. `sudo purge`).
        #[arg(long)]
        cold: bool,
        /// Machine ID for the tables (see docs/PERF.md → Machines).
        #[arg(long, default_value = "?")]
        machine: String,
        /// Step column of the rows.
        #[arg(long, default_value = "12")]
        step: String,
    },
}

fn run(cmd: Cmd) -> std::io::Result<()> {
    match cmd {
        Cmd::All {
            out,
            max_size,
            seed,
        } => all::generate_all(&out, max_size, seed),
        Cmd::Orders { c, size, minified } => orders::generate_orders(
            &c.out,
            RecordsOpts {
                size,
                seed: c.seed,
                minified,
            },
        )
        .map(drop),
        Cmd::Ndjson { c, size } => logs::generate_ndjson(
            &c.out,
            RecordsOpts {
                size,
                seed: c.seed,
                minified: true,
            },
        )
        .map(drop),
        Cmd::Deep { c, depth } => shapes::generate_deep(&c.out, depth).map(drop),
        Cmd::Wide { c, keys, minified } => {
            shapes::generate_wide(&c.out, keys, c.seed, minified).map(drop)
        }
        Cmd::Prim { c, count, minified } => {
            shapes::generate_prim(&c.out, count, c.seed, minified).map(drop)
        }
        Cmd::String { c, size } => shapes::generate_string(&c.out, size, c.seed).map(drop),
        Cmd::Invalid { c, big_size } => {
            invalid::generate_invalid(&c.out, big_size, c.seed).map(drop)
        }
        Cmd::Encoding { c } => encoding::generate_encoding(&c.out, c.seed).map(drop),
        Cmd::Diff { c, size, changes } => {
            diff::generate_diff(&c.out, size, changes, c.seed).map(drop)
        }
        Cmd::BenchBig {
            dir,
            files,
            size,
            runs,
            cold,
            machine,
            step,
        } => {
            let report = bench_big::run(&bench_big::BenchOpts {
                dir,
                files,
                size,
                runs,
                cold,
            })?;
            println!("{}", report.markdown(&machine, &step));
            if report.all_met() {
                Ok(())
            } else {
                Err(std::io::Error::other("some targets were missed (❌ rows)"))
            }
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    records::QUIET.store(cli.quiet, Ordering::Relaxed);
    let start = Instant::now();
    match run(cli.cmd) {
        Ok(()) => {
            if !cli.quiet {
                eprintln!("done in {:.2} s", start.elapsed().as_secs_f64());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("gen-fixtures: {e}");
            ExitCode::FAILURE
        }
    }
}
