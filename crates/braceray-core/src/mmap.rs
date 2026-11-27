//! Read-only memory mapping. This is the only place where the file is mapped.

use std::fs::File;
use std::io;

use memmap2::Mmap;

/// A read-only memory map of a whole file.
#[derive(Debug)]
pub(crate) struct MappedFile {
    map: Mmap,
}

impl MappedFile {
    /// Maps `file` read-only. `file` must be a regular, non-empty file.
    pub(crate) fn map(file: &File) -> io::Result<Self> {
        // SAFETY: `Mmap::map` is unsafe because the bytes behind the returned slice can change
        // (or, after truncation on Unix, become unreadable and raise SIGBUS) if another process
        // modifies the file while it is mapped. Braceray never writes to the file itself and
        // only ever maps it read-only. The remaining risk comes from other processes; we
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
        Ok(Self { map })
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
    }
}
