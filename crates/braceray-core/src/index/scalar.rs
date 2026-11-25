//! Reference indexer: a single-threaded, byte-by-byte RFC 8259 validator that builds the
//! node table with an explicit stack (no recursion).
//!
//! It is the oracle for the SIMD / parallel builder, which must produce an equal [`Index`].
//! Like `serde_json`'s `IgnoredAny`, it doesn't validate UTF-8 inside strings or pair
//! `\u` surrogates; everything else in the grammar is checked.

use super::lines::LineTable;
use super::node::{
    ContainerKind, MAX_DEPTH, MAX_OFFSET, NO_PARENT, Node, UNKNOWN_COUNT, stored_count,
};
use super::{Index, JsonError, Validation};
use crate::document::is_json_ws;
use crate::error::Error;
use crate::job::{CancelToken, Progress};

/// Cancellation is checked and progress reported every this many bytes.
const TICK: usize = 1 << 20;

/// Message for nesting beyond [`MAX_DEPTH`].
pub(crate) const TOO_DEEP: &str = "less nesting (at most 16,777,215 levels)";

/// Builds the index of `bytes` byte by byte.
///
/// `max_stored_depth` is the adaptive-mode limit (`None` stores every container).
/// Returns `Err` only for cancellation or a document over 1 TiB; invalid JSON gives an
/// `Ok` index whose [`Index::validation`] is `Invalid`.
///
/// # Example
/// ```
/// use braceray_core::index::{scalar, ContainerKind};
/// use braceray_core::{CancelToken, Progress};
///
/// let index = scalar::build(br#"{"a": [1, 2], "b": {}}"#, None, &CancelToken::new(), &Progress::default())?;
/// assert!(index.is_valid());
/// let kinds: Vec<_> = index.nodes().iter().map(|n| (n.kind(), n.child_count())).collect();
/// assert_eq!(kinds, [(ContainerKind::Object, 2), (ContainerKind::Array, 2), (ContainerKind::Object, 0)]);
/// # Ok::<(), braceray_core::Error>(())
/// ```
pub fn build(
    bytes: &[u8],
    max_stored_depth: Option<u32>,
    cancel: &CancelToken,
    progress: &Progress,
) -> Result<Index, Error> {
    if bytes.len() as u64 > MAX_OFFSET {
        return Err(Error::FileTooLarge);
    }
    progress.reset(bytes.len() as u64);
    let mut p = Parser {
        bytes,
        i: 0,
        nodes: Vec::new(),
        stack: Vec::new(),
        max_stored_depth,
        next_tick: TICK,
        reported: 0,
        cancel,
        progress,
    };
    let fail = match p.run() {
        Ok(()) => None,
        Err(Stop::Invalid(f)) => Some(f),
        Err(Stop::Cancelled) => return Err(Error::Cancelled),
        Err(Stop::TooManyNodes) => return Err(Error::FileTooLarge),
    };
    let lines = LineTable::build(bytes);
    let validation = match fail {
        None => Validation::Valid,
        Some(f) => {
            // Containers still open end at the error and have an unknown child count.
            for frame in &p.stack {
                if frame.node != NO_PARENT {
                    p.nodes[frame.node as usize].set_end(f.offset as u64);
                }
            }
            let (line, column) = lines.line_col(bytes, f.offset as u64);
            Validation::Invalid(JsonError {
                offset: f.offset as u64,
                line,
                column,
                expected: f.expected,
                found: bytes.get(f.offset).copied(),
            })
        }
    };
    progress.add((bytes.len() - p.reported) as u64);
    let mut nodes = p.nodes;
    nodes.shrink_to_fit();
    Ok(Index {
        nodes,
        lines,
        validation,
        max_stored_depth,
        len: bytes.len() as u64,
    })
}

struct Fail {
    offset: usize,
    expected: &'static str,
}

enum Stop {
    Invalid(Fail),
    Cancelled,
    TooManyNodes,
}

fn fail<T>(offset: usize, expected: &'static str) -> Result<T, Stop> {
    Err(Stop::Invalid(Fail { offset, expected }))
}

/// What the grammar allows next.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// Any value (document start, after `:` or after `,` in an array).
    Value,
    /// A value or `]` (right after `[`).
    ValueOrClose,
    /// A key or `}` (right after `{`).
    KeyOrClose,
    /// A key (after `,` in an object).
    Key,
    /// `:` after a key.
    Colon,
    /// `,` or the closing bracket of the current container.
    CommaOrClose,
    /// Nothing but whitespace (after the top-level value).
    Done,
}

struct Frame {
    kind: ContainerKind,
    /// Node id, or `NO_PARENT` when not stored (adaptive mode).
    node: u32,
    children: u64,
}

struct Parser<'a> {
    bytes: &'a [u8],
    i: usize,
    nodes: Vec<Node>,
    stack: Vec<Frame>,
    max_stored_depth: Option<u32>,
    next_tick: usize,
    reported: usize,
    cancel: &'a CancelToken,
    progress: &'a Progress,
}

impl Parser<'_> {
    fn run(&mut self) -> Result<(), Stop> {
        let mut expect = Expect::Value;
        loop {
            // Skip whitespace.
            let b = loop {
                self.tick()?;
                match self.bytes.get(self.i) {
                    None => return self.at_eof(expect),
                    Some(&b) if is_json_ws(b) => self.i += 1,
                    Some(&b) => break b,
                }
            };
            expect = match expect {
                Expect::Value | Expect::ValueOrClose => {
                    if b == b']' && expect == Expect::ValueOrClose {
                        self.close();
                        self.after_value()
                    } else {
                        self.count_array_child();
                        self.value(b, expect)?
                    }
                }
                Expect::KeyOrClose | Expect::Key => match b {
                    b'"' => {
                        if let Some(top) = self.stack.last_mut() {
                            top.children += 1;
                        }
                        self.string()?;
                        Expect::Colon
                    }
                    b'}' if expect == Expect::KeyOrClose => {
                        self.close();
                        self.after_value()
                    }
                    _ if expect == Expect::KeyOrClose => return fail(self.i, "string key or '}'"),
                    _ => return fail(self.i, "string key"),
                },
                Expect::Colon => {
                    if b != b':' {
                        return fail(self.i, "':'");
                    }
                    self.i += 1;
                    Expect::Value
                }
                Expect::CommaOrClose => {
                    let kind = self.top_kind();
                    match (b, kind) {
                        (b',', ContainerKind::Object) => {
                            self.i += 1;
                            Expect::Key
                        }
                        (b',', ContainerKind::Array) => {
                            self.i += 1;
                            Expect::Value
                        }
                        (b'}', ContainerKind::Object) | (b']', ContainerKind::Array) => {
                            self.close();
                            self.after_value()
                        }
                        _ => return fail(self.i, comma_or_close(kind)),
                    }
                }
                Expect::Done => return fail(self.i, "end of input"),
            };
        }
    }

    /// Parses the value starting with `b` at `self.i`.
    fn value(&mut self, b: u8, expect: Expect) -> Result<Expect, Stop> {
        match b {
            b'{' => {
                self.open(ContainerKind::Object)?;
                return Ok(Expect::KeyOrClose);
            }
            b'[' => {
                self.open(ContainerKind::Array)?;
                return Ok(Expect::ValueOrClose);
            }
            b'"' => self.string()?,
            b'-' | b'0'..=b'9' => self.number()?,
            b't' => self.literal(b"true", "'true'")?,
            b'f' => self.literal(b"false", "'false'")?,
            b'n' => self.literal(b"null", "'null'")?,
            _ if expect == Expect::ValueOrClose => return fail(self.i, "value or ']'"),
            _ => return fail(self.i, "value"),
        }
        Ok(self.after_value())
    }

    fn at_eof(&self, expect: Expect) -> Result<(), Stop> {
        let end = self.bytes.len();
        match expect {
            Expect::Done => Ok(()),
            Expect::Value => fail(end, "value"),
            Expect::ValueOrClose => fail(end, "value or ']'"),
            Expect::KeyOrClose => fail(end, "string key or '}'"),
            Expect::Key => fail(end, "string key"),
            Expect::Colon => fail(end, "':'"),
            Expect::CommaOrClose => fail(end, comma_or_close(self.top_kind())),
        }
    }

    fn after_value(&self) -> Expect {
        if self.stack.is_empty() {
            Expect::Done
        } else {
            Expect::CommaOrClose
        }
    }

    fn top_kind(&self) -> ContainerKind {
        // Only called in CommaOrClose, which implies a non-empty stack.
        self.stack.last().map_or(ContainerKind::Array, |f| f.kind)
    }

    fn count_array_child(&mut self) {
        if let Some(top) = self.stack.last_mut()
            && top.kind == ContainerKind::Array
        {
            top.children += 1;
        }
    }

    fn open(&mut self, kind: ContainerKind) -> Result<(), Stop> {
        let depth = self.stack.len();
        if depth > MAX_DEPTH as usize {
            return fail(self.i, TOO_DEEP);
        }
        let depth = depth as u32;
        let stored = self.max_stored_depth.is_none_or(|d| depth <= d);
        let node = if stored {
            let id = u32::try_from(self.nodes.len())
                .ok()
                .filter(|&id| id != NO_PARENT)
                .ok_or(Stop::TooManyNodes)?;
            // Every ancestor of a stored node is stored, so the top frame is the parent.
            let parent = self.stack.last().map_or(NO_PARENT, |f| f.node);
            self.nodes.push(Node::new(
                kind,
                self.i as u64,
                0,
                depth,
                parent,
                UNKNOWN_COUNT,
            ));
            id
        } else {
            NO_PARENT
        };
        self.stack.push(Frame {
            kind,
            node,
            children: 0,
        });
        self.i += 1;
        Ok(())
    }

    /// Closes the top container at `self.i` (the caller checked the bracket matches).
    fn close(&mut self) {
        if let Some(frame) = self.stack.pop()
            && frame.node != NO_PARENT
        {
            let node = &mut self.nodes[frame.node as usize];
            node.set_end(self.i as u64);
            node.set_child_count(stored_count(frame.children));
        }
        self.i += 1;
    }

    fn string(&mut self) -> Result<(), Stop> {
        let bytes = self.bytes;
        self.i += 1; // opening quote
        loop {
            self.tick()?;
            let Some(&b) = bytes.get(self.i) else {
                return fail(bytes.len(), "closing '\"'");
            };
            match b {
                b'"' => {
                    self.i += 1;
                    return Ok(());
                }
                b'\\' => {
                    self.i += 1;
                    match bytes.get(self.i) {
                        None => return fail(bytes.len(), "escape character"),
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.i += 1;
                        }
                        Some(b'u') => {
                            self.i += 1;
                            for _ in 0..4 {
                                match bytes.get(self.i) {
                                    Some(h) if h.is_ascii_hexdigit() => self.i += 1,
                                    Some(_) => return fail(self.i, "hex digit"),
                                    None => return fail(bytes.len(), "hex digit"),
                                }
                            }
                        }
                        Some(_) => return fail(self.i, "escape character"),
                    }
                }
                0x00..=0x1F => {
                    return fail(self.i, "closing '\"' (control characters must be escaped)");
                }
                _ => self.i += 1,
            }
        }
    }

    fn number(&mut self) -> Result<(), Stop> {
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => self.digits()?,
            _ => return fail(self.i, "digit"),
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            self.required_digits()?;
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.i += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.i += 1;
            }
            self.required_digits()?;
        }
        Ok(())
    }

    fn required_digits(&mut self) -> Result<(), Stop> {
        match self.peek() {
            Some(b'0'..=b'9') => self.digits(),
            _ => fail(self.i, "digit"),
        }
    }

    fn digits(&mut self) -> Result<(), Stop> {
        while let Some(b'0'..=b'9') = self.peek() {
            self.i += 1;
            self.tick()?;
        }
        Ok(())
    }

    fn literal(&mut self, word: &'static [u8], expected: &'static str) -> Result<(), Stop> {
        for &c in word {
            if self.peek() != Some(c) {
                return fail(self.i, expected);
            }
            self.i += 1;
        }
        Ok(())
    }

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.i).copied()
    }

    /// Every [`TICK`] bytes: check cancellation and report progress.
    #[inline]
    fn tick(&mut self) -> Result<(), Stop> {
        if self.i >= self.next_tick {
            if self.cancel.is_cancelled() {
                return Err(Stop::Cancelled);
            }
            let done = self.i.min(self.bytes.len());
            self.progress.add((done - self.reported) as u64);
            self.reported = done;
            self.next_tick = self.i + TICK;
        }
        Ok(())
    }
}

fn comma_or_close(kind: ContainerKind) -> &'static str {
    match kind {
        ContainerKind::Object => "',' or '}'",
        ContainerKind::Array => "',' or ']'",
    }
}
