//! Line table: newline counts at fixed checkpoints, for O(64 KiB) line/column lookups.

/// Distance between line-table checkpoints.
pub const LINE_BLOCK: usize = 64 * 1024;

/// `checkpoints[k]` = number of `\n` bytes in `bytes[..k * LINE_BLOCK]`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineTable {
    checkpoints: Vec<u64>,
    /// Total number of `\n` bytes in the document.
    newlines: u64,
}

impl LineTable {
    /// Builds the table with a single pass (memchr-accelerated).
    pub fn build(bytes: &[u8]) -> Self {
        let mut checkpoints = Vec::with_capacity(bytes.len() / LINE_BLOCK + 1);
        let mut newlines = 0u64;
        for block in bytes.chunks(LINE_BLOCK) {
            checkpoints.push(newlines);
            newlines += count_newlines(block);
        }
        if checkpoints.is_empty() {
            checkpoints.push(0);
        }
        Self {
            checkpoints,
            newlines,
        }
    }

    /// Builds a table from per-block newline counts (block `k` covers
    /// `k * LINE_BLOCK .. (k + 1) * LINE_BLOCK`).
    pub(crate) fn from_block_counts(counts: impl IntoIterator<Item = u64>) -> Self {
        let mut checkpoints = Vec::new();
        let mut newlines = 0u64;
        for c in counts {
            checkpoints.push(newlines);
            newlines += c;
        }
        if checkpoints.is_empty() {
            checkpoints.push(0);
        }
        Self {
            checkpoints,
            newlines,
        }
    }

    /// Number of lines (newlines + 1).
    pub fn line_count(&self) -> u64 {
        self.newlines + 1
    }

    /// Heap bytes used.
    pub fn memory_bytes(&self) -> usize {
        self.checkpoints.capacity() * std::mem::size_of::<u64>()
    }

    /// 1-based line of `offset` (an offset equal to the length is allowed).
    pub fn line_of(&self, bytes: &[u8], offset: u64) -> u64 {
        let off = (offset as usize).min(bytes.len());
        let k = (off / LINE_BLOCK).min(self.checkpoints.len() - 1);
        let base = k * LINE_BLOCK;
        1 + self.checkpoints[k] + count_newlines(&bytes[base..off])
    }

    /// 1-based `(line, column)` of `offset`. The column counts **characters** (UTF-8 lead
    /// bytes), as an editor shows it. Cost: one scan back to the line start.
    pub fn line_col(&self, bytes: &[u8], offset: u64) -> (u64, u64) {
        let off = (offset as usize).min(bytes.len());
        let line_start = line_start(bytes, off);
        let chars = bytes[line_start..off]
            .iter()
            .filter(|&&b| b & 0xC0 != 0x80)
            .count() as u64;
        (self.line_of(bytes, off as u64), chars + 1)
    }
}

/// Offset of the first byte of the line containing `off`.
pub(crate) fn line_start(bytes: &[u8], off: usize) -> usize {
    memchr::memrchr(b'\n', &bytes[..off]).map_or(0, |p| p + 1)
}

fn count_newlines(bytes: &[u8]) -> u64 {
    memchr::memchr_iter(b'\n', bytes).count() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small() {
        let text = "ab\ncd\n\nÅrø x";
        let b = text.as_bytes();
        let t = LineTable::build(b);
        assert_eq!(t.line_count(), 4);
        assert_eq!(t.line_col(b, 0), (1, 1));
        assert_eq!(t.line_col(b, 2), (1, 3)); // the '\n' itself
        assert_eq!(t.line_col(b, 3), (2, 1));
        assert_eq!(t.line_col(b, 6), (3, 1));
        let x = text.find('x').unwrap() as u64;
        assert_eq!(t.line_col(b, x), (4, 5)); // "Årø " = 4 chars, 6 bytes
        assert_eq!(t.line_col(b, b.len() as u64), (4, 6));
    }

    #[test]
    fn across_blocks() {
        let mut b = vec![b' '; 3 * LINE_BLOCK + 10];
        for i in (0..b.len()).step_by(1000) {
            b[i] = b'\n';
        }
        let t = LineTable::build(&b);
        assert_eq!(
            LineTable::from_block_counts(b.chunks(LINE_BLOCK).map(count_newlines)),
            t
        );
        for off in [
            0,
            999,
            1000,
            1001,
            LINE_BLOCK,
            LINE_BLOCK + 1,
            b.len() - 1,
            b.len(),
        ] {
            let naive = 1 + b[..off].iter().filter(|&&c| c == b'\n').count() as u64;
            assert_eq!(t.line_of(&b, off as u64), naive, "offset {off}");
        }
    }

    #[test]
    fn empty() {
        let t = LineTable::build(b"");
        assert_eq!(t.line_count(), 1);
        assert_eq!(t.line_col(b"", 0), (1, 1));
    }
}
