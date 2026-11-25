//! The reference JSON state machine, shared by both builders.
//!
//! [`Parser::step`] parses one token byte by byte (the reference behaviour).
//! [`Parser::fast_token`] applies one token that a SIMD worker already classified and
//! validated; it changes the state exactly as `step` would, or declines so that the
//! caller hands the token to `step`. Errors therefore always come from `step`.

mod lexer;

use super::chunk::Tok;
use super::lines::LineTable;
use super::node::{ContainerKind, MAX_DEPTH, NO_PARENT, Node, UNKNOWN_COUNT, stored_count};
use super::{Index, JsonError, Validation};
use crate::document::is_json_ws;
use crate::error::Error;
use crate::job::{CancelToken, Progress};

/// Cancellation is checked and progress reported every this many bytes.
const TICK: usize = 1 << 20;

/// Message for nesting beyond [`MAX_DEPTH`].
const TOO_DEEP: &str = "less nesting (at most 16,777,215 levels)";

pub(crate) struct Fail {
    pub(crate) offset: usize,
    pub(crate) expected: &'static str,
}

pub(crate) enum Stop {
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

#[derive(Clone, Copy)]
struct Frame {
    kind: ContainerKind,
    /// Node id, or `NO_PARENT` when not stored (adaptive mode).
    node: u32,
    children: u64,
}

pub(crate) struct Parser<'a> {
    bytes: &'a [u8],
    /// Next byte to look at.
    pub(crate) i: usize,
    /// Set by the token consumer after it handed a token to [`step`](Self::step): bytes
    /// up to the next token may still need the reference parser (see `catch_up`).
    pub(crate) stepped: bool,
    expect: Expect,
    nodes: Vec<Node>,
    stack: Vec<Frame>,
    max_stored_depth: Option<u32>,
    next_tick: usize,
    reported: usize,
    cancel: &'a CancelToken,
    /// `None`: don't report progress (the caller does).
    progress: Option<&'a Progress>,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(
        bytes: &'a [u8],
        max_stored_depth: Option<u32>,
        cancel: &'a CancelToken,
        progress: Option<&'a Progress>,
    ) -> Self {
        Parser {
            bytes,
            i: 0,
            stepped: false,
            expect: Expect::Value,
            nodes: Vec::new(),
            stack: Vec::new(),
            max_stored_depth,
            next_tick: TICK,
            reported: 0,
            cancel,
            progress,
        }
    }

    /// Parses to the end of the input.
    pub(crate) fn run(&mut self) -> Result<(), Stop> {
        while self.step()? {}
        Ok(())
    }

    /// Parses one token (after skipping whitespace). `Ok(false)` at a valid end of input.
    pub(crate) fn step(&mut self) -> Result<bool, Stop> {
        let b = loop {
            self.tick()?;
            match self.bytes.get(self.i) {
                None => return self.at_eof().map(|()| false),
                Some(&b) if is_json_ws(b) => self.i += 1,
                Some(&b) => break b,
            }
        };
        let expect = self.expect;
        self.expect = match expect {
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
                    self.count_key();
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
        Ok(true)
    }

    /// Applies a pre-validated token at `offset` if the grammar accepts it here, exactly as
    /// [`step`](Self::step) would. Returns `Ok(false)` (state unchanged) otherwise.
    #[inline]
    pub(crate) fn fast_token(&mut self, tok: Tok, offset: usize) -> Result<bool, Stop> {
        use Expect as E;
        let expect = self.expect;
        self.expect = match (expect, tok) {
            (E::Value | E::ValueOrClose, Tok::OpenObject) => {
                self.i = offset;
                self.count_array_child();
                self.open(ContainerKind::Object)?;
                E::KeyOrClose
            }
            (E::Value | E::ValueOrClose, Tok::OpenArray) => {
                self.i = offset;
                self.count_array_child();
                self.open(ContainerKind::Array)?;
                E::ValueOrClose
            }
            (E::Value | E::ValueOrClose, Tok::CleanString | Tok::Scalar) => {
                self.count_array_child();
                self.after_value()
            }
            (E::KeyOrClose | E::Key, Tok::CleanString) => {
                self.count_key();
                E::Colon
            }
            (E::KeyOrClose | E::Key, Tok::Key) => {
                self.count_key();
                E::Value
            }
            (E::Colon, Tok::Colon) => E::Value,
            (E::CommaOrClose, Tok::Comma) => match self.top_kind() {
                ContainerKind::Object => E::Key,
                ContainerKind::Array => E::Value,
            },
            (E::ValueOrClose, Tok::CloseArray) | (E::KeyOrClose, Tok::CloseObject) => {
                self.i = offset;
                self.close();
                self.after_value()
            }
            (E::CommaOrClose, Tok::CloseObject) if self.top_kind() == ContainerKind::Object => {
                self.i = offset;
                self.close();
                self.after_value()
            }
            (E::CommaOrClose, Tok::CloseArray) if self.top_kind() == ContainerKind::Array => {
                self.i = offset;
                self.close();
                self.after_value()
            }
            _ => return Ok(false),
        };
        Ok(true)
    }

    /// After a hand-off to `step`: parses with `step` until only whitespace is left before
    /// `limit` (the next token's offset, or the input length). A `step` may stop inside a
    /// token's byte range (`null0` reads `null`), and those bytes have no token of their own.
    pub(crate) fn catch_up(&mut self, limit: usize) -> Result<(), Stop> {
        self.stepped = false;
        while self.i < limit {
            match self.bytes[self.i..limit]
                .iter()
                .position(|&b| !is_json_ws(b))
            {
                Some(p) => {
                    self.i += p;
                    if !self.step()? {
                        break;
                    }
                }
                None => break,
            }
        }
        Ok(())
    }

    /// After the last token: checks the end of input as `step` would at EOF.
    pub(crate) fn finish_input(&mut self) -> Result<(), Stop> {
        if self.expect != Expect::Done {
            self.i = self.bytes.len();
        }
        self.at_eof()
    }

    /// Number of stored nodes.
    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Adaptive mode: drops stored nodes deeper than the largest depth `D` for which at most
    /// `target` nodes remain, remaps parent ids, and stores only depth ≤ `D` from now on.
    pub(crate) fn compact(&mut self, target: usize) {
        let mut per_depth: Vec<usize> = Vec::new();
        for n in &self.nodes {
            let d = n.depth() as usize;
            if d >= per_depth.len() {
                per_depth.resize(d + 1, 0);
            }
            per_depth[d] += 1;
        }
        let mut keep_depth = 0u32;
        let mut total = 0usize;
        for (d, &c) in per_depth.iter().enumerate() {
            total += c;
            if total > target && d > 0 {
                break;
            }
            keep_depth = d as u32;
        }
        let limit = self
            .max_stored_depth
            .map_or(keep_depth, |d| d.min(keep_depth));
        self.max_stored_depth = Some(limit);

        // New id of every kept node; NO_PARENT for dropped ones.
        let mut new_id = vec![NO_PARENT; self.nodes.len()];
        let mut kept = 0u32;
        for (old, n) in self.nodes.iter().enumerate() {
            if n.depth() <= limit {
                new_id[old] = kept;
                kept += 1;
            }
        }
        let remap = |id: u32| {
            if id == NO_PARENT {
                NO_PARENT
            } else {
                new_id[id as usize]
            }
        };
        let old_nodes = std::mem::take(&mut self.nodes);
        self.nodes = old_nodes
            .into_iter()
            .filter(|n| n.depth() <= limit)
            .map(|n| n.with_parent(remap(n.parent())))
            .collect();
        for f in &mut self.stack {
            f.node = remap(f.node);
        }
    }

    /// Turns the final state into an [`Index`] (`fail` is the first error, if any).
    pub(crate) fn finish(self, fail: Option<Fail>, lines: LineTable) -> Index {
        let bytes = self.bytes;
        let mut nodes = self.nodes;
        let validation = match fail {
            None => Validation::Valid,
            Some(f) => {
                // Containers still open end at the error and keep an unknown child count.
                for frame in &self.stack {
                    if frame.node != NO_PARENT {
                        nodes[frame.node as usize].set_end(f.offset as u64);
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
        if let Some(p) = self.progress {
            p.add((bytes.len() - self.reported.min(bytes.len())) as u64);
        }
        nodes.shrink_to_fit();
        Index {
            nodes,
            lines,
            validation,
            max_stored_depth: self.max_stored_depth,
            len: bytes.len() as u64,
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

    fn at_eof(&self) -> Result<(), Stop> {
        let end = self.bytes.len();
        match self.expect {
            Expect::Done => Ok(()),
            Expect::Value => fail(end, "value"),
            Expect::ValueOrClose => fail(end, "value or ']'"),
            Expect::KeyOrClose => fail(end, "string key or '}'"),
            Expect::Key => fail(end, "string key"),
            Expect::Colon => fail(end, "':'"),
            Expect::CommaOrClose => fail(end, comma_or_close(self.top_kind())),
        }
    }

    #[inline]
    fn after_value(&self) -> Expect {
        if self.stack.is_empty() {
            Expect::Done
        } else {
            Expect::CommaOrClose
        }
    }

    #[inline]
    fn top_kind(&self) -> ContainerKind {
        // Only needed in CommaOrClose, which implies a non-empty stack.
        self.stack.last().map_or(ContainerKind::Array, |f| f.kind)
    }

    #[inline]
    fn count_array_child(&mut self) {
        if let Some(top) = self.stack.last_mut()
            && top.kind == ContainerKind::Array
        {
            top.children += 1;
        }
    }

    #[inline]
    fn count_key(&mut self) {
        if let Some(top) = self.stack.last_mut() {
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
}

fn comma_or_close(kind: ContainerKind) -> &'static str {
    match kind {
        ContainerKind::Object => "',' or '}'",
        ContainerKind::Array => "',' or ']'",
    }
}

/// Builds with the reference parser only (see `scalar::build`).
pub(crate) fn build_reference(
    bytes: &[u8],
    max_stored_depth: Option<u32>,
    cancel: &CancelToken,
    progress: &Progress,
) -> Result<Index, Error> {
    progress.reset(bytes.len() as u64);
    let mut p = Parser::new(bytes, max_stored_depth, cancel, Some(progress));
    let fail = match p.run() {
        Ok(()) => None,
        Err(Stop::Invalid(f)) => Some(f),
        Err(Stop::Cancelled) => return Err(Error::Cancelled),
        Err(Stop::TooManyNodes) => return Err(Error::FileTooLarge),
    };
    Ok(p.finish(fail, LineTable::build(bytes)))
}
