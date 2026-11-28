//! Text of a value for the clipboard or a file: as in the file, pretty, minified, or a
//! string's decoded contents. One pass over the value's bytes with no DOM and no
//! recursion; cancel and progress are checked every [`CHECK_EVERY`] bytes.
//!
//! The formatter only moves whitespace: strings, numbers and literals are copied
//! byte for byte, so the output parses to the same value as the input. Its pretty layout
//! is `serde_json`'s (`": "`, empty containers as `{}` / `[]`), so a compact serde
//! document comes out exactly as `serde_json::to_string_pretty` with the same indent.

use std::io::Write;

use crate::error::Error;
use crate::job::{CancelToken, Progress};
use crate::nav::Span;
use crate::path::{CHECK_EVERY, decode_escapes, strip_quotes};

/// How to write a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CopyStyle {
    /// The bytes as they are in the file.
    Raw,
    /// One member or element per line, `indent` spaces per level.
    Pretty {
        /// Spaces per level.
        indent: u8,
    },
    /// No whitespace outside strings.
    Minified,
    /// A string's text with escapes decoded, without quotes (other values: as `Raw`).
    StringContents,
}

/// The text of `bytes[span]` in `style` (invalid UTF-8 becomes U+FFFD). `progress`
/// counts input bytes (its total is reset to the span length).
///
/// # Example
/// ```
/// use braceray_core::copy::{render, CopyStyle};
/// use braceray_core::nav::Span;
/// use braceray_core::{CancelToken, Progress};
///
/// let bytes = br#"{"a": [1, {}], "s": "x\ty"}"#;
/// let all = Span { start: 0, end: bytes.len() as u64 };
/// let (c, p) = (CancelToken::new(), Progress::default());
/// assert_eq!(render(bytes, all, CopyStyle::Minified, &c, &p)?, r#"{"a":[1,{}],"s":"x\ty"}"#);
/// assert_eq!(
///     render(bytes, all, CopyStyle::Pretty { indent: 2 }, &c, &p)?,
///     "{\n  \"a\": [\n    1,\n    {}\n  ],\n  \"s\": \"x\\ty\"\n}"
/// );
/// let s = Span { start: 20, end: 26 };
/// assert_eq!(render(bytes, s, CopyStyle::StringContents, &c, &p)?, "x\ty");
/// # Ok::<(), braceray_core::Error>(())
/// ```
pub fn render(
    bytes: &[u8],
    span: Span,
    style: CopyStyle,
    cancel: &CancelToken,
    progress: &Progress,
) -> Result<String, Error> {
    let src = span.slice(bytes);
    let mut out = Vec::with_capacity(match style {
        CopyStyle::Pretty { .. } => src.len() + src.len() / 4,
        _ => src.len(),
    });
    write_into(src, style, &mut out, cancel, progress)?;
    Ok(match String::from_utf8(out) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    })
}

/// Writes `bytes[span]` in `style` to `out` (for "Export…"), in pieces of about
/// [`CHECK_EVERY`] bytes. Write errors name `path`.
///
/// # Example
/// ```
/// use braceray_core::copy::{write_to, CopyStyle};
/// use braceray_core::nav::Span;
/// use braceray_core::{CancelToken, Progress};
///
/// let bytes = br#"[1, 2]"#;
/// let mut out = Vec::new();
/// let all = Span { start: 0, end: 6 };
/// let (c, p) = (CancelToken::new(), Progress::default());
/// write_to(bytes, all, CopyStyle::Raw, &mut out, "out.json".as_ref(), &c, &p)?;
/// assert_eq!(out, bytes);
/// # Ok::<(), braceray_core::Error>(())
/// ```
pub fn write_to(
    bytes: &[u8],
    span: Span,
    style: CopyStyle,
    out: &mut impl Write,
    path: &std::path::Path,
    cancel: &CancelToken,
    progress: &Progress,
) -> Result<(), Error> {
    let src = span.slice(bytes);
    let io = |source| Error::Write {
        path: path.to_path_buf(),
        source,
    };
    if style == CopyStyle::Raw {
        // Straight from the mapping.
        progress.reset(src.len() as u64);
        for chunk in src.chunks(CHECK_EVERY) {
            cancel.check()?;
            out.write_all(chunk).map_err(io)?;
            progress.add(chunk.len() as u64);
        }
        return out.flush().map_err(io);
    }
    let mut buf = Vec::with_capacity(CHECK_EVERY.min(src.len()) + 1024);
    format(src, style, &mut buf, cancel, progress, &mut |buf| {
        out.write_all(buf).map_err(io)?;
        buf.clear();
        Ok(())
    })?;
    out.write_all(&buf).map_err(io)?;
    out.flush().map_err(io)
}

fn write_into(
    src: &[u8],
    style: CopyStyle,
    out: &mut Vec<u8>,
    cancel: &CancelToken,
    progress: &Progress,
) -> Result<(), Error> {
    format(src, style, out, cancel, progress, &mut |_| Ok(()))
}

/// Formats `src` into `out`; every [`CHECK_EVERY`] input bytes: cancel check, progress,
/// then `flush(out)` (which may drain `out`).
fn format(
    src: &[u8],
    style: CopyStyle,
    out: &mut Vec<u8>,
    cancel: &CancelToken,
    progress: &Progress,
    flush: &mut dyn FnMut(&mut Vec<u8>) -> Result<(), Error>,
) -> Result<(), Error> {
    progress.reset(src.len() as u64);
    match style {
        CopyStyle::Raw => {
            for chunk in src.chunks(CHECK_EVERY) {
                cancel.check()?;
                out.extend_from_slice(chunk);
                progress.add(chunk.len() as u64);
            }
            Ok(())
        }
        CopyStyle::StringContents => {
            let trimmed = src.trim_ascii();
            if !trimmed.starts_with(b"\"") {
                return format(src, CopyStyle::Raw, out, cancel, progress, flush);
            }
            let mut last = 0;
            decode_escapes(strip_quotes(trimmed), out, |done| {
                cancel.check()?;
                progress.add((done - last) as u64);
                last = done;
                Ok(())
            })?;
            progress.add((src.len() - last) as u64);
            Ok(())
        }
        CopyStyle::Pretty { indent } => reformat(src, Some(indent), out, cancel, progress, flush),
        CopyStyle::Minified => reformat(src, None, out, cancel, progress, flush),
    }
}

/// The re-formatter: `indent` `None` is minified.
fn reformat(
    src: &[u8],
    indent: Option<u8>,
    out: &mut Vec<u8>,
    cancel: &CancelToken,
    progress: &Progress,
    flush: &mut dyn FnMut(&mut Vec<u8>) -> Result<(), Error>,
) -> Result<(), Error> {
    let width = indent.map(usize::from);
    let mut depth = 0usize;
    let mut p = 0usize;
    let mut next_check = CHECK_EVERY;
    let mut reported = 0usize;
    let newline = |out: &mut Vec<u8>, depth: usize| {
        if let Some(w) = width {
            out.push(b'\n');
            out.resize(out.len() + w * depth, b' ');
        }
    };
    while p < src.len() {
        if p >= next_check {
            cancel.check()?;
            progress.add((p - reported) as u64);
            reported = p;
            next_check = p + CHECK_EVERY;
            flush(out)?;
        }
        let b = src[p];
        match b {
            b' ' | b'\t' | b'\n' | b'\r' => p += 1,
            b'{' | b'[' => {
                out.push(b);
                let next = skip_ws(src, p + 1);
                let close = if b == b'{' { b'}' } else { b']' };
                if src.get(next) == Some(&close) {
                    out.push(close);
                    p = next + 1;
                } else {
                    depth += 1;
                    newline(out, depth);
                    p += 1;
                }
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                newline(out, depth);
                out.push(b);
                p += 1;
            }
            b',' => {
                out.push(b',');
                newline(out, depth);
                p += 1;
            }
            b':' => {
                out.push(b':');
                if width.is_some() {
                    out.push(b' ');
                }
                p += 1;
            }
            b'"' => {
                let end = string_end(src, p + 1);
                out.extend_from_slice(&src[p..end]);
                p = end;
            }
            _ => {
                let end = src[p..]
                    .iter()
                    .position(|&c| {
                        matches!(
                            c,
                            b' ' | b'\t'
                                | b'\n'
                                | b'\r'
                                | b'{'
                                | b'}'
                                | b'['
                                | b']'
                                | b':'
                                | b','
                                | b'"'
                        )
                    })
                    .map_or(src.len(), |q| p + q);
                out.extend_from_slice(&src[p..end]);
                p = end;
            }
        }
    }
    progress.add((src.len() - reported) as u64);
    Ok(())
}

fn skip_ws(src: &[u8], mut p: usize) -> usize {
    while p < src.len() && matches!(src[p], b' ' | b'\t' | b'\n' | b'\r') {
        p += 1;
    }
    p
}

/// One past the closing quote of the string whose body starts at `p` (the end of `src`
/// if it isn't closed).
fn string_end(src: &[u8], mut p: usize) -> usize {
    while let Some(rel) = memchr::memchr2(b'"', b'\\', &src[p.min(src.len())..]) {
        let i = p + rel;
        if src[i] == b'"' {
            return i + 1;
        }
        p = i + 2;
        if p >= src.len() {
            return src.len();
        }
    }
    src.len()
}

#[cfg(test)]
mod tests;
