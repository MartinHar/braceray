//! The `.brx` file format: a fixed header, the raw node table, the line table and a
//! checksum (step 05, architecture.md §6). All integers are little-endian.
//!
//! ```text
//! offset  size  field
//!      0     8  magic "BRXIDX\0\0"
//!      8     4  format_version
//!     12     4  flags (bit 0: invalid document, bit 1: adaptive mode)
//!     16    16  app_version (UTF-8, zero padded)
//!     32     8  file_size (with BOM)          ┐
//!     40     8  content_len (indexed bytes)   │ must equal the
//!     48    16  file_mtime (i128 ns since the Unix epoch, i128::MIN = unknown)
//!     64     8  content_hash (head + tail)    │ current file
//!     72     8  memory_budget                 ┘ and options
//!     80     8  node_count
//!     88     8  line_entries
//!     96     8  newlines
//!    104     4  adaptive_depth (u32::MAX = all depths stored)
//!    108     4  error found byte (0x100 = end of input)
//!    112    24  error offset, line, column
//!    136     4  error expected length
//!    140    64  error expected text
//!    204    52  zero padding (header = 256 bytes)
//!    256        [Node; node_count]        (24 bytes each, #[repr(C)])
//!               [u64; line_entries]       (line checkpoints)
//!               u64 checksum
//! ```
//! The checksum is xxh3-64 over the xxh3-64 of every 16 MiB block of the three sections
//! (header, nodes, lines), so it is computed on all cores and covers every byte.

use std::io::{self, Write};
use std::sync::Arc;

use rayon::prelude::*;
use xxhash_rust::xxh3::{Xxh3, xxh3_64};

use super::CacheError;
use crate::index::{Index, JsonError, LINE_BLOCK, LineTable, Node, NodeStore, Validation};
use crate::job::CancelToken;
use crate::mmap::MappedFile;

/// File magic.
pub(crate) const MAGIC: [u8; 8] = *b"BRXIDX\0\0";

/// Bump on every change of the layout or of the index semantics.
pub(crate) const FORMAT_VERSION: u32 = 1;

/// Header size; the node table starts here (8-byte aligned, a multiple of 64).
pub(crate) const HEADER_LEN: usize = 256;

const FLAG_INVALID: u32 = 1;
const FLAG_ADAPTIVE: u32 = 2;
const FOUND_EOF: u32 = 0x100;
const EXPECTED_MAX: usize = 64;
const CHECKSUM_BLOCK: usize = 16 << 20;

const _: () = assert!(140 + EXPECTED_MAX <= HEADER_LEN && HEADER_LEN.is_multiple_of(64));

/// What a cache entry must match to be used: the source file and the index options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub(crate) file_size: u64,
    pub(crate) content_len: u64,
    pub(crate) mtime_nanos: i128,
    pub(crate) content_hash: u64,
    pub(crate) memory_budget: u64,
}

/// Total size of the encoded entry for `index`.
pub(crate) fn encoded_len(index: &Index) -> u64 {
    (HEADER_LEN
        + std::mem::size_of_val(index.nodes())
        + std::mem::size_of_val(index.lines().checkpoints())
        + 8) as u64
}

/// The app version stored in the header: this crate's version, zero padded.
fn app_version() -> [u8; 16] {
    let mut out = [0u8; 16];
    let v = crate::version().as_bytes();
    let n = v.len().min(16);
    out[..n].copy_from_slice(&v[..n]);
    out
}

/// Encodes the header.
pub(crate) fn header(index: &Index, stamp: &Stamp) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    let mut flags = 0;
    let (mut found, mut err_off, mut line, mut col, mut expected) = (FOUND_EOF, 0, 0, 0, "");
    if let Validation::Invalid(e) = index.validation() {
        flags |= FLAG_INVALID;
        found = e.found.map_or(FOUND_EOF, u32::from);
        (err_off, line, col, expected) = (e.offset, e.line, e.column, e.expected);
    }
    if index.max_stored_depth().is_some() {
        flags |= FLAG_ADAPTIVE;
    }
    let expected = &expected.as_bytes()[..expected.len().min(EXPECTED_MAX)];

    h[0..8].copy_from_slice(&MAGIC);
    put_u32(&mut h, 8, FORMAT_VERSION);
    put_u32(&mut h, 12, flags);
    h[16..32].copy_from_slice(&app_version());
    put_u64(&mut h, 32, stamp.file_size);
    put_u64(&mut h, 40, stamp.content_len);
    h[48..64].copy_from_slice(&stamp.mtime_nanos.to_le_bytes());
    put_u64(&mut h, 64, stamp.content_hash);
    put_u64(&mut h, 72, stamp.memory_budget);
    put_u64(&mut h, 80, index.nodes().len() as u64);
    put_u64(&mut h, 88, index.lines().checkpoints().len() as u64);
    put_u64(&mut h, 96, index.lines().newlines());
    put_u32(&mut h, 104, index.max_stored_depth().unwrap_or(u32::MAX));
    put_u32(&mut h, 108, found);
    put_u64(&mut h, 112, err_off);
    put_u64(&mut h, 120, line);
    put_u64(&mut h, 128, col);
    put_u32(&mut h, 136, expected.len() as u32);
    h[140..140 + expected.len()].copy_from_slice(expected);
    h
}

/// Writes the whole entry for `index` to `out`.
pub(crate) fn write(out: &mut impl Write, index: &Index, stamp: &Stamp) -> io::Result<()> {
    let header = header(index, stamp);
    let nodes: &[u8] = bytemuck::cast_slice(index.nodes());
    let lines: &[u8] = bytemuck::cast_slice(index.lines().checkpoints());
    let sum = checksum(&[&header, nodes, lines], &CancelToken::new()).ok_or_else(|| {
        io::Error::other("checksum cancelled") // unreachable: the token is fresh
    })?;
    out.write_all(&header)?;
    out.write_all(nodes)?;
    out.write_all(lines)?;
    out.write_all(&sum.to_le_bytes())?;
    out.flush()
}

/// Validates the mapped entry against `stamp` and returns the index it holds. The node
/// table stays in the map (zero copy). `None` from the checksum only means cancelled.
pub(crate) fn read(
    map: Arc<MappedFile>,
    stamp: &Stamp,
    cancel: &CancelToken,
) -> Result<Index, CacheError> {
    let b = map.bytes();
    if b.len() < HEADER_LEN + 8 {
        return Err(CacheError::Corrupt("shorter than the header"));
    }
    let h = &b[..HEADER_LEN];
    if h[0..8] != MAGIC {
        return Err(CacheError::Corrupt("bad magic"));
    }
    let version = get_u32(h, 8);
    if version != FORMAT_VERSION {
        return Err(CacheError::FormatVersion(version));
    }
    if h[16..32] != app_version() {
        return Err(CacheError::AppVersion);
    }
    let found = Stamp {
        file_size: get_u64(h, 32),
        content_len: get_u64(h, 40),
        mtime_nanos: i128::from_le_bytes(h[48..64].try_into().unwrap_or_default()),
        content_hash: get_u64(h, 64),
        memory_budget: get_u64(h, 72),
    };
    if found != *stamp {
        return Err(CacheError::Stale);
    }

    let node_count = get_u64(h, 80);
    let line_entries = get_u64(h, 88);
    let nodes_len = node_count
        .checked_mul(std::mem::size_of::<Node>() as u64)
        .ok_or(CacheError::Corrupt("node count"))?;
    let lines_len = line_entries
        .checked_mul(8)
        .ok_or(CacheError::Corrupt("line count"))?;
    let total = (HEADER_LEN as u64 + 8)
        .checked_add(nodes_len)
        .and_then(|t| t.checked_add(lines_len))
        .ok_or(CacheError::Corrupt("sizes"))?;
    if total != b.len() as u64 {
        return Err(CacheError::Corrupt("file size doesn't match the header"));
    }
    let nodes_end = HEADER_LEN + nodes_len as usize;
    let lines_end = nodes_end + lines_len as usize;
    let stored_sum = get_u64(b, lines_end);
    let sum = checksum(
        &[h, &b[HEADER_LEN..nodes_end], &b[nodes_end..lines_end]],
        cancel,
    )
    .ok_or(CacheError::Cancelled)?;
    if sum != stored_sum {
        return Err(CacheError::Corrupt("checksum mismatch"));
    }

    // The bytes are exactly what we wrote; still check what navigation relies on.
    let expected_blocks = (stamp.content_len as usize).div_ceil(LINE_BLOCK).max(1);
    if line_entries as usize != expected_blocks {
        return Err(CacheError::Corrupt("line table length"));
    }
    let checkpoints: &[u64] = bytemuck::try_cast_slice(&b[nodes_end..lines_end])
        .map_err(|_| CacheError::Corrupt("line table alignment"))?;
    let lines = LineTable::from_parts(checkpoints.to_vec(), get_u64(h, 96))
        .ok_or(CacheError::Corrupt("line table values"))?;
    let flags = get_u32(h, 12);
    let max_stored_depth = (flags & FLAG_ADAPTIVE != 0).then(|| get_u32(h, 104));
    let validation = if flags & FLAG_INVALID == 0 {
        Validation::Valid
    } else {
        let len = (get_u32(h, 136) as usize).min(EXPECTED_MAX);
        let text = std::str::from_utf8(&h[140..140 + len])
            .map_err(|_| CacheError::Corrupt("error text"))?;
        let expected = crate::index::intern_expected(text).ok_or(CacheError::UnknownMessage)?;
        let found = match get_u32(h, 108) {
            FOUND_EOF => None,
            b => Some(u8::try_from(b).map_err(|_| CacheError::Corrupt("error byte"))?),
        };
        Validation::Invalid(JsonError {
            offset: get_u64(h, 112),
            line: get_u64(h, 120),
            column: get_u64(h, 128),
            expected,
            found,
        })
    };
    let nodes = NodeStore::mapped(map.clone(), HEADER_LEN..nodes_end)
        .ok_or(CacheError::Corrupt("node table alignment"))?;
    check_nodes(nodes.as_slice(), stamp.content_len)?;
    Ok(Index {
        nodes,
        lines,
        validation,
        max_stored_depth,
        len: stamp.content_len,
    })
}

/// Cheap structural checks that navigation relies on (offsets in range, parents before
/// children). Parallel; a few ms for 20 M nodes.
fn check_nodes(nodes: &[Node], len: u64) -> Result<(), CacheError> {
    let ok = nodes
        .par_iter()
        .enumerate()
        .with_min_len(1 << 16)
        .all(|(i, n)| {
            let parent_ok = if i == 0 {
                n.parent() == crate::index::NO_PARENT
            } else {
                (n.parent() as usize) < i
            };
            parent_ok && n.start() <= n.end() && n.end() <= len
        });
    if ok {
        Ok(())
    } else {
        Err(CacheError::Corrupt("node table values"))
    }
}

/// Block-parallel xxh3 over `parts`; `None` if cancelled.
fn checksum(parts: &[&[u8]], cancel: &CancelToken) -> Option<u64> {
    let blocks: Vec<&[u8]> = parts
        .iter()
        .flat_map(|p| {
            // An empty part still contributes one (empty) block, so section borders count.
            let n = p.len().div_ceil(CHECKSUM_BLOCK).max(1);
            (0..n).map(move |k| {
                &p[(k * CHECKSUM_BLOCK).min(p.len())..((k + 1) * CHECKSUM_BLOCK).min(p.len())]
            })
        })
        .collect();
    let digests: Option<Vec<u64>> = blocks
        .par_iter()
        .map(|blk| (!cancel.is_cancelled()).then(|| xxh3_64(blk)))
        .collect();
    let mut h = Xxh3::new();
    for d in digests? {
        h.update(&d.to_le_bytes());
    }
    Some(h.digest())
}

fn put_u32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_u64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn get_u32(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4)
        .and_then(|s| s.try_into().ok())
        .map_or(0, u32::from_le_bytes)
}

fn get_u64(b: &[u8], at: usize) -> u64 {
    b.get(at..at + 8)
        .and_then(|s| s.try_into().ok())
        .map_or(0, u64::from_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_sees_every_byte_and_section_borders() {
        let a = vec![7u8; 3 * CHECKSUM_BLOCK / 2];
        let c = CancelToken::new();
        let base = checksum(&[b"head", &a, b""], &c).unwrap();
        let mut b = a.clone();
        b[CHECKSUM_BLOCK + 5] ^= 1;
        assert_ne!(checksum(&[b"head", &b, b""], &c), Some(base));
        // Moving bytes between sections changes the sum.
        assert_ne!(checksum(&[b"hea", &a, b"d"], &c), Some(base));
        c.cancel();
        assert_eq!(checksum(&[b"head"], &c), None);
    }
}
