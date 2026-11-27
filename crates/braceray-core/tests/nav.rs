//! Navigation against the generator's oracle: paths at random offsets, children, counts.

mod common;

use std::time::Instant;

use braceray_core::index::{self, Index, IndexOptions};
use braceray_core::nav::{Container, Navigator, PathSegment, ValueKind};
use braceray_core::{CancelToken, Progress};
use common::*;
use proptest::prelude::*;

fn build(bytes: &[u8], budget: u64) -> Index {
    let opts = IndexOptions {
        chunk_size: 4096,
        memory_budget: budget,
        threads: None,
    };
    index::build_bytes(bytes, &opts, &CancelToken::new(), &Progress::default()).unwrap()
}

fn to_segs(bytes: &[u8], path: &[PathSegment]) -> Vec<Seg> {
    path.iter()
        .map(|p| match p {
            PathSegment::Key(span) => {
                let raw = span.slice(bytes);
                Seg::Key(String::from_utf8(raw[1..raw.len() - 1].to_vec()).unwrap())
            }
            PathSegment::Index(i) => Seg::Index(*i),
        })
        .collect()
}

/// The deepest value whose key-or-value extent contains `offset`.
fn oracle_path(values: &[ValueInfo], offset: usize) -> Vec<Seg> {
    values
        .iter()
        .filter(|v| v.first <= offset && offset < v.end)
        .max_by_key(|v| v.path.len())
        .map(|v| v.path.clone())
        .unwrap_or_default()
}

/// Walks the tree through `children`, checking kinds, counts and spans against `J`.
fn check_children(nav: &Navigator, bytes: &[u8], c: &Container, tree: &J) {
    let (kinds, count): (Vec<ValueKind>, usize) = match tree {
        J::Arr(items) => (items.iter().map(kind_of).collect(), items.len()),
        J::Obj(members) => (
            members.iter().map(|(_, v)| kind_of(v)).collect(),
            members.len(),
        ),
        _ => unreachable!(),
    };
    assert_eq!(nav.child_count(c), count as u64);
    let children = nav.children(c, 0..u64::MAX);
    assert_eq!(children.iter().map(|c| c.kind).collect::<Vec<_>>(), kinds);
    // Ranges from the middle give the same children.
    if count > 1 {
        assert_eq!(nav.children(c, 1..count as u64), children[1..]);
    }
    let subtrees: Vec<&J> = match tree {
        J::Arr(items) => items.iter().collect(),
        J::Obj(members) => {
            for (child, (k, _)) in children.iter().zip(members) {
                let key = child.key.unwrap().slice(bytes);
                assert_eq!(&key[1..key.len() - 1], k.as_bytes());
            }
            members.iter().map(|(_, v)| v).collect()
        }
        _ => unreachable!(),
    };
    for (child, sub) in children.iter().zip(subtrees) {
        if let Some(inner) = nav.container_of(child) {
            check_children(nav, bytes, &inner, sub);
        }
    }
}

fn kind_of(j: &J) -> ValueKind {
    match j {
        J::Null => ValueKind::Null,
        J::Bool(_) => ValueKind::Bool,
        J::Num(_) => ValueKind::Number,
        J::Str(_) => ValueKind::String,
        J::Arr(_) => ValueKind::Array,
        J::Obj(_) => ValueKind::Object,
    }
}

#[test]
fn million_element_array() {
    let mut bytes = b"[".to_vec();
    for i in 0..1_000_000u32 {
        if i > 0 {
            bytes.extend_from_slice(b", ");
        }
        bytes.extend_from_slice(i.to_string().as_bytes());
    }
    bytes.push(b']');
    let idx = build(&bytes, 0);
    let nav = Navigator::new(&bytes, &idx);
    let root = nav.root_container().unwrap();
    let t = Instant::now();
    let first = nav.children(&root, 0..50);
    let elapsed = t.elapsed();
    assert_eq!(nav.child_count(&root), 1_000_000);
    assert_eq!(first.len(), 50);
    let deep = nav.children(&root, 765_432..765_435);
    let text: Vec<_> = deep.iter().map(|c| c.value.slice(&bytes)).collect();
    assert_eq!(text, [&b"765432"[..], b"765433", b"765434"]);
    assert!(nav.children(&root, 999_999..2_000_000).len() == 1);
    eprintln!("first children of a 1M array: {elapsed:?}");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// 1000 documents × 10 offsets = 10,000 random offsets; also in adaptive mode.
    #[test]
    fn paths_at_random_offsets(
        tree in json_tree(),
        layout in layout(),
        offsets in prop::collection::vec(any::<prop::sample::Index>(), 10),
    ) {
        let (bytes, _, values) = serialize_with_values(&tree, &layout);
        for budget in [0, 24] {
            let idx = build(&bytes, budget);
            let nav = Navigator::new(&bytes, &idx);
            for o in &offsets {
                let off = o.index(bytes.len() + 1);
                let loc = nav.node_at_offset(off as u64);
                let path = nav.path_of(&loc);
                let got = to_segs(&bytes, &path);
                prop_assert_eq!(got, oracle_path(&values, off), "offset {} budget {} in {:?}",
                    off, budget, String::from_utf8_lossy(&bytes));
                // The address walks the same steps by child index.
                let address = nav.address_of(&loc);
                prop_assert_eq!(address.len(), path.len());
                let mut c = nav.root_container();
                for (k, seg) in address.iter().zip(&path) {
                    let child = nav.children(c.as_ref().unwrap(), *k..*k + 1)[0];
                    match seg {
                        PathSegment::Key(span) => prop_assert_eq!(child.key, Some(*span)),
                        PathSegment::Index(i) => prop_assert_eq!(i, k),
                    }
                    c = nav.container_of(&child);
                }
            }
        }
    }

    #[test]
    fn children_match_the_tree(tree in json_tree(), layout in layout()) {
        let (bytes, _) = serialize(&tree, &layout);
        for budget in [0, 24] {
            let idx = build(&bytes, budget);
            let nav = Navigator::with_cache_budget(&bytes, &idx, 512);
            let root = nav.root().unwrap();
            prop_assert_eq!(root.kind, kind_of(&tree));
            if let Some(c) = nav.container_of(&root) {
                check_children(&nav, &bytes, &c, &tree);
            }
        }
    }
}
