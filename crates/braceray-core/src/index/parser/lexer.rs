//! Byte-level scanners of the reference parser: strings, numbers, literals.

use super::{Parser, Stop, TICK, fail};

impl Parser<'_> {
    pub(super) fn string(&mut self) -> Result<(), Stop> {
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

    pub(super) fn number(&mut self) -> Result<(), Stop> {
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

    pub(super) fn literal(
        &mut self,
        word: &'static [u8],
        expected: &'static str,
    ) -> Result<(), Stop> {
        for &c in word {
            if self.peek() != Some(c) {
                return fail(self.i, expected);
            }
            self.i += 1;
        }
        Ok(())
    }

    #[inline]
    pub(super) fn peek(&self) -> Option<u8> {
        self.bytes.get(self.i).copied()
    }

    /// Every [`TICK`] bytes: check cancellation and report progress.
    #[inline]
    pub(super) fn tick(&mut self) -> Result<(), Stop> {
        if self.i >= self.next_tick {
            if self.cancel.is_cancelled() {
                return Err(Stop::Cancelled);
            }
            let done = self.i.min(self.bytes.len());
            if let Some(p) = self.progress {
                p.add(done.saturating_sub(self.reported) as u64);
            }
            self.reported = self.reported.max(done);
            self.next_tick = self.i + TICK;
        }
        Ok(())
    }
}
