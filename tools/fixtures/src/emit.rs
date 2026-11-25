//! A tiny streaming JSON emitter that writes straight into a `Vec<u8>`.
//!
//! It handles separators and indentation only. String contents are passed in
//! **already escaped** (`raw` JSON string bodies), because the generators build
//! escapes on purpose to exercise every escape type.

/// Streaming emitter. `pretty` uses a 2-space indent.
pub struct Emitter<'a> {
    out: &'a mut Vec<u8>,
    pretty: bool,
    depth: usize,
    /// No element written yet in the current container.
    first: bool,
    /// A key was just written; the next value needs no separator.
    after_key: bool,
}

impl<'a> Emitter<'a> {
    /// Starts emitting at `depth` (the indent level of the first value).
    pub fn new(out: &'a mut Vec<u8>, pretty: bool, depth: usize) -> Self {
        Self {
            out,
            pretty,
            depth,
            first: true,
            after_key: true,
        }
    }

    fn indent(&mut self) {
        self.out.push(b'\n');
        for _ in 0..self.depth {
            self.out.extend_from_slice(b"  ");
        }
    }

    fn sep(&mut self) {
        if self.after_key {
            self.after_key = false;
            return;
        }
        if !self.first {
            self.out.push(b',');
        }
        if self.pretty {
            self.indent();
        }
        self.first = false;
    }

    pub fn begin_obj(&mut self) {
        self.sep();
        self.out.push(b'{');
        self.depth += 1;
        self.first = true;
    }

    pub fn end_obj(&mut self) {
        self.close(b'}');
    }

    pub fn begin_arr(&mut self) {
        self.sep();
        self.out.push(b'[');
        self.depth += 1;
        self.first = true;
    }

    pub fn end_arr(&mut self) {
        self.close(b']');
    }

    fn close(&mut self, c: u8) {
        self.depth -= 1;
        if !self.first && self.pretty {
            self.indent();
        }
        self.out.push(c);
        self.first = false;
    }

    /// Writes an object key (must not need escaping beyond what `k` contains).
    pub fn key(&mut self, k: &str) {
        self.sep();
        self.out.push(b'"');
        self.out.extend_from_slice(k.as_bytes());
        self.out
            .extend_from_slice(if self.pretty { b"\": " } else { b"\":" });
        self.after_key = true;
    }

    /// Writes a string whose body is already valid escaped JSON.
    pub fn str_raw(&mut self, body: &str) {
        self.sep();
        self.out.push(b'"');
        self.out.extend_from_slice(body.as_bytes());
        self.out.push(b'"');
    }

    /// Writes a string built by `f` directly into the output buffer.
    pub fn str_with(&mut self, f: impl FnOnce(&mut Vec<u8>)) {
        self.sep();
        self.out.push(b'"');
        f(self.out);
        self.out.push(b'"');
    }

    /// Writes a number (or other bare token) built by `f` directly into the output.
    pub fn num_with(&mut self, f: impl FnOnce(&mut Vec<u8>)) {
        self.sep();
        f(self.out);
    }

    pub fn int(&mut self, v: i64) {
        self.sep();
        self.out
            .extend_from_slice(itoa::Buffer::new().format(v).as_bytes());
    }

    /// Writes `whole.frac` with exactly two fractional digits (`frac < 100`).
    pub fn dec2(&mut self, whole: i64, frac: u32) {
        self.sep();
        write_dec2(self.out, whole, frac);
    }

    /// Writes a raw token (number in exponent form, `true`, `null`, …).
    pub fn raw(&mut self, token: &str) {
        self.sep();
        self.out.extend_from_slice(token.as_bytes());
    }

    pub fn bool(&mut self, v: bool) {
        self.raw(if v { "true" } else { "false" });
    }

    pub fn null(&mut self) {
        self.raw("null");
    }
}

/// Appends `whole.ff` (two fractional digits) without allocating.
pub fn write_dec2(out: &mut Vec<u8>, whole: i64, frac: u32) {
    out.extend_from_slice(itoa::Buffer::new().format(whole).as_bytes());
    out.push(b'.');
    out.push(b'0' + (frac / 10 % 10) as u8);
    out.push(b'0' + (frac % 10) as u8);
}

/// Appends an unsigned integer.
pub fn write_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(itoa::Buffer::new().format(v).as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(pretty: bool) -> String {
        let mut out = Vec::new();
        let mut e = Emitter::new(&mut out, pretty, 0);
        e.begin_obj();
        e.key("a");
        e.int(-1);
        e.key("b");
        e.begin_arr();
        e.dec2(12, 5);
        e.bool(true);
        e.null();
        e.end_arr();
        e.key("e");
        e.begin_obj();
        e.end_obj();
        e.key("s");
        e.str_raw("x\\\"y");
        e.end_obj();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn minified() {
        assert_eq!(
            sample(false),
            r#"{"a":-1,"b":[12.05,true,null],"e":{},"s":"x\"y"}"#
        );
    }

    #[test]
    fn pretty() {
        let expected = "{\n  \"a\": -1,\n  \"b\": [\n    12.05,\n    true,\n    null\n  ],\n  \"e\": {},\n  \"s\": \"x\\\"y\"\n}";
        assert_eq!(sample(true), expected);
    }
}
