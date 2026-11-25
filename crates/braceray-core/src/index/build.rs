//! The parallel index builder (architecture.md §3.2).
//!
//! 1. Phase A, parallel: per-chunk summaries (quote parity, escape carry).
//! 2. Phase B, sequential: every chunk's real start state.
//! 3. Phase C, parallel in batches: SIMD tokenization with local validation.
//! 4. Consumer, sequential and overlapped with the next batch: the reference state
//!    machine runs over the tokens ([`Parser::fast_token`]); any token it can't take
//!    goes through the byte-level [`Parser::step`], so the result equals
//!    [`scalar::build`](super::scalar::build) exactly, errors included.

use rayon::prelude::*;

use super::chunk::{self, MAX_CHUNK};
use super::lines::{LINE_BLOCK, LineTable};
use super::node::{MAX_OFFSET, Node};
use super::parser::{Parser, Stop};
use super::simd::{self, Simd};
use super::{Index, parser::Fail};
use crate::document::Document;
use crate::error::Error;
use crate::job::{CancelToken, Progress};

/// Default chunk size of the parallel build.
pub const DEFAULT_CHUNK_SIZE: usize = 8 << 20;

/// Tokens between cancellation checks in the consumer.
const CANCEL_TOKENS: usize = 1 << 16;

/// Smallest automatic memory budget, so small files never use adaptive mode.
const MIN_AUTO_BUDGET: u64 = 16 << 20;

/// Options of [`build_index`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexOptions {
    /// Bytes per parallel work item (clamped to 64 B ..= 256 MiB). Small values are for
    /// tests: they make strings, escapes and tokens cross chunk borders.
    pub chunk_size: usize,
    /// Most bytes the node table may use before adaptive mode stores only shallow
    /// containers. `0` = automatic: 25 % of the file size, at least 16 MiB.
    pub memory_budget: u64,
    /// Worker threads; `None` uses rayon's global pool (all cores).
    pub threads: Option<usize>,
}

impl Default for IndexOptions {
    fn default() -> Self {
        IndexOptions {
            chunk_size: DEFAULT_CHUNK_SIZE,
            memory_budget: 0,
            threads: None,
        }
    }
}

impl IndexOptions {
    fn budget_for(&self, len: u64) -> u64 {
        if self.memory_budget == 0 {
            (len / 4).max(MIN_AUTO_BUDGET)
        } else {
            self.memory_budget
        }
    }
}

/// Builds the structural index of `doc` on all cores.
///
/// Errors only for cancellation ([`Error::Cancelled`]) or a file over 1 TiB; invalid JSON
/// gives an index whose [`Index::validation`] is `Invalid` (with the valid prefix
/// indexed). Progress goes from 0 to 2 × the length (two passes).
///
/// # Example
/// ```
/// use braceray_core::index::{build_index, IndexOptions};
/// use braceray_core::{CancelToken, Document, Progress};
///
/// let dir = tempfile::tempdir()?;
/// let path = dir.path().join("a.json");
/// std::fs::write(&path, br#"{"users": [{"id": 1}, {"id": 2}]}"#)?;
/// let doc = Document::open(&path)?;
///
/// let index = build_index(&doc, &IndexOptions::default(), &CancelToken::new(), &Progress::default())?;
/// assert!(index.is_valid());
/// assert_eq!(index.nodes().len(), 4);
/// assert_eq!(index.nodes()[1].child_count(), 2);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn build_index(
    doc: &Document,
    opts: &IndexOptions,
    cancel: &CancelToken,
    progress: &Progress,
) -> Result<Index, Error> {
    doc.advise_sequential();
    let result = build_bytes(doc.bytes(), opts, cancel, progress);
    doc.advise_random();
    result
}

/// [`build_index`] on a byte slice (tests, fuzzing, benchmarks).
pub fn build_bytes(
    bytes: &[u8],
    opts: &IndexOptions,
    cancel: &CancelToken,
    progress: &Progress,
) -> Result<Index, Error> {
    if bytes.len() as u64 > MAX_OFFSET {
        return Err(Error::FileTooLarge);
    }
    let pool = opts.threads.and_then(|n| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n.max(1))
            .build()
            .ok()
    });
    let run = || build_in_pool(simd::best(), bytes, opts, cancel, progress);
    match pool {
        Some(pool) => pool.install(run),
        None => run(),
    }
}

/// The builder with an explicit classifier (equivalence tests of every implementation).
#[doc(hidden)]
pub fn build_bytes_with(
    simd: Simd,
    bytes: &[u8],
    opts: &IndexOptions,
    cancel: &CancelToken,
    progress: &Progress,
) -> Result<Index, Error> {
    if bytes.len() as u64 > MAX_OFFSET {
        return Err(Error::FileTooLarge);
    }
    build_in_pool(simd, bytes, opts, cancel, progress)
}

fn build_in_pool(
    simd: Simd,
    bytes: &[u8],
    opts: &IndexOptions,
    cancel: &CancelToken,
    progress: &Progress,
) -> Result<Index, Error> {
    progress.reset(2 * bytes.len() as u64);
    let chunk_size = opts.chunk_size.clamp(64, MAX_CHUNK);
    let ranges: Vec<(usize, usize)> = (0..bytes.len())
        .step_by(chunk_size)
        .map(|s| (s, (s + chunk_size).min(bytes.len())))
        .collect();

    // Phase A + line table.
    let (summaries, block_newlines) = rayon::join(
        || {
            ranges
                .par_iter()
                .map(|&(s, e)| chunk::summarize(&simd, &bytes[s..e], cancel, progress))
                .collect::<Option<Vec<_>>>()
        },
        || {
            bytes
                .par_chunks(LINE_BLOCK)
                .map(|b| {
                    (!cancel.is_cancelled()).then(|| memchr::memchr_iter(b'\n', b).count() as u64)
                })
                .collect::<Option<Vec<_>>>()
        },
    );
    let summaries = summaries.ok_or(Error::Cancelled)?;
    let lines = LineTable::from_block_counts(block_newlines.ok_or(Error::Cancelled)?);

    // Phase B.
    let states = chunk::start_states(&summaries);

    // Phase C + consumer, pipelined batch by batch.
    let node_cap =
        (opts.budget_for(bytes.len() as u64) / std::mem::size_of::<Node>() as u64).max(1) as usize;
    // Progress is reported by the workers (bytes tokenized), not by the parser.
    let mut parser = Parser::new(bytes, None, cancel, None);
    let batch = (rayon::current_num_threads() * 2).max(2);
    let produce = |from: usize| -> Option<Vec<Vec<u32>>> {
        let to = (from + batch).min(ranges.len());
        (from..to)
            .into_par_iter()
            .map(|k| {
                let (s, e) = ranges[k];
                let mut out = Vec::with_capacity((e - s) / 8);
                let ok = chunk::tokenize(&simd, bytes, s, e, states[k], &mut out, cancel);
                progress.add((e - s) as u64);
                ok.then_some(out)
            })
            .collect()
    };

    let mut outcome: Result<(), Stop> = Ok(());
    let mut pending = (!ranges.is_empty())
        .then(|| produce(0))
        .flatten()
        .map(|t| (0, t));
    if pending.is_none() && !ranges.is_empty() {
        return Err(Error::Cancelled);
    }
    while let Some((from, tokens)) = pending.take() {
        let next = from + batch;
        let (consumed, produced) = rayon::join(
            || consume(&mut parser, &ranges[from..], &tokens, node_cap, cancel),
            || (next < ranges.len()).then(|| produce(next)),
        );
        if let Err(stop) = consumed {
            outcome = Err(stop);
            break;
        }
        match produced {
            Some(Some(t)) => pending = Some((next, t)),
            Some(None) => return Err(Error::Cancelled),
            None => {}
        }
    }
    if outcome.is_ok() && parser.stepped {
        outcome = parser.catch_up(bytes.len());
    }
    if outcome.is_ok() {
        outcome = parser.finish_input();
    }
    let fail: Option<Fail> = match outcome {
        Ok(()) => None,
        Err(Stop::Invalid(f)) => Some(f),
        Err(Stop::Cancelled) => return Err(Error::Cancelled),
        Err(Stop::TooManyNodes) => return Err(Error::FileTooLarge),
    };
    Ok(parser.finish(fail, lines))
}

/// Runs the parser over one batch of token lists (chunk `k` of the batch starts at
/// `ranges[k].0`).
fn consume(
    parser: &mut Parser<'_>,
    ranges: &[(usize, usize)],
    batch: &[Vec<u32>],
    node_cap: usize,
    cancel: &CancelToken,
) -> Result<(), Stop> {
    for (tokens, &(start, _)) in batch.iter().zip(ranges) {
        if cancel.is_cancelled() {
            return Err(Stop::Cancelled);
        }
        for (n, &t) in tokens.iter().enumerate() {
            if n % CANCEL_TOKENS == CANCEL_TOKENS - 1 && cancel.is_cancelled() {
                return Err(Stop::Cancelled);
            }
            let (tok, rel) = chunk::decode(t);
            let offset = start + rel;
            if parser.stepped {
                parser.catch_up(offset)?;
            }
            if offset < parser.i {
                continue; // already read by the reference parser
            }
            let handled = match tok {
                Some(tok) => parser.fast_token(tok, offset)?,
                None => false,
            };
            if !handled {
                parser.i = offset;
                parser.step()?;
                parser.stepped = true;
            }
        }
        if parser.node_count() > node_cap {
            parser.compact(node_cap / 2);
        }
    }
    Ok(())
}
