//! Structural index: the container table, the line table and the validation result
//! (architecture.md §3).

mod lines;
mod node;
pub mod scalar;
pub mod simd;

use std::fmt;

pub use lines::{LINE_BLOCK, LineTable};
pub use node::{ContainerKind, MAX_DEPTH, MAX_OFFSET, NO_PARENT, Node, UNKNOWN_COUNT};

/// The first error in an invalid document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    /// Offset of the offending byte, or the document length for an unexpected end of input.
    pub offset: u64,
    /// 1-based line.
    pub line: u64,
    /// 1-based column in **characters** (UTF-8 aware).
    pub column: u64,
    /// What would have been valid here, such as `"',' or '}'"`.
    pub expected: &'static str,
    /// The offending byte; `None` at the end of input.
    pub found: Option<u8>,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Invalid JSON at line {}, column {} (offset {}): expected {}, found ",
            group_digits(self.line),
            group_digits(self.column),
            group_digits(self.offset),
            self.expected,
        )?;
        match self.found {
            None => f.write_str("end of file"),
            Some(b) if b.is_ascii_graphic() => write!(f, "'{}'", b as char),
            Some(b' ') => f.write_str("a space"),
            Some(b) => write!(f, "byte 0x{b:02X}"),
        }
    }
}

impl std::error::Error for JsonError {}

/// Whether the document is valid JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Validation {
    /// Valid RFC 8259 JSON.
    Valid,
    /// Invalid; the index covers the containers opened before the error.
    Invalid(JsonError),
}

/// The structural index of a document.
///
/// Built by [`scalar::build`] (reference implementation) or the parallel builder (step 4c);
/// both produce equal values for the same input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub(crate) nodes: Vec<Node>,
    pub(crate) lines: LineTable,
    pub(crate) validation: Validation,
    pub(crate) max_stored_depth: Option<u32>,
    pub(crate) len: u64,
}

impl Index {
    /// All stored containers in document order. Node id = position in this slice.
    ///
    /// In an invalid document, containers that weren't closed before the error have
    /// `end() == error offset` and `child_count() == UNKNOWN_COUNT`.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The line table.
    pub fn lines(&self) -> &LineTable {
        &self.lines
    }

    /// Validation result.
    pub fn validation(&self) -> &Validation {
        &self.validation
    }

    /// `true` if the document is valid JSON.
    pub fn is_valid(&self) -> bool {
        self.validation == Validation::Valid
    }

    /// Adaptive mode: containers deeper than this are not stored (`None`: all are).
    pub fn max_stored_depth(&self) -> Option<u32> {
        self.max_stored_depth
    }

    /// Length of the indexed bytes.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// `true` for an index of zero bytes.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Heap bytes used by the index.
    pub fn memory_bytes(&self) -> usize {
        self.nodes.capacity() * std::mem::size_of::<Node>() + self.lines.memory_bytes()
    }
}

/// Formats `n` with `,` thousands separators: `1204331` → `"1,204,331"`.
pub(crate) fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1000), "1,000");
        assert_eq!(group_digits(88_120_551), "88,120,551");
    }

    #[test]
    fn message() {
        let e = JsonError {
            offset: 88_120_551,
            line: 1_204_331,
            column: 17,
            expected: "',' or '}'",
            found: Some(b'x'),
        };
        assert_eq!(
            e.to_string(),
            "Invalid JSON at line 1,204,331, column 17 (offset 88,120,551): \
             expected ',' or '}', found 'x'"
        );
        let e = JsonError { found: None, ..e };
        assert!(e.to_string().ends_with("found end of file"));
    }
}
