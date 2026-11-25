//! `diff`: two `orders` files with N known, manifest-listed changes.

use crate::emit::write_dec2;
use crate::manifest::{DiffChange, DiffManifest, file_name, write_json};
use crate::orders::{Mutation, RecordFacts, order_record};
use crate::records::{Frame, Out, Stop, record_seed, write_records};
use crate::size::size_label;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

const KINDS: [Mutation; 4] = [
    Mutation::ChangeTotal,
    Mutation::FlipPaid,
    Mutation::AddCoupon,
    Mutation::RemoveNotes,
];

/// Writes `diff_a_<size>.json`, `diff_b_<size>.json` and `diff_manifest.json` into `dir`.
/// Both files have the same records; `changes` of them differ in B.
pub fn generate_diff(dir: &Path, size: u64, changes: u64, seed: u64) -> io::Result<DiffManifest> {
    let label = size_label(size);
    let path_a = dir.join(format!("diff_a_{label}.json"));
    let path_b = dir.join(format!("diff_b_{label}.json"));
    let frame = Frame::array(true);

    // Pass 1: A, sized by bytes (at least `changes` records).
    let mut out = Out::create(&path_a, size)?;
    let stop = Stop::Size {
        bytes: size,
        min_records: changes,
    };
    let records = write_records(&mut out, &frame, stop, |i, b| {
        order_record(seed, i, true, false, Mutation::None, b);
    })?;
    out.finish()?;

    // Changed records: evenly spread, distinct, with rotating kinds.
    let mutations: BTreeMap<u64, Mutation> = (0..changes)
        .map(|k| {
            let idx = (2 * k + 1) * records / (2 * changes.max(1));
            let kind = KINDS[(record_seed(seed, k) % KINDS.len() as u64) as usize];
            (idx, kind)
        })
        .collect();

    // Pass 2: B, exactly the same record count.
    let mut out = Out::create(&path_b, size)?;
    write_records(&mut out, &frame, Stop::Count(records), |i, b| {
        let m = mutations.get(&i).copied().unwrap_or(Mutation::None);
        order_record(seed, i, true, false, m, b);
    })?;
    out.finish()?;

    let changes = mutations
        .iter()
        .map(|(&i, &m)| describe(seed, i, m))
        .collect();
    let manifest = DiffManifest {
        file_a: file_name(&path_a),
        file_b: file_name(&path_b),
        seed,
        records,
        changes,
    };
    write_json(&dir.join("diff_manifest.json"), &manifest)?;
    Ok(manifest)
}

fn facts(seed: u64, i: u64, m: Mutation) -> RecordFacts {
    order_record(seed, i, true, false, m, &mut Vec::new())
}

fn money(cents: i64) -> String {
    let mut v = Vec::new();
    write_dec2(&mut v, cents / 100, (cents % 100) as u32);
    String::from_utf8_lossy(&v).into_owned()
}

fn describe(seed: u64, i: u64, m: Mutation) -> DiffChange {
    let a = facts(seed, i, Mutation::None);
    let (kind, field, va, vb) = match m {
        Mutation::ChangeTotal => (
            "value_changed",
            "total",
            Some(money(a.total_cents)),
            Some(money(a.total_cents + 100)),
        ),
        Mutation::FlipPaid => (
            "value_changed",
            "paid",
            Some(a.paid.to_string()),
            Some((!a.paid).to_string()),
        ),
        Mutation::AddCoupon => ("key_added", "coupon", None, Some("\"SAVE10\"".to_owned())),
        Mutation::RemoveNotes | Mutation::None => (
            "key_removed",
            "notes",
            Some(format!("\"{}\"", String::from_utf8_lossy(&a.notes))),
            None,
        ),
    };
    DiffChange {
        kind: kind.to_owned(),
        path: format!("$[{i}].{field}"),
        a: va,
        b: vb,
    }
}
