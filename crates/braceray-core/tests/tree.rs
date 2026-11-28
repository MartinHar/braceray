//! Tree row model against a serde walk of the same document with the same grouping rules.

mod common;

use std::collections::HashSet;
use std::time::Instant;

use braceray_core::index::{self, Index, IndexOptions};
use braceray_core::nav::{Navigator, ValueKind};
use braceray_core::tree::preview::Preview;
use braceray_core::tree::{Expand, Row, RowItem, TreeModel};
use braceray_core::{CancelToken, Progress};
use common::*;
use proptest::prelude::*;
use serde_json::Value;

fn build(bytes: &[u8], budget: u64) -> Index {
    let opts = IndexOptions {
        chunk_size: 4096,
        memory_budget: budget,
        threads: None,
    };
    index::build_bytes(bytes, &opts, &CancelToken::new(), &Progress::default()).unwrap()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Step {
    Child(u64),
    Group(u64, u64),
}

/// What a row shows, comparable between the model and the oracle.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Desc {
    Value {
        key: Option<String>,
        index: u64,
        kind: ValueKind,
    },
    Group(u64, u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ORow {
    depth: u32,
    expanded: bool,
    desc: Desc,
}

struct Oracle<'v> {
    root: &'v J,
    g: u64,
    expanded: HashSet<Vec<Step>>,
}

fn unit_for(n: u64, g: u64) -> u64 {
    let mut unit = 1;
    while n.div_ceil(unit) > g {
        unit *= g;
    }
    unit
}

fn kind(v: &J) -> ValueKind {
    match v {
        J::Null => ValueKind::Null,
        J::Bool(_) => ValueKind::Bool,
        J::Num(_) => ValueKind::Number,
        J::Str(_) => ValueKind::String,
        J::Arr(_) => ValueKind::Array,
        J::Obj(_) => ValueKind::Object,
    }
}

fn len_of(v: &J) -> u64 {
    match v {
        J::Arr(a) => a.len() as u64,
        J::Obj(o) => o.len() as u64,
        _ => 0,
    }
}

/// Key (raw, as written) and value of child `i`.
fn nth(v: &J, i: u64) -> (Option<String>, &J) {
    match v {
        J::Arr(a) => (None, &a[i as usize]),
        J::Obj(o) => {
            let (k, v) = &o[i as usize];
            (Some(k.clone()), v)
        }
        _ => unreachable!(),
    }
}

/// One oracle row plus what is needed to act on it.
struct Flat {
    row: ORow,
    path: Vec<Step>,
    expandable: bool,
    address: Vec<u64>,
    parent: Option<usize>,
}

impl Oracle<'_> {
    fn flatten(&self) -> Vec<Flat> {
        let mut out = Vec::new();
        let exp = self.expanded.contains(&Vec::new()) && len_of(self.root) > 0;
        out.push(Flat {
            row: ORow {
                depth: 0,
                expanded: exp,
                desc: Desc::Value {
                    key: None,
                    index: 0,
                    kind: kind(self.root),
                },
            },
            path: Vec::new(),
            expandable: len_of(self.root) > 0,
            address: Vec::new(),
            parent: None,
        });
        if exp {
            let n = len_of(self.root);
            self.items(self.root, 0, n, &[], &[], 1, 0, &mut out);
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn items(
        &self,
        v: &J,
        base: u64,
        len: u64,
        path: &[Step],
        address: &[u64],
        depth: u32,
        parent: usize,
        out: &mut Vec<Flat>,
    ) {
        let unit = unit_for(len, self.g);
        for j in 0..len.div_ceil(unit) {
            let at = out.len();
            if unit > 1 {
                let (a, b) = (base + j * unit, (base + (j + 1) * unit).min(base + len));
                let mut p = path.to_vec();
                p.push(Step::Group(a, b));
                let exp = self.expanded.contains(&p);
                out.push(Flat {
                    row: ORow {
                        depth,
                        expanded: exp,
                        desc: Desc::Group(a, b),
                    },
                    path: p.clone(),
                    expandable: true,
                    address: address.to_vec(),
                    parent: Some(parent),
                });
                if exp {
                    self.items(v, a, b - a, &p, address, depth + 1, at, out);
                }
            } else {
                let i = base + j;
                let (key, child) = nth(v, i);
                let mut p = path.to_vec();
                p.push(Step::Child(i));
                let mut addr = address.to_vec();
                addr.push(i);
                let n = len_of(child);
                let exp = n > 0 && self.expanded.contains(&p);
                out.push(Flat {
                    row: ORow {
                        depth,
                        expanded: exp,
                        desc: Desc::Value {
                            key,
                            index: i,
                            kind: kind(child),
                        },
                    },
                    path: p.clone(),
                    expandable: n > 0,
                    address: addr.clone(),
                    parent: Some(parent),
                });
                if exp {
                    self.items(child, 0, n, &p, &addr, depth + 1, at, out);
                }
            }
        }
    }

    fn toggle(&mut self, flat: &Flat) {
        if flat.row.expanded {
            self.expanded.retain(|p| !p.starts_with(&flat.path));
        } else if flat.expandable {
            self.expanded.insert(flat.path.clone());
        }
    }
}

fn describe(bytes: &[u8], row: &Row) -> ORow {
    let desc = match &row.item {
        RowItem::Value(v) => Desc::Value {
            key: v.child.key.map(|k| {
                let raw = k.slice(bytes);
                String::from_utf8_lossy(&raw[1..raw.len() - 1]).into_owned()
            }),
            index: v.index,
            kind: v.child.kind,
        },
        RowItem::Group { range, .. } => Desc::Group(range.start, range.end),
        RowItem::Loading { .. } => panic!("small documents are always ready"),
    };
    ORow {
        depth: row.depth,
        expanded: row.expanded,
        desc,
    }
}

fn model_rows(nav: &Navigator, bytes: &[u8], tree: &TreeModel) -> Vec<ORow> {
    let rows = tree.rows(nav, 0..tree.row_count());
    assert!(rows.pending.is_empty());
    rows.rows.iter().map(|r| describe(bytes, r)).collect()
}

fn expanded_addresses(tree: &TreeModel, nav: &Navigator) -> HashSet<Vec<u64>> {
    let rows = tree.rows(nav, 0..tree.row_count()).rows;
    (0..tree.row_count())
        .filter(|&r| {
            rows[r as usize].expanded && matches!(rows[r as usize].item, RowItem::Value(_))
        })
        .map(|r| tree.address(r).unwrap())
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn rows_match_a_serde_walk(
        tree in json_tree(),
        layout in layout(),
        g in 2u64..5,
        ops in prop::collection::vec(any::<prop::sample::Index>(), 1..40),
    ) {
        let (bytes, _) = serialize(&tree, &layout);
        for budget in [0, 24] {
            let idx = build(&bytes, budget);
            let nav = Navigator::new(&bytes, &idx);
            let mut model = TreeModel::new(&nav, g);
            let mut oracle = Oracle { root: &tree, g, expanded: HashSet::new() };
            for op in &ops {
                let flat = oracle.flatten();
                let r = op.index(flat.len());
                oracle.toggle(&flat[r]);
                model.toggle(&nav, r as u64);
                let flat = oracle.flatten();
                let want: Vec<ORow> = flat.iter().map(|f| f.row.clone()).collect();
                prop_assert_eq!(model.row_count(), want.len() as u64);
                prop_assert_eq!(model_rows(&nav, &bytes, &model), want);
            }
            // Addresses, parents and depths of every row.
            let flat = oracle.flatten();
            for (r, f) in flat.iter().enumerate() {
                prop_assert_eq!(model.address(r as u64).unwrap(), f.address.clone());
                prop_assert_eq!(model.parent(r as u64), f.parent.map(|p| p as u64));
                prop_assert_eq!(model.depth(r as u64), Some(f.row.depth));
                // Ancestors: the parent chain, value rows only, root first.
                let mut want = Vec::new();
                let mut at = Some(r);
                while let Some(i) = at {
                    if matches!(flat[i].row.desc, Desc::Value { .. }) {
                        want.push(i as u64);
                    }
                    at = flat[i].parent;
                }
                want.reverse();
                prop_assert_eq!(model.ancestors(r as u64), want);
            }
            // Any range of rows equals the same slice of all rows.
            let all = model_rows(&nav, &bytes, &model);
            let n = all.len() as u64;
            for (a, b) in [(0, n / 2), (n / 3, n), (n - 1, n + 5)] {
                let part: Vec<ORow> = model.rows(&nav, a..b).rows.iter().map(|r| describe(&bytes, r)).collect();
                prop_assert_eq!(&part[..], &all[a as usize..b.min(n) as usize]);
            }
            // Restoring the expansion state into a fresh model gives the same rows.
            let mut again = TreeModel::new(&nav, g);
            prop_assert_eq!(again.restore(&nav, &model.expansions()), 0);
            prop_assert_eq!(model_rows(&nav, &bytes, &again), all.clone());
            // With another group size, the same values are expanded.
            let mut other = TreeModel::new(&nav, g + 3);
            other.restore(&nav, &model.expansions());
            prop_assert_eq!(expanded_addresses(&other, &nav), expanded_addresses(&model, &nav));
            // Revealing any row's address in a fresh model reaches the same value.
            for (r, f) in flat.iter().enumerate() {
                if !matches!(f.row.desc, Desc::Value { .. }) {
                    continue;
                }
                let mut fresh = TreeModel::new(&nav, g);
                let at = fresh.reveal(&nav, &f.address).unwrap();
                let got = describe(&bytes, &fresh.rows(&nav, at..at + 1).rows[0]);
                prop_assert_eq!(got.desc, f.row.desc.clone(), "row {}", r);
                prop_assert_eq!(fresh.address(at).unwrap(), f.address.clone());
            }
        }
    }

    /// Expansions made on a preview carry over to the full index.
    #[test]
    fn preview_expansions_carry_over(
        tree in json_tree(),
        layout in layout(),
        cut in any::<prop::sample::Index>(),
        ops in prop::collection::vec(any::<prop::sample::Index>(), 1..20),
    ) {
        let (bytes, _) = serialize(&tree, &layout);
        let preview = Preview::build(&bytes, cut.index(bytes.len() + 1), &CancelToken::new()).unwrap();
        let cache = braceray_core::nav::NavCache::default();
        let pnav = preview.navigator(&bytes, &cache);
        let mut p = TreeModel::new(&pnav, 3);
        for op in &ops {
            let n = p.row_count();
            if n == 0 {
                break;
            }
            p.toggle(&pnav, op.index(n as usize) as u64);
        }
        let idx = build(&bytes, 0);
        let nav = Navigator::new(&bytes, &idx);
        let mut full = TreeModel::new(&nav, 3);
        prop_assert_eq!(full.restore(&nav, &p.expansions()), 0);
        let want = expanded_addresses(&p, &pnav);
        let got = expanded_addresses(&full, &nav);
        prop_assert!(want.is_subset(&got), "{:?} ⊄ {:?}", want, got);
    }
}

#[test]
fn golden_fixtures_expand_fully() {
    let dir = fixture("valid");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let bytes = std::fs::read(&path).unwrap();
        let value = serde_value(&bytes);
        let idx = build(&bytes, 0);
        let nav = Navigator::new(&bytes, &idx);
        let mut model = TreeModel::new(&nav, 1000);
        let done = model.expand_all(&nav, 0, u32::MAX, u64::MAX);
        assert!(
            done.complete && done.pending.is_none(),
            "{}",
            path.display()
        );
        // Every container row is expanded: rows = number of values in the document.
        fn values(v: &Value) -> u64 {
            1 + match v {
                Value::Array(a) => a.iter().map(values).sum(),
                Value::Object(o) => o.values().map(values).sum(),
                _ => 0,
            }
        }
        assert_eq!(model.row_count(), values(&value), "{}", path.display());
    }
}

#[test]
fn expand_all_respects_levels_and_budget() {
    let bytes = br#"[[1, [2, [3]]], [4, [5]], 6]"#;
    let idx = build(bytes, 0);
    let nav = Navigator::new(bytes, &idx);
    let mut model = TreeModel::new(&nav, 1000);
    let one = model.expand_all(&nav, 0, 1, u64::MAX);
    assert_eq!((one.added, one.complete, model.row_count()), (3, true, 4));
    let two = model.expand_all(&nav, 0, 2, u64::MAX);
    assert_eq!((two.added, model.row_count()), (4, 8));
    let mut model = TreeModel::new(&nav, 1000);
    let partial = model.expand_all(&nav, 0, u32::MAX, 4);
    assert!(!partial.complete && partial.added >= 4);
    let rest = model.expand_all(&nav, 0, u32::MAX, u64::MAX);
    assert!(rest.complete);
    assert_eq!(model.row_count(), 12);
}

#[test]
fn big_array_groups_and_loading_rows() {
    let n = 2_500_000u64;
    let mut bytes = b"[".to_vec();
    for i in 0..n {
        bytes.extend_from_slice(i.to_string().as_bytes());
        bytes.push(b',');
    }
    *bytes.last_mut().unwrap() = b']';
    let idx = build(&bytes, 0);
    let nav = Navigator::new(&bytes, &idx);
    let mut model = TreeModel::new(&nav, 1000);
    let t = Instant::now();
    // 2.5 M children → 3 groups of up to 1,000,000 (nested above 1000²).
    assert_eq!(model.expand(&nav, 0), Expand::Added(3));
    eprintln!("expand 2.5M array: {:?}", t.elapsed());
    assert!(t.elapsed().as_millis() < 50);
    let rows = model.rows(&nav, 0..4).rows;
    assert!(
        matches!(&rows[3].item, RowItem::Group { range, .. } if *range == (2_000_000..2_500_000))
    );
    assert_eq!(model.expand(&nav, 2), Expand::Added(1000)); // [1,000,000 … 1,999,999]
    assert_eq!(model.expand(&nav, 3 + 456), Expand::Added(1000)); // [1,456,000 … 1,456,999]
    let probe = 3 + 456 + 1 + 7;
    let rows = model.rows(&nav, probe..probe + 1);
    assert!(matches!(
        rows.rows[0].item,
        RowItem::Loading {
            index: 1_456_007,
            ..
        }
    ));
    assert_eq!(rows.pending.len(), 1);
    nav.prepare(&rows.pending[0], &CancelToken::new()).unwrap();
    let row = &model.rows(&nav, probe..probe + 1).rows[0];
    let RowItem::Value(v) = &row.item else {
        panic!("{row:?}")
    };
    assert_eq!(v.child.value.slice(&bytes), b"1456007");
    assert_eq!(model.address(probe), Some(vec![1_456_007]));
    let mut fresh = TreeModel::new(&nav, 1000);
    assert_eq!(fresh.reveal(&nav, &[1_456_007]), Ok(probe));
    // Collapsing the outer group removes its 2,000 rows and merges the segments again.
    assert_eq!(model.collapse(2), 2000);
    assert_eq!(model.segment_count(), 2);
}

#[test]
fn deep_100k_by_keyboard() {
    let depth = 100_000;
    let mut bytes = vec![b'['; depth];
    bytes.extend(std::iter::repeat_n(b']', depth));
    let idx = build(&bytes, 0);
    let nav = Navigator::new(&bytes, &idx);
    let mut model = TreeModel::new(&nav, 1000);
    let t = Instant::now();
    // "→" on the last row, 100,000 times.
    let mut row = 0;
    loop {
        match model.expand(&nav, row) {
            Expand::Added(1) => row += 1,
            Expand::NotExpandable => break,
            other => panic!("{other:?} at {row}"),
        }
    }
    eprintln!("expand 100k levels: {:?}", t.elapsed());
    assert_eq!(model.row_count(), depth as u64);
    assert_eq!(model.depth(row), Some(depth as u32 - 1));
    assert_eq!(model.address(row).unwrap().len(), depth - 1);
    let t = Instant::now();
    let exp = model.expansions();
    let mut again = TreeModel::new(&nav, 1000);
    assert_eq!(again.restore(&nav, &exp), 0);
    eprintln!("capture + restore 100k levels: {:?}", t.elapsed());
    assert_eq!(again, model);
    // "←" back up to the root.
    let mut r = row;
    while let Some(p) = model.parent(r) {
        r = p;
    }
    assert_eq!(r, 0);
    assert_eq!(model.collapse(0), depth as u64 - 1);
}
