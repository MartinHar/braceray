//! "Go to" targets (step 08): a line, `line:col`, a byte offset (`@1.5g`) or a simple
//! path (`$.a.b[3]`); parsing and resolving them to a byte offset. Full JSONPath comes in
//! step 14.

use std::fmt;

use crate::error::Error;
use crate::job::CancelToken;
use crate::nav::{Child, Navigator};

/// Children read per batch while looking for a key (cancel checked between batches).
const KEY_BATCH: u64 = 4096;

/// A parsed go-to request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoTo {
    /// 1-based line, optional 1-based column (in characters).
    Line {
        /// The line.
        line: u64,
        /// The column.
        column: Option<u64>,
    },
    /// A byte offset.
    Offset(u64),
    /// A path from the top-level value.
    Path(Vec<Step>),
}

/// One step of a simple path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// `.name` or `["name"]`.
    Key(String),
    /// `[3]`.
    Index(u64),
}

/// Why a go-to request failed (the message is shown to the user).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoToError {
    /// The text isn't a line, offset or path.
    Syntax(String),
    /// Past the end of the file.
    OutOfRange(String),
    /// A path step doesn't exist.
    NotFound(String),
    /// The search was cancelled.
    Cancelled,
}

impl fmt::Display for GoToError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GoToError::Syntax(m) | GoToError::OutOfRange(m) | GoToError::NotFound(m) => {
                f.write_str(m)
            }
            GoToError::Cancelled => f.write_str("Cancelled."),
        }
    }
}

impl std::error::Error for GoToError {}

impl From<Error> for GoToError {
    fn from(_: Error) -> Self {
        GoToError::Cancelled
    }
}

/// Parses `12345`, `12,345:17`, `@88120551`, `@1.5g` / `@200m` / `@64k` (binary units),
/// or `$.a.b[3]` / `$["a b"][0]`.
///
/// # Example
/// ```
/// use braceray_core::goto::{parse, GoTo, Step};
/// assert_eq!(parse("12,345,678"), Ok(GoTo::Line { line: 12_345_678, column: None }));
/// assert_eq!(parse("7:3"), Ok(GoTo::Line { line: 7, column: Some(3) }));
/// assert_eq!(parse("@1.5g"), Ok(GoTo::Offset(1_610_612_736)));
/// assert_eq!(
///     parse(r#"$.users[2]["e-mail"]"#),
///     Ok(GoTo::Path(vec![Step::Key("users".into()), Step::Index(2), Step::Key("e-mail".into())]))
/// );
/// assert!(parse("hello").is_err());
/// ```
pub fn parse(input: &str) -> Result<GoTo, GoToError> {
    let s = input.trim();
    if let Some(rest) = s.strip_prefix('@') {
        return parse_size(rest).map(GoTo::Offset).ok_or_else(|| {
            GoToError::Syntax(format!(
                "“{rest}” isn't a byte offset (examples: @1024, @200m, @1.5g)."
            ))
        });
    }
    if s.starts_with('$') {
        return parse_path(s).map(GoTo::Path);
    }
    let (line, column) = match s.split_once(':') {
        Some((l, c)) => (l, Some(c)),
        None => (s, None),
    };
    let bad = || {
        GoToError::Syntax(format!(
            "“{s}” isn't a line (12345 or 12345:17), an offset (@200m) or a path ($.a.b[3])."
        ))
    };
    let line = parse_int(line).filter(|&l| l > 0).ok_or_else(bad)?;
    let column = match column {
        Some(c) => Some(parse_int(c).filter(|&c| c > 0).ok_or_else(bad)?),
        None => None,
    };
    Ok(GoTo::Line { line, column })
}

/// A non-negative integer with optional `,` / `_` / space separators.
fn parse_int(s: &str) -> Option<u64> {
    let digits: String = s
        .chars()
        .filter(|c| !matches!(c, ',' | '_' | ' '))
        .collect();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn parse_size(s: &str) -> Option<u64> {
    let mut s = s.trim();
    // "64KB" / "1.5gb": drop the B after a unit letter.
    if let Some(rest) = s.strip_suffix(['b', 'B'])
        && rest.ends_with(['k', 'K', 'm', 'M', 'g', 'G', 't', 'T'])
    {
        s = rest;
    }
    let (num, mult) = match s.char_indices().last()? {
        (i, 'k' | 'K') => (&s[..i], 1u64 << 10),
        (i, 'm' | 'M') => (&s[..i], 1 << 20),
        (i, 'g' | 'G') => (&s[..i], 1 << 30),
        (i, 't' | 'T') => (&s[..i], 1 << 40),
        _ => (s, 1),
    };
    let num = num.trim();
    if mult == 1 {
        return parse_int(num);
    }
    let clean: String = num.chars().filter(|c| !matches!(c, ',' | '_')).collect();
    if clean.is_empty() || !clean.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    let v: f64 = clean.parse().ok()?;
    let bytes = v * mult as f64;
    (bytes.is_finite() && bytes >= 0.0 && bytes < u64::MAX as f64).then_some(bytes as u64)
}

fn parse_path(s: &str) -> Result<Vec<Step>, GoToError> {
    let err = |what: &str| GoToError::Syntax(format!("Can't read the path “{s}”: {what}."));
    let b = s.as_bytes();
    let mut i = 1; // after '$'
    let mut steps = Vec::new();
    while i < b.len() {
        match b[i] {
            b'.' => {
                let start = i + 1;
                let mut j = start;
                while j < b.len() && !matches!(b[j], b'.' | b'[') {
                    j += 1;
                }
                if j == start {
                    return Err(err("a name is missing after “.”"));
                }
                steps.push(Step::Key(s[start..j].to_owned()));
                i = j;
            }
            b'[' => {
                let close = |from: usize| s[from..].find(']').map(|p| from + p);
                match b.get(i + 1) {
                    Some(&q @ (b'"' | b'\'')) => {
                        let mut key = String::new();
                        let mut j = i + 2;
                        let mut chars = s[j..].char_indices();
                        let mut closed = false;
                        while let Some((k, c)) = chars.next() {
                            match c {
                                '\\' => {
                                    if let Some((_, e)) = chars.next() {
                                        key.push(e);
                                    }
                                }
                                c if c as u32 == u32::from(q) => {
                                    j += k + 1;
                                    closed = true;
                                    break;
                                }
                                c => key.push(c),
                            }
                        }
                        if !closed || b.get(j) != Some(&b']') {
                            return Err(err("a quoted name isn't closed with ”]”"));
                        }
                        steps.push(Step::Key(key));
                        i = j + 1;
                    }
                    _ => {
                        let j = close(i + 1).ok_or_else(|| err("“[” isn't closed"))?;
                        let n = parse_int(&s[i + 1..j])
                            .ok_or_else(|| err("an index must be a number, like [3]"))?;
                        steps.push(Step::Index(n));
                        i = j + 1;
                    }
                }
            }
            _ => return Err(err("expected “.name” or “[index]”")),
        }
    }
    Ok(steps)
}

/// Offset of 1-based `line` and `column` (characters, clamped to the line's end).
///
/// # Errors
/// [`GoToError::OutOfRange`] past the last line.
pub fn resolve_line(nav: &Navigator<'_>, line: u64, column: Option<u64>) -> Result<u64, GoToError> {
    let bytes = nav.bytes();
    let lines = nav.index().lines();
    let start = lines.line_offset(bytes, line).ok_or_else(|| {
        GoToError::OutOfRange(format!(
            "Line {} is past the end: the file has {} lines.",
            crate::index::group_digits(line),
            crate::index::group_digits(lines.line_count())
        ))
    })?;
    let Some(col) = column.filter(|&c| c > 1) else {
        return Ok(start);
    };
    // Walk `col - 1` characters, stopping at the end of the line.
    let mut p = start as usize;
    let mut left = col - 1;
    while left > 0 && p < bytes.len() && bytes[p] != b'\n' {
        p += 1;
        while p < bytes.len() && bytes[p] & 0xC0 == 0x80 {
            p += 1;
        }
        left -= 1;
    }
    Ok(p as u64)
}

/// Follows `steps` from the top-level value; returns the tree address (child indices)
/// and the child reached (`None` for the empty path, which is the top-level value).
/// Looking up a key scans the object's children (cancellable; run it on a worker).
///
/// # Errors
/// [`GoToError::NotFound`] naming the first missing step, or [`GoToError::Cancelled`].
pub fn resolve_path(
    nav: &Navigator<'_>,
    steps: &[Step],
    cancel: &CancelToken,
) -> Result<(Vec<u64>, Option<Child>), GoToError> {
    let bytes = nav.bytes();
    let mut address = Vec::with_capacity(steps.len());
    let mut current = nav.root();
    let mut shown = String::from("$");
    for step in steps {
        let container = current.as_ref().and_then(|c| nav.container_of(c));
        let not_found = |what: String| GoToError::NotFound(what);
        let Some(c) = container else {
            return Err(not_found(format!("{shown} isn't an object or array.")));
        };
        let (idx, child) =
            match step {
                Step::Index(i) => {
                    let child = nav
                        .children(&c, *i..*i + 1)
                        .pop()
                        .ok_or_else(|| not_found(format!("{shown} has no element [{i}].")))?;
                    (*i, child)
                }
                Step::Key(name) => {
                    let mut found = None;
                    let mut from = 0;
                    loop {
                        cancel.check()?;
                        let batch = nav.children(&c, from..from + KEY_BATCH);
                        if batch.is_empty() {
                            break;
                        }
                        if let Some((k, child)) = batch.iter().enumerate().find(|(_, ch)| {
                            ch.key.is_some_and(|k| key_matches(k.slice(bytes), name))
                        }) {
                            found = Some((from + k as u64, *child));
                            break;
                        }
                        from += batch.len() as u64;
                    }
                    found.ok_or_else(|| not_found(format!("{shown} has no key “{name}”.")))?
                }
            };
        match step {
            Step::Index(i) => shown.push_str(&format!("[{i}]")),
            Step::Key(k) => {
                shown.push('.');
                shown.push_str(k);
            }
        }
        address.push(idx);
        current = Some(child);
    }
    Ok((address, if steps.is_empty() { None } else { current }))
}

/// Compares a raw key (with quotes) to a decoded name.
fn key_matches(raw: &[u8], name: &str) -> bool {
    let body = raw
        .strip_prefix(b"\"")
        .and_then(|r| r.strip_suffix(b"\""))
        .unwrap_or(raw);
    if !body.contains(&b'\\') {
        return body == name.as_bytes();
    }
    serde_json::from_slice::<String>(raw).is_ok_and(|k| k == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{IndexOptions, build_bytes};
    use crate::job::Progress;

    #[test]
    fn parse_forms() {
        assert_eq!(
            parse(" 42 "),
            Ok(GoTo::Line {
                line: 42,
                column: None
            })
        );
        assert_eq!(
            parse("1_000:2"),
            Ok(GoTo::Line {
                line: 1000,
                column: Some(2)
            })
        );
        assert_eq!(parse("@0"), Ok(GoTo::Offset(0)));
        assert_eq!(parse("@200m"), Ok(GoTo::Offset(200 << 20)));
        assert_eq!(parse("@64KB"), Ok(GoTo::Offset(64 << 10)));
        assert_eq!(parse("@1,024"), Ok(GoTo::Offset(1024)));
        assert_eq!(parse("$"), Ok(GoTo::Path(vec![])));
        assert_eq!(
            parse(r#"$['a\'b'].c[10]"#),
            Ok(GoTo::Path(vec![
                Step::Key("a'b".into()),
                Step::Key("c".into()),
                Step::Index(10)
            ]))
        );
        for bad in [
            "", "0", "1:0", "x", "@", "@1.5", "@-1m", "$.", "$[", "$[x]", "$[\"a\"", "$a",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn resolve() {
        let bytes = "{\"a\": [10, {\"é\\n\": 2}],\n \"b\": \"xÅy\"\n}".as_bytes();
        let index = build_bytes(
            bytes,
            &IndexOptions::default(),
            &CancelToken::new(),
            &Progress::default(),
        )
        .unwrap();
        let nav = Navigator::new(bytes, &index);
        let cancel = CancelToken::new();
        let (addr, child) = resolve_path(
            &nav,
            &[
                Step::Key("a".into()),
                Step::Index(1),
                Step::Key("é\n".into()),
            ],
            &cancel,
        )
        .unwrap();
        assert_eq!(addr, [0, 1, 0]);
        assert_eq!(child.unwrap().value.slice(bytes), b"2");
        assert_eq!(resolve_path(&nav, &[], &cancel).unwrap(), (vec![], None));
        let err = resolve_path(&nav, &[Step::Key("a".into()), Step::Index(5)], &cancel);
        assert_eq!(err.unwrap_err().to_string(), "$.a has no element [5].");
        let err = resolve_path(
            &nav,
            &[Step::Key("b".into()), Step::Key("x".into())],
            &cancel,
        );
        assert_eq!(
            err.unwrap_err().to_string(),
            "$.b isn't an object or array."
        );

        assert_eq!(resolve_line(&nav, 1, None), Ok(0));
        let line2 = bytes.iter().position(|&b| b == b'\n').unwrap() as u64 + 1;
        assert_eq!(resolve_line(&nav, 2, None), Ok(line2));
        // Column counts characters: `"xÅy"` → col 10 is the `y` after the 2-byte `Å`.
        let y = bytes.iter().position(|&b| b == b'y').unwrap() as u64;
        assert_eq!(resolve_line(&nav, 2, Some(10)), Ok(y));
        // Past the line's end: clamped to its newline.
        let nl2 = line2
            + bytes[line2 as usize..]
                .iter()
                .position(|&b| b == b'\n')
                .unwrap() as u64;
        assert_eq!(resolve_line(&nav, 2, Some(1000)), Ok(nl2));
        assert!(matches!(
            resolve_line(&nav, 4, None),
            Err(GoToError::OutOfRange(_))
        ));
    }
}
