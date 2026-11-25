//! `orders`: a top-level array of realistic order objects (≈ 400–1200 bytes each).

use crate::emit::{Emitter, write_dec2, write_u64};
use crate::manifest::{NeedleCount, RecordsManifest, file_name, manifest_path, write_json};
use crate::records::{Frame, Out, Stop, record_seed, write_records};
use crate::text::*;
use fastrand::Rng;
use std::io;
use std::path::Path;

/// Appears exactly this many times in every `orders` / `ndjson` file.
pub const RARE_NEEDLE: &str = "NEEDLE_RARE";
pub const RARE_COUNT: u64 = 17;
/// Appears once in every record whose index satisfies [`is_common`] (1 %).
pub const COMMON_NEEDLE: &str = "needle_common";

pub fn is_common(i: u64) -> bool {
    i % 100 == 50
}

/// Record indices that carry [`RARE_NEEDLE`], spread over `est_records`.
pub fn rare_positions(est_records: u64) -> Vec<u64> {
    let est = est_records.max(RARE_COUNT);
    (0..RARE_COUNT)
        .map(|k| (2 * k + 1) * est / (2 * RARE_COUNT))
        .collect()
}

/// A deliberate difference used by the `diff` fixture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutation {
    None,
    /// `total` gets +1.00.
    ChangeTotal,
    /// `paid` is negated.
    FlipPaid,
    /// A `"coupon": "SAVE10"` key is added at the end.
    AddCoupon,
    /// The `notes` key is removed.
    RemoveNotes,
}

/// Values of one record that the diff manifest needs to describe a change.
#[derive(Debug, Default)]
pub struct RecordFacts {
    pub total_cents: i64,
    pub paid: bool,
    pub notes: Vec<u8>,
}

/// Generates record `i` (rendered at depth 1) into `out`.
pub fn order_record(
    seed: u64,
    i: u64,
    pretty: bool,
    rare: bool,
    mutation: Mutation,
    out: &mut Vec<u8>,
) -> RecordFacts {
    let mut rng = Rng::with_seed(record_seed(seed, i));
    let mut facts = RecordFacts::default();
    let mut e = Emitter::new(out, pretty, 1);
    e.begin_obj();
    e.key("id");
    e.int(100_000 + i as i64);
    e.key("uuid");
    e.str_with(|o| write_uuid(&mut rng, o));

    e.key("customer");
    e.begin_obj();
    e.key("name");
    let (first, last) = (pick(&mut rng, FIRST_NAMES), pick(&mut rng, LAST_NAMES));
    e.str_with(|o| {
        o.extend_from_slice(first.as_bytes());
        o.push(b' ');
        o.extend_from_slice(last.as_bytes());
    });
    e.key("email");
    let (user, domain) = (pick(&mut rng, EMAIL_NAMES), pick(&mut rng, DOMAINS));
    e.str_with(|o| {
        o.extend_from_slice(user.as_bytes());
        o.push(b'.');
        write_u64(o, i);
        o.push(b'@');
        o.extend_from_slice(domain.as_bytes());
    });
    e.key("country");
    e.str_raw(pick(&mut rng, COUNTRIES));
    e.key("tags");
    e.begin_arr();
    for _ in 0..rng.usize(..4) {
        e.str_raw(pick(&mut rng, TAGS));
    }
    e.end_arr();
    e.end_obj();

    e.key("items");
    e.begin_arr();
    let mut total: i64 = 0;
    for _ in 0..rng.usize(1..=4) {
        e.begin_obj();
        e.key("sku");
        let sku = rng.u32(..100_000);
        e.str_with(|o| {
            o.extend_from_slice(b"SKU-");
            for d in [10_000, 1_000, 100, 10, 1] {
                o.push(b'0' + (sku / d % 10) as u8);
            }
        });
        e.key("title");
        e.str_raw(pick(&mut rng, TITLES));
        e.key("qty");
        let qty = rng.i64(1..=10);
        e.int(qty);
        e.key("price");
        let (whole, frac) = (rng.i64(0..500), rng.u32(..100));
        total += qty * (whole * 100 + frac as i64);
        match rng.u8(..4) {
            // Exponent forms: 12.50e0, 1250E-2.
            0 => e.num_with(|o| {
                write_dec2(o, whole, frac);
                o.extend_from_slice(b"e0");
            }),
            1 => e.num_with(|o| {
                write_u64(o, (whole * 100) as u64 + frac as u64);
                o.extend_from_slice(b"E-2");
            }),
            _ => e.dec2(whole, frac),
        }
        e.key("discount");
        match rng.u8(..4) {
            0 => e.dec2(-(rng.i64(0..20)), rng.u32(..100)),
            1 => e.raw("-2.5e-1"),
            _ => e.null(),
        }
        e.end_obj();
    }
    e.end_arr();

    if mutation == Mutation::ChangeTotal {
        total += 100;
    }
    facts.total_cents = total;
    e.key("total");
    e.dec2(total / 100, (total % 100) as u32);
    e.key("paid");
    let paid = rng.bool() ^ (mutation == Mutation::FlipPaid);
    facts.paid = paid;
    e.bool(paid);
    e.key("score");
    e.raw(if rng.bool() { "-3.2e-4" } else { "1.5E+3" });

    let common = is_common(i);
    let n1 = pick(&mut rng, NOTES);
    let n2 = pick(&mut rng, NOTES);
    if mutation != Mutation::RemoveNotes {
        e.key("notes");
        e.str_with(|o| {
            let start = o.len();
            o.extend_from_slice(n1.as_bytes());
            o.push(b' ');
            o.extend_from_slice(n2.as_bytes());
            if common {
                o.extend_from_slice(b" ");
                o.extend_from_slice(COMMON_NEEDLE.as_bytes());
            }
            if rare {
                o.extend_from_slice(b" ");
                o.extend_from_slice(RARE_NEEDLE.as_bytes());
            }
            facts.notes = o[start..].to_vec();
        });
    }
    e.key("meta");
    e.begin_obj();
    if rng.bool() {
        e.key("gift");
        e.bool(true);
        e.key("wrap");
        e.begin_arr();
        e.end_arr();
    }
    e.end_obj();
    e.key("created_at");
    e.str_with(|o| write_timestamp(&mut rng, o));
    if mutation == Mutation::AddCoupon {
        e.key("coupon");
        e.str_raw("SAVE10");
    }
    e.end_obj();
    facts
}

/// Average record size, from a deterministic sample (used to place needles).
pub fn estimate_records(
    target_bytes: u64,
    pretty: bool,
    make: impl Fn(u64, bool, &mut Vec<u8>),
) -> u64 {
    const SAMPLE: u64 = 256;
    let mut buf = Vec::new();
    for i in 0..SAMPLE {
        make(i, pretty, &mut buf);
    }
    let avg = (buf.len() as u64 / SAMPLE).max(1) + if pretty { 4 } else { 1 };
    (target_bytes / avg).max(1)
}

/// Options shared by `orders` and `ndjson`.
#[derive(Clone, Copy, Debug)]
pub struct RecordsOpts {
    pub size: u64,
    pub seed: u64,
    pub minified: bool,
}

/// Writes an `orders` fixture plus `<file>.manifest.json`.
pub fn generate_orders(path: &Path, opts: RecordsOpts) -> io::Result<RecordsManifest> {
    let pretty = !opts.minified;
    let est = estimate_records(opts.size, pretty, |i, p, b| {
        order_record(opts.seed, i, p, false, Mutation::None, b);
    });
    let rare = rare_positions(est);
    let mut out = Out::create(path, opts.size)?;
    let stop = Stop::Size {
        bytes: opts.size,
        min_records: rare.last().copied().unwrap_or(0) + 1,
    };
    let records = write_records(&mut out, &Frame::array(pretty), stop, |i, b| {
        let is_rare = rare.binary_search(&i).is_ok();
        order_record(opts.seed, i, pretty, is_rare, Mutation::None, b);
    })?;
    let bytes = out.finish()?;
    let manifest = records_manifest(path, "orders", opts, bytes, records);
    write_json(&manifest_path(path), &manifest)?;
    Ok(manifest)
}

/// Builds the needle manifest for a records file with `records` records.
pub fn records_manifest(
    path: &Path,
    kind: &str,
    opts: RecordsOpts,
    bytes: u64,
    records: u64,
) -> RecordsManifest {
    let common = records / 100 + u64::from(records % 100 > 50);
    RecordsManifest {
        file: file_name(path),
        kind: kind.to_owned(),
        seed: opts.seed,
        minified: opts.minified,
        bytes,
        records,
        needles: vec![
            NeedleCount {
                needle: RARE_NEEDLE.to_owned(),
                count: RARE_COUNT,
                count_case_insensitive: RARE_COUNT,
            },
            NeedleCount {
                needle: COMMON_NEEDLE.to_owned(),
                count: common,
                count_case_insensitive: common,
            },
            NeedleCount {
                needle: "needle".to_owned(),
                count: common,
                count_case_insensitive: common + RARE_COUNT,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rare_positions_are_distinct_and_sorted() {
        for est in [1, 17, 18, 100, 1_000_000] {
            let p = rare_positions(est);
            assert_eq!(p.len(), RARE_COUNT as usize);
            assert!(p.windows(2).all(|w| w[0] < w[1]), "{est}: {p:?}");
        }
    }

    #[test]
    fn record_is_valid_json_both_modes() {
        for pretty in [false, true] {
            for i in 0..200 {
                let mut b = Vec::new();
                order_record(7, i, pretty, i == 3, Mutation::None, &mut b);
                let v: serde_json::Value = serde_json::from_slice(&b)
                    .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&b)));
                assert!(v["items"].is_array());
            }
        }
    }

    #[test]
    fn record_size_is_realistic() {
        let mut b = Vec::new();
        for i in 0..1000 {
            order_record(1, i, false, false, Mutation::None, &mut b);
        }
        let avg = b.len() / 1000;
        assert!(
            (400..=1200).contains(&avg),
            "avg minified record {avg} bytes"
        );
    }

    #[test]
    fn mutations_only_touch_their_field() {
        let mut a = Vec::new();
        let fa = order_record(5, 9, false, false, Mutation::None, &mut a);
        for m in [
            Mutation::ChangeTotal,
            Mutation::FlipPaid,
            Mutation::AddCoupon,
            Mutation::RemoveNotes,
        ] {
            let mut b = Vec::new();
            let fb = order_record(5, 9, false, false, m, &mut b);
            let (va, vb): (serde_json::Value, serde_json::Value) = (
                serde_json::from_slice(&a).unwrap(),
                serde_json::from_slice(&b).unwrap(),
            );
            let mut va = va.as_object().unwrap().clone();
            let mut vb = vb.as_object().unwrap().clone();
            for k in ["total", "paid", "coupon", "notes"] {
                va.remove(k);
                vb.remove(k);
            }
            assert_eq!(va, vb, "{m:?}");
            match m {
                Mutation::ChangeTotal => assert_eq!(fb.total_cents, fa.total_cents + 100),
                Mutation::FlipPaid => assert_ne!(fb.paid, fa.paid),
                _ => {}
            }
        }
    }
}
