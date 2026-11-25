//! Output file with progress reporting, and a block-parallel record writer.
//!
//! Records are generated in parallel blocks with rayon, but every record `i` is
//! a pure function of `(seed, i)`, and blocks are written in order, so the
//! output is byte-identical regardless of thread count.

use rayon::prelude::*;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Suppresses progress output (set by `--quiet` and by tests).
pub static QUIET: AtomicBool = AtomicBool::new(false);
/// Shows live percentages (off when several files are written in parallel).
pub static SHOW_PERCENT: AtomicBool = AtomicBool::new(true);

const BUF_SIZE: usize = 8 << 20;
const BLOCK_RECORDS: usize = 1024;

/// A fixture file being written, with progress and timing on stderr.
pub struct Out {
    w: BufWriter<File>,
    path: PathBuf,
    written: u64,
    total_hint: u64,
    next_report: u64,
    start: Instant,
}

impl Out {
    /// Creates `path` (and its parent directories). `total_hint` is the
    /// expected size, used only for progress percentages (0 = unknown).
    pub fn create(path: &Path, total_hint: u64) -> io::Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        Ok(Self {
            w: BufWriter::with_capacity(BUF_SIZE, File::create(path)?),
            path: path.to_path_buf(),
            written: 0,
            total_hint,
            next_report: total_hint / 10,
            start: Instant::now(),
        })
    }

    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.w.write_all(bytes)?;
        self.written += bytes.len() as u64;
        if self.total_hint > 0 && self.written >= self.next_report {
            self.next_report += self.total_hint / 10;
            if !QUIET.load(Ordering::Relaxed) && SHOW_PERCENT.load(Ordering::Relaxed) {
                let pct = (self.written * 100 / self.total_hint).min(100);
                eprint!("\r  {} … {pct:>3} %", self.path.display());
            }
        }
        Ok(())
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    /// Flushes and prints the elapsed time and throughput. Returns the size.
    pub fn finish(mut self) -> io::Result<u64> {
        self.w.flush()?;
        if !QUIET.load(Ordering::Relaxed) {
            let secs = self.start.elapsed().as_secs_f64();
            let mb = self.written as f64 / (1u64 << 20) as f64;
            eprintln!(
                "\r  {} — {mb:.1} MiB in {secs:.2} s ({:.0} MiB/s)",
                self.path.display(),
                mb / secs.max(1e-9)
            );
        }
        Ok(self.written)
    }
}

/// How a sequence of records is framed in the file.
pub struct Frame {
    pub open: &'static [u8],
    pub sep: &'static [u8],
    pub close: &'static [u8],
}

impl Frame {
    /// Top-level JSON array, pretty (records rendered at depth 1) or minified.
    pub fn array(pretty: bool) -> Self {
        if pretty {
            Self {
                open: b"[\n  ",
                sep: b",\n  ",
                close: b"\n]\n",
            }
        } else {
            Self {
                open: b"[",
                sep: b",",
                close: b"]",
            }
        }
    }

    /// One record per line; the generator writes the trailing `\n` itself.
    pub fn lines() -> Self {
        Self {
            open: b"",
            sep: b"",
            close: b"",
        }
    }
}

/// When to stop emitting records.
#[derive(Clone, Copy, Debug)]
pub enum Stop {
    /// Stop once the file reaches this many bytes (but never before `min_records`).
    Size { bytes: u64, min_records: u64 },
    /// Emit exactly this many records.
    Count(u64),
}

/// Writes records `0, 1, 2, …` produced by `make(i, buf)` until `stop`.
/// Returns the number of records written.
pub fn write_records<F>(out: &mut Out, frame: &Frame, stop: Stop, make: F) -> io::Result<u64>
where
    F: Fn(u64, &mut Vec<u8>) + Sync,
{
    let batch_blocks = rayon::current_num_threads().max(1) * 2;
    let mut n: u64 = 0;
    out.write(frame.open)?;
    'outer: loop {
        let first = n;
        let mut blocks_wanted = batch_blocks as u64;
        if let Stop::Count(c) = stop {
            let left = c - n;
            if left == 0 {
                break;
            }
            blocks_wanted = blocks_wanted.min(left.div_ceil(BLOCK_RECORDS as u64));
        }
        let blocks: Vec<(Vec<u8>, Vec<usize>)> = (0..blocks_wanted)
            .into_par_iter()
            .map(|b| {
                let mut buf = Vec::with_capacity(BLOCK_RECORDS * 1024);
                let mut ends = Vec::with_capacity(BLOCK_RECORDS);
                let base = first + b * BLOCK_RECORDS as u64;
                for i in base..base + BLOCK_RECORDS as u64 {
                    if let Stop::Count(c) = stop
                        && i >= c
                    {
                        break;
                    }
                    make(i, &mut buf);
                    ends.push(buf.len());
                }
                (buf, ends)
            })
            .collect();
        for (buf, ends) in &blocks {
            let mut start = 0;
            for &end in ends {
                match stop {
                    Stop::Count(c) if n >= c => break 'outer,
                    Stop::Size { bytes, min_records }
                        if n >= min_records.max(1)
                            && out.written() + frame.close.len() as u64 >= bytes =>
                    {
                        break 'outer;
                    }
                    _ => {}
                }
                if n > 0 {
                    out.write(frame.sep)?;
                }
                out.write(&buf[start..end])?;
                start = end;
                n += 1;
            }
        }
    }
    out.write(frame.close)?;
    Ok(n)
}

/// Mixes a seed and a record index into an independent per-record seed (splitmix64).
pub fn record_seed(seed: u64, i: u64) -> u64 {
    let mut z = seed ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(stop: Stop, frame: Frame) -> (Vec<u8>, u64) {
        QUIET.store(true, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("genfix-rec-{}", std::process::id()));
        let path = dir.join(format!("{stop:?}.txt").replace(['{', '}', ' ', ':'], "_"));
        let mut out = Out::create(&path, 0).unwrap();
        let n = write_records(&mut out, &frame, stop, |i, b| {
            b.extend_from_slice(i.to_string().as_bytes())
        })
        .unwrap();
        out.finish().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).ok();
        (bytes, n)
    }

    #[test]
    fn count_stop_crosses_blocks() {
        let (bytes, n) = run(Stop::Count(3000), Frame::array(false));
        assert_eq!(n, 3000);
        let s = String::from_utf8(bytes).unwrap();
        let expected: Vec<String> = (0..3000).map(|i| i.to_string()).collect();
        assert_eq!(s, format!("[{}]", expected.join(",")));
    }

    #[test]
    fn size_stop_respects_min_records() {
        let (bytes, n) = run(
            Stop::Size {
                bytes: 1,
                min_records: 5,
            },
            Frame::array(true),
        );
        assert_eq!(n, 5);
        assert_eq!(bytes, b"[\n  0,\n  1,\n  2,\n  3,\n  4\n]\n");
    }

    #[test]
    fn size_stop_reaches_target() {
        let (bytes, _) = run(
            Stop::Size {
                bytes: 100_000,
                min_records: 0,
            },
            Frame::array(false),
        );
        assert!(bytes.len() >= 100_000 && bytes.len() < 100_010);
    }
}
