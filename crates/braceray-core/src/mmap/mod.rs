//! Read-only memory mapping. This is the only place where the file is mapped.

use std::fs::File;
use std::io;

use memmap2::Mmap;

#[cfg(unix)]
mod guard;

/// A read-only memory map of a whole file.
#[derive(Debug)]
pub(crate) struct MappedFile {
    /// SIGBUS guard (Unix). Declared before `map` so it unregisters before the unmap.
    #[cfg(unix)]
    guard: Option<guard::Guard>,
    map: Mmap,
}

impl MappedFile {
    /// Maps `file` read-only. `file` must be a regular, non-empty file.
    pub(crate) fn map(file: &File) -> io::Result<Self> {
        // SAFETY: `Mmap::map` is unsafe because the bytes behind the returned slice can change
        // (or, after truncation on Unix, become unreadable and raise SIGBUS) if another process
        // modifies the file while it is mapped. Braceray never writes to the file itself and
        // only ever maps it read-only. The remaining risk comes from other processes. On Unix
        // the mapping is registered with the SIGBUS guard (`guard.rs`, ADR 028), which turns a
        // read past a truncated end into zero pages instead of a crash. Beyond that we
        // mitigate it as described in architecture.md §9: `Document` records size and mtime at
        // open time, the app checks `Document::has_changed_on_disk` on focus and every few
        // seconds, and long jobs re-check the length before they start. Covered by every
        // `Document::open` test (e.g. `tests/document.rs`).
        // Index cache entries (`cache/`) are mapped the same way. Braceray never modifies an
        // entry in place: writers create a new temp file and rename it over the old entry,
        // and eviction deletes, so an existing mapping keeps its bytes. The entry's
        // contents are untrusted until `cache::format::read` has checked sizes, alignment
        // and the checksum (`tests/cache.rs` covers truncated and corrupted entries).
        let map = unsafe { Mmap::map(file)? };
        Ok(Self {
            #[cfg(unix)]
            guard: guard::Guard::register(map.as_ptr(), map.len()),
            map,
        })
    }

    /// `true` once a read hit a page the file no longer backs (it was truncated, or the
    /// volume reported an I/O error) and saw zeros instead. Always `false` on Windows, where
    /// a mapped file can't be truncated.
    pub(crate) fn faulted(&self) -> bool {
        #[cfg(unix)]
        {
            self.guard.as_ref().is_some_and(guard::Guard::faulted)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// The mapped bytes.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.map
    }

    /// Hints the OS that the mapping will be read front to back (read-ahead). No-op where
    /// unsupported. Advice is only a hint, so failures are ignored.
    pub(crate) fn advise_sequential(&self) {
        #[cfg(unix)]
        {
            let _ = self.map.advise(memmap2::Advice::Sequential);
        }
    }

    /// Hints the OS that the mapping will be read in random order (no read-ahead). No-op where
    /// unsupported. Advice is only a hint, so failures are ignored.
    pub(crate) fn advise_random(&self) {
        #[cfg(unix)]
        {
            let _ = self.map.advise(memmap2::Advice::Random);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn maps_the_file_bytes() {
        let mut f = tempfile::tempfile().unwrap();
        f.write_all(b"[1,2,3]").unwrap();
        let m = MappedFile::map(&f).unwrap();
        assert_eq!(m.bytes(), b"[1,2,3]");
        m.advise_sequential();
        m.advise_random();
        assert_eq!(m.bytes(), b"[1,2,3]");
        assert!(!m.faulted());
    }

    /// Truncating a mapped file and reading past the new end gives zeros, not SIGBUS.
    /// Covers the `unsafe` blocks in `guard.rs` (handler install, `mmap` over the range).
    #[cfg(unix)]
    #[test]
    fn read_past_truncation_is_zeros_not_sigbus() {
        const LEN: usize = 64 * 1024;
        let mut f = tempfile::tempfile().unwrap();
        f.write_all(&[b'x'; LEN]).unwrap();
        let m = MappedFile::map(&f).unwrap();
        assert_eq!(m.bytes()[LEN - 1], b'x');
        // Evict nothing, just cut the file: the last pages lose their backing.
        f.set_len(100).unwrap();
        let b = m.bytes();
        assert_eq!(std::hint::black_box(b[LEN - 1]), 0);
        assert!(m.faulted());
        assert_eq!(b[50], b'x', "pages before the fault keep their content");
        // A scan over the whole range works too.
        assert!(b[100..].iter().all(|&c| c == 0 || c == b'x'));
        drop(m);
        // A new mapping of the truncated file starts unfaulted.
        let m2 = MappedFile::map(&f).unwrap();
        assert_eq!(m2.bytes().len(), 100);
        assert!(!m2.faulted());
    }
}
