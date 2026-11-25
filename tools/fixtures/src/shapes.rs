//! Shape stress fixtures: `deep`, `wide`, `prim`, `string`.

use crate::emit::{Emitter, write_dec2, write_u64};
use crate::records::{Frame, Out, Stop, record_seed, write_records};
use crate::text::{WORDS, pick};
use fastrand::Rng;
use std::io;
use std::path::Path;

/// Nesting `depth` containers, alternating `{"k":"v","d": …}` and `[1, …]`.
/// Always minified (pretty indentation would grow quadratically with depth).
pub fn generate_deep(path: &Path, depth: u64) -> io::Result<u64> {
    let mut out = Out::create(path, depth * 22)?;
    let mut buf = Vec::with_capacity(1 << 20);
    for level in 0..depth {
        if level % 2 == 0 {
            buf.extend_from_slice(br#"{"k":"v","d":"#);
        } else {
            buf.extend_from_slice(b"[1,");
        }
        flush_if_big(&mut out, &mut buf)?;
    }
    buf.extend_from_slice(br#""bottom""#);
    for level in (0..depth).rev() {
        buf.push(if level % 2 == 0 { b'}' } else { b']' });
        flush_if_big(&mut out, &mut buf)?;
    }
    buf.push(b'\n');
    out.write(&buf)?;
    out.finish()
}

fn flush_if_big(out: &mut Out, buf: &mut Vec<u8>) -> io::Result<()> {
    if buf.len() >= 1 << 20 {
        out.write(buf)?;
        buf.clear();
    }
    Ok(())
}

/// One object with `keys` unique keys and mixed primitive / empty values.
pub fn generate_wide(path: &Path, keys: u64, seed: u64, minified: bool) -> io::Result<u64> {
    let pretty = !minified;
    let frame = if pretty {
        Frame {
            open: b"{\n  ",
            sep: b",\n  ",
            close: b"\n}\n",
        }
    } else {
        Frame {
            open: b"{",
            sep: b",",
            close: b"}",
        }
    };
    let mut out = Out::create(path, keys * 50)?;
    write_records(&mut out, &frame, Stop::Count(keys), |i, o| {
        let mut rng = Rng::with_seed(record_seed(seed, i));
        o.extend_from_slice(b"\"key_");
        write_u64(o, i);
        o.extend_from_slice(if pretty { b"\": " } else { b"\":" });
        let mut e = Emitter::new(o, pretty, 1);
        match rng.u8(..8) {
            0 => e.int(rng.i64(-1_000_000..1_000_000)),
            1 => e.dec2(rng.i64(-500..500), rng.u32(..100)),
            2 => e.bool(rng.bool()),
            3 => e.null(),
            4 => {
                e.begin_obj();
                e.end_obj();
            }
            5 => {
                e.begin_arr();
                e.end_arr();
            }
            _ => {
                let (a, b) = (pick(&mut rng, WORDS), pick(&mut rng, WORDS));
                e.str_with(|s| {
                    s.extend_from_slice(b"value ");
                    s.extend_from_slice(a.as_bytes());
                    s.push(b' ');
                    s.extend_from_slice(b.as_bytes());
                });
            }
        }
    })?;
    out.finish()
}

/// An array of `count` numbers (ints, negatives, decimals, exponents).
pub fn generate_prim(path: &Path, count: u64, seed: u64, minified: bool) -> io::Result<u64> {
    let mut out = Out::create(path, count * 11)?;
    write_records(
        &mut out,
        &Frame::array(!minified),
        Stop::Count(count),
        |i, o| {
            let mut rng = Rng::with_seed(record_seed(seed, i));
            match rng.u8(..5) {
                0 => {
                    o.push(b'-');
                    write_u64(o, rng.u64(..100_000_000));
                }
                1 => write_dec2(o, rng.i64(-10_000..10_000), rng.u32(..100)),
                2 => {
                    write_u64(o, rng.u64(1..10));
                    o.extend_from_slice(b".5e");
                    o.push(if rng.bool() { b'-' } else { b'+' });
                    write_u64(o, rng.u64(..30));
                }
                _ => write_u64(o, rng.u64(..100_000_000)),
            }
        },
    )?;
    out.finish()
}

/// `{"data": "<one string of about `size` bytes>"}` with escapes and Unicode.
pub fn generate_string(path: &Path, size: u64, seed: u64) -> io::Result<u64> {
    let mut out = Out::create(path, size)?;
    out.write(b"{\"data\": \"")?;
    // Body pieces of ~4 KiB (the record writer stops by size).
    let frame = Frame {
        open: b"",
        sep: b"",
        close: b"",
    };
    let body = size.saturating_sub(13);
    write_records(
        &mut out,
        &frame,
        Stop::Size {
            bytes: body,
            min_records: 1,
        },
        |i, o| {
            let mut rng = Rng::with_seed(record_seed(seed, i));
            let start = o.len();
            while o.len() - start < 4 << 10 {
                o.extend_from_slice(pick(&mut rng, WORDS).as_bytes());
                o.push(b' ');
            }
        },
    )?;
    out.write(b"\"}\n")?;
    out.finish()
}
