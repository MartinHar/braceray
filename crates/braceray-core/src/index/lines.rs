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

    /// Rebuilds a table from its parts (index cache). `None` unless the checkpoints
    /// start at 0, never decrease and stay ≤ `newlines`.
    pub(crate) fn from_parts(checkpoints: Vec<u64>, newlines: u64) -> Option<Self> {
        let ordered = checkpoints.windows(2).all(|w| w[0] <= w[1]);
        let bounded = checkpoints.last().is_some_and(|&last| last <= newlines);
        (checkpoints.first() == Some(&0) && ordered && bounded).then_some(Self {
            checkpoints,
            newlines,
        })
    }

    /// The checkpoints (index cache).
    pub(crate) fn checkpoints(&self) -> &[u64] {
        &self.checkpoints
    }

    /// Total number of `\n` bytes.
    pub(crate) fn newlines(&self) -> u64 {
        self.newlines
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
    /// bytes), as an editor shows it. Cost: finding the line start is O(log n + 64 KiB);
    /// counting the characters is proportional to the column (use
    /// [`position`](Self::position) to bound it on very long lines).
    pub fn line_col(&self, bytes: &[u8], offset: u64) -> (u64, u64) {
        let off = (offset as usize).min(bytes.len());
        let start = self.line_start(bytes, off as u64) as usize;
        (
            self.line_of(bytes, off as u64),
            count_chars(&bytes[start..off]) + 1,
        )
    }

    /// Line and column of `offset`, counting at most `max_chars_scan` bytes for the
    /// column: further into a line the column is given in bytes instead.
    ///
    /// # Example
    /// ```
    /// use braceray_core::index::LineTable;
    /// let bytes = "ab\nçd".as_bytes();
    /// let t = LineTable::build(bytes);
    /// let p = t.position(bytes, 5, 1 << 20); // the `d`
    /// assert_eq!((p.line, p.column, p.column_in_bytes), (2, 2, false));
    /// let p = t.position(bytes, 5, 1);
    /// assert_eq!((p.line, p.column, p.column_in_bytes), (2, 3, true)); // "ç" is 2 bytes
    /// ```
    pub fn position(&self, bytes: &[u8], offset: u64, max_chars_scan: usize) -> TextPos {
        let off = (offset as usize).min(bytes.len());
        let start = self.line_start(bytes, off as u64) as usize;
        let line = self.line_of(bytes, off as u64);
        if off - start <= max_chars_scan {
            TextPos {
                line,
                column: count_chars(&bytes[start..off]) + 1,
                column_in_bytes: false,
            }
        } else {
            TextPos {
                line,
                column: (off - start) as u64 + 1,
                column_in_bytes: true,
            }
        }
    }

    /// Offset of the first byte of the line containing `offset`, in O(log n + 64 KiB):
    /// the checkpoints find the block holding the previous newline, so a single 2 GB line
    /// is never scanned backwards.
    ///
    /// # Example
    /// ```
    /// use braceray_core::index::LineTable;
    /// let bytes = b"ab\ncd\n";
    /// let t = LineTable::build(bytes);
    /// assert_eq!(t.line_start(bytes, 4), 3);
    /// assert_eq!(t.line_start(bytes, 1), 0);
    /// ```
    pub fn line_start(&self, bytes: &[u8], offset: u64) -> u64 {
        let off = (offset as usize).min(bytes.len());
        let k = (off / LINE_BLOCK).min(self.checkpoints.len() - 1);
        let base = k * LINE_BLOCK;
        if let Some(p) = memchr::memrchr(b'\n', &bytes[base..off]) {
            return (base + p + 1) as u64;
        }
        let before = self.checkpoints[k];
        if before == 0 {
            return 0;
        }
        // Largest block j < k that ends with fewer newlines before it: the last newline
        // before `base` lies in block j (blocks j+1..k have none).
        let j = self.checkpoints[..k].partition_point(|&c| c < before) - 1;
        let block = &bytes[j * LINE_BLOCK..((j + 1) * LINE_BLOCK).min(bytes.len())];
        match memchr::memrchr(b'\n', block) {
            Some(p) => (j * LINE_BLOCK + p + 1) as u64,
            None => 0, // only with a table that doesn't belong to `bytes`
        }
    }

    /// Offset of the first byte of 1-based `line`, or `None` past the last line.
    ///
    /// # Example
    /// ```
    /// use braceray_core::index::LineTable;
    /// let bytes = b"ab\ncd\n";
    /// let t = LineTable::build(bytes);
    /// assert_eq!(t.line_offset(bytes, 1), Some(0));
    /// assert_eq!(t.line_offset(bytes, 2), Some(3));
    /// assert_eq!(t.line_offset(bytes, 3), Some(6)); // the empty last line
    /// assert_eq!(t.line_offset(bytes, 4), None);
    /// ```
    pub fn line_offset(&self, bytes: &[u8], line: u64) -> Option<u64> {
        let n = line.checked_sub(1)?; // newlines before the line
        if n == 0 {
            return Some(0);
        }
        if n > self.newlines {
            return None;
        }
        let j = self.checkpoints.partition_point(|&c| c < n) - 1;
        let start = j * LINE_BLOCK;
        let block = bytes.get(start..((j + 1) * LINE_BLOCK).min(bytes.len()))?;
        let nth = (n - self.checkpoints[j] - 1) as usize;
        memchr::memchr_iter(b'\n', block)
            .nth(nth)
            .map(|p| (start + p + 1) as u64)
    }
}

/// A line and column (see [`LineTable::position`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPos {
    /// 1-based line.
    pub line: u64,
    /// 1-based column, in characters or (when `column_in_bytes`) in bytes.
    pub column: u64,
    /// The column counts bytes because the line start is too far away to count characters.
    pub column_in_bytes: bool,
}

/// Number of characters (UTF-8 lead bytes) in `bytes`.
fn count_chars(bytes: &[u8]) -> u64 {
    bytes.iter().filter(|&&b| b & 0xC0 != 0x80).count() as u64
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
    fn line_start_and_offset_match_naive() {
        // Lines of very different lengths, including blocks without any newline.
        let mut b = Vec::new();
        let mut len = 1usize;
        while b.len() < 5 * LINE_BLOCK {
            b.extend(std::iter::repeat_n(b'x', len));
            b.push(b'\n');
            len = (len * 7 + 3) % (3 * LINE_BLOCK);
        }
        b.extend_from_slice(b"tail");
        let t = LineTable::build(&b);
        let naive_start = |off: usize| memchr::memrchr(b'\n', &b[..off]).map_or(0, |p| p + 1);
        for off in (0..=b.len()).step_by(997).chain([0, 1, b.len()]) {
            assert_eq!(
                t.line_start(&b, off as u64),
                naive_start(off) as u64,
                "{off}"
            );
        }
        let mut starts = vec![0u64];
        starts.extend(memchr::memchr_iter(b'\n', &b).map(|p| p as u64 + 1));
        for (i, &s) in starts.iter().enumerate() {
            assert_eq!(t.line_offset(&b, i as u64 + 1), Some(s), "line {}", i + 1);
        }
        assert_eq!(t.line_offset(&b, starts.len() as u64 + 1), None);
        assert_eq!(t.line_offset(&b, 0), None);
    }

    #[test]
    fn position_bounds_the_column_scan() {
        let b = "x\nÅÅÅ".as_bytes();
        let t = LineTable::build(b);
        let exact = t.position(b, b.len() as u64, 100);
        assert_eq!(
            (exact.line, exact.column, exact.column_in_bytes),
            (2, 4, false)
        );
        let bytes = t.position(b, b.len() as u64, 2);
        assert_eq!(
            (bytes.line, bytes.column, bytes.column_in_bytes),
            (2, 7, true)
        );
    }

    #[test]
    fn empty() {
        let t = LineTable::build(b"");
        assert_eq!(t.line_count(), 1);
        assert_eq!(t.line_col(b"", 0), (1, 1));
    }
}
