//! `ndjson`: one minified log record per line (JSON Lines).

use crate::emit::Emitter;
use crate::manifest::{RecordsManifest, manifest_path, write_json};
use crate::orders::{
    COMMON_NEEDLE, RARE_NEEDLE, RecordsOpts, estimate_records, is_common, rare_positions,
    records_manifest,
};
use crate::records::{Frame, Out, Stop, record_seed, write_records};
use crate::text::*;
use fastrand::Rng;
use std::io;
use std::path::Path;

/// Generates log record `i` followed by `\n`.
pub fn log_record(seed: u64, i: u64, rare: bool, out: &mut Vec<u8>) {
    let mut rng = Rng::with_seed(record_seed(seed, i));
    let mut e = Emitter::new(out, false, 0);
    e.begin_obj();
    e.key("seq");
    e.int(i as i64);
    e.key("ts");
    e.str_with(|o| write_timestamp(&mut rng, o));
    e.key("level");
    e.str_raw(pick(&mut rng, LEVELS));
    e.key("service");
    e.str_raw(pick(&mut rng, SERVICES));
    e.key("trace");
    e.str_with(|o| write_uuid(&mut rng, o));
    e.key("msg");
    let msg = pick(&mut rng, MESSAGES);
    let common = is_common(i);
    e.str_with(|o| {
        o.extend_from_slice(msg.as_bytes());
        if common {
            o.push(b' ');
            o.extend_from_slice(COMMON_NEEDLE.as_bytes());
        }
        if rare {
            o.push(b' ');
            o.extend_from_slice(RARE_NEEDLE.as_bytes());
        }
    });
    e.key("latency_ms");
    e.dec2(rng.i64(0..2000), rng.u32(..100));
    e.key("status");
    e.int([200, 200, 201, 204, 400, 404, 500][rng.usize(..7)]);
    e.key("user");
    if rng.u8(..4) == 0 {
        e.null();
    } else {
        e.begin_obj();
        e.key("id");
        e.int(rng.i64(1..1_000_000));
        e.key("name");
        let (f, l) = (pick(&mut rng, FIRST_NAMES), pick(&mut rng, LAST_NAMES));
        e.str_with(|o| {
            o.extend_from_slice(f.as_bytes());
            o.push(b' ');
            o.extend_from_slice(l.as_bytes());
        });
        e.end_obj();
    }
    e.key("tags");
    e.begin_arr();
    for _ in 0..rng.usize(..3) {
        e.str_raw(pick(&mut rng, TAGS));
    }
    e.end_arr();
    e.end_obj();
    out.push(b'\n');
}

/// Writes an NDJSON fixture plus `<file>.manifest.json`.
pub fn generate_ndjson(path: &Path, opts: RecordsOpts) -> io::Result<RecordsManifest> {
    let opts = RecordsOpts {
        minified: true,
        ..opts
    };
    let est = estimate_records(opts.size, false, |i, _, b| {
        log_record(opts.seed, i, false, b)
    });
    let rare = rare_positions(est);
    let mut out = Out::create(path, opts.size)?;
    let stop = Stop::Size {
        bytes: opts.size,
        min_records: rare.last().copied().unwrap_or(0) + 1,
    };
    let records = write_records(&mut out, &Frame::lines(), stop, |i, b| {
        log_record(opts.seed, i, rare.binary_search(&i).is_ok(), b)
    })?;
    let bytes = out.finish()?;
    let manifest = records_manifest(path, "ndjson", opts, bytes, records);
    write_json(&manifest_path(path), &manifest)?;
    Ok(manifest)
}
