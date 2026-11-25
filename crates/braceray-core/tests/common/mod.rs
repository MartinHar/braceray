//! Shared test helpers: a proptest JSON generator that also records the expected node
//! table, serde-based oracles, and fixture paths.
#![allow(dead_code)] // each test binary uses a different subset

use std::path::{Path, PathBuf};

use braceray_core::index::{ContainerKind, NO_PARENT, Node};
use proptest::prelude::*;

pub fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

/// A JSON tree. Strings hold their **encoded** body (escapes already applied), so the
/// generator controls exactly which escape sequences appear.
#[derive(Debug, Clone)]
pub enum J {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

/// Pieces of encoded string bodies: escapes, multi-byte UTF-8, structural characters.
const STR_PIECES: &[&str] = &[
    "a",
    "key",
    " ",
    "Արամ",
    "Ереван",
    "日本",
    "😀",
    "\\\"",
    "\\\\",
    "\\\\\\\"",
    "\\/",
    "\\b",
    "\\f",
    "\\n",
    "\\r",
    "\\t",
    "\\u0000",
    "\\u001f",
    "\\u00e9",
    "\\ud83d\\ude00",
    "{",
    "}",
    "[",
    "]",
    ",",
    ":",
    "\\\\n",
];

fn encoded_string() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(STR_PIECES), 0..6).prop_map(|v| v.concat())
}

fn number() -> impl Strategy<Value = String> {
    "-?(0|[1-9][0-9]{0,8})(\\.[0-9]{1,4})?([eE][+-]?[0-9]{1,3})?"
}

pub fn json_tree() -> impl Strategy<Value = J> {
    let leaf = prop_oneof![
        Just(J::Null),
        any::<bool>().prop_map(J::Bool),
        number().prop_map(J::Num),
        encoded_string().prop_map(J::Str),
    ];
    leaf.prop_recursive(20, 256, 8, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..8).prop_map(J::Arr),
            prop::collection::vec((encoded_string(), inner), 0..8).prop_map(J::Obj),
        ]
    })
}

/// How to lay out the serialized text.
#[derive(Debug, Clone)]
pub enum Layout {
    Minified,
    Pretty,
    /// Whitespace from this list is inserted between tokens, cycling through it.
    Random(Vec<&'static str>),
}

pub fn layout() -> impl Strategy<Value = Layout> {
    let ws = prop::sample::select(&["", " ", "\n", "\t", "\r\n", "  \n  ", "\n\n"][..]);
    prop_oneof![
        Just(Layout::Minified),
        Just(Layout::Pretty),
        prop::collection::vec(ws, 1..8).prop_map(Layout::Random),
    ]
}

/// Serializes `tree` and returns the text plus the node table a correct indexer produces.
pub fn serialize(tree: &J, layout: &Layout) -> (Vec<u8>, Vec<Node>) {
    let mut s = Ser {
        out: Vec::new(),
        nodes: Vec::new(),
        layout,
        ws_i: 0,
    };
    s.ws();
    s.value(tree, 0, NO_PARENT);
    s.ws();
    (s.out, s.nodes)
}

struct Ser<'a> {
    out: Vec<u8>,
    nodes: Vec<Node>,
    layout: &'a Layout,
    ws_i: usize,
}

impl Ser<'_> {
    fn ws(&mut self) {
        if let Layout::Random(list) = self.layout {
            let w = list[self.ws_i % list.len()];
            self.ws_i += 1;
            self.out.extend_from_slice(w.as_bytes());
        }
    }

    fn newline(&mut self, depth: usize) {
        match self.layout {
            Layout::Pretty => {
                self.out.push(b'\n');
                self.out.extend(std::iter::repeat_n(b' ', depth * 2));
            }
            _ => self.ws(),
        }
    }

    fn value(&mut self, v: &J, depth: usize, parent: u32) {
        match v {
            J::Null => self.out.extend_from_slice(b"null"),
            J::Bool(b) => self
                .out
                .extend_from_slice(if *b { b"true" } else { b"false" }),
            J::Num(n) => self.out.extend_from_slice(n.as_bytes()),
            J::Str(s) => self.string(s),
            J::Arr(items) => {
                let id = self.open(ContainerKind::Array, depth, parent, items.len());
                self.out.push(b'[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        self.out.push(b',');
                    }
                    self.newline(depth + 1);
                    self.value(item, depth + 1, id);
                }
                if !items.is_empty() {
                    self.newline(depth);
                } else {
                    self.ws();
                }
                self.close(id);
                self.out.push(b']');
            }
            J::Obj(members) => {
                let id = self.open(ContainerKind::Object, depth, parent, members.len());
                self.out.push(b'{');
                for (i, (k, item)) in members.iter().enumerate() {
                    if i > 0 {
                        self.out.push(b',');
                    }
                    self.newline(depth + 1);
                    self.string(k);
                    self.ws();
                    self.out.push(b':');
                    if matches!(self.layout, Layout::Pretty) {
                        self.out.push(b' ');
                    } else {
                        self.ws();
                    }
                    self.value(item, depth + 1, id);
                }
                if !members.is_empty() {
                    self.newline(depth);
                } else {
                    self.ws();
                }
                self.close(id);
                self.out.push(b'}');
            }
        }
    }

    fn string(&mut self, body: &str) {
        self.out.push(b'"');
        self.out.extend_from_slice(body.as_bytes());
        self.out.push(b'"');
    }

    fn open(&mut self, kind: ContainerKind, depth: usize, parent: u32, children: usize) -> u32 {
        let id = self.nodes.len() as u32;
        let start = self.out.len() as u64;
        self.nodes.push(Node::new(
            kind,
            start,
            0,
            depth as u32,
            parent,
            children as u32,
        ));
        id
    }

    fn close(&mut self, id: u32) {
        let n = self.nodes[id as usize];
        self.nodes[id as usize] = Node::new(
            n.kind(),
            n.start(),
            self.out.len() as u64,
            n.depth(),
            n.parent(),
            n.child_count(),
        );
    }
}

/// serde's verdict: `Ok` or the 1-based (line, byte column) it reports.
pub fn serde_validity(bytes: &[u8]) -> Result<(), (usize, usize)> {
    serde_json::from_slice::<serde::de::IgnoredAny>(bytes)
        .map(|_| ())
        .map_err(|e| (e.line(), e.column()))
}

/// Parses with serde without its recursion limit (for deep fixtures).
pub fn serde_value(bytes: &[u8]) -> serde_json::Value {
    use serde::Deserialize;
    let mut de = serde_json::Deserializer::from_slice(bytes);
    de.disable_recursion_limit();
    let v = serde_json::Value::deserialize(&mut de).unwrap();
    de.end().unwrap();
    v
}

/// Byte offset of serde's 1-based (line, column) error position. Column 0 means the
/// newline that ends the previous line.
pub fn serde_offset(bytes: &[u8], line: usize, col: usize) -> u64 {
    let mut start = 0usize;
    for _ in 1..line {
        start += bytes[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len() - start, |p| p + 1);
    }
    (start + col).saturating_sub(1) as u64
}

/// Pre-order list of (kind, child count) from a serde walk (explicit stack).
pub fn serde_containers(v: &serde_json::Value) -> Vec<(ContainerKind, u32)> {
    use serde_json::Value;
    let mut out = Vec::new();
    let mut stack = vec![v];
    while let Some(v) = stack.pop() {
        match v {
            Value::Array(a) => {
                out.push((ContainerKind::Array, a.len() as u32));
                stack.extend(a.iter().rev());
            }
            Value::Object(o) => {
                out.push((ContainerKind::Object, o.len() as u32));
                stack.extend(o.values().rev());
            }
            _ => {}
        }
    }
    out
}

/// Applies one byte mutation: 0 = delete, 1 = insert, 2 = replace.
pub fn mutate(bytes: &[u8], op: u8, pos: usize, byte: u8) -> Vec<u8> {
    let mut v = bytes.to_vec();
    if v.is_empty() {
        v.push(byte);
        return v;
    }
    let pos = pos % (v.len() + usize::from(op == 1));
    match op % 3 {
        0 => {
            v.remove(pos.min(v.len() - 1));
        }
        1 => v.insert(pos, byte),
        _ => {
            let p = pos.min(v.len() - 1);
            v[p] = byte;
        }
    }
    v
}

/// Bytes that make mutations interesting.
pub fn mutation_byte() -> impl Strategy<Value = u8> {
    prop_oneof![
        prop::sample::select(&b"{}[],:\"\\ \n0123456789-+.eEtrufalsn\x01\x1f\x7f"[..]),
        any::<u8>(),
    ]
}
