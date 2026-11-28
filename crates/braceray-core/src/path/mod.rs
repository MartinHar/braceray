//! Text forms of a [`PathSegment`] path: JSONPath (RFC 9535), jq, JavaScript and JSON
//! Pointer (RFC 6901).
//!
//! Keys are decoded from their JSON escapes first, then quoted for the target format.
//! The short dot form is used only for plain ASCII identifiers:
//!
//! | Format | Dot form when the key matches | Otherwise |
//! |---|---|---|
//! | JSONPath | `[A-Za-z_][A-Za-z0-9_]*` | `['…']` (`\'`, `\\`, `\n`, `\uXXXX`) |
//! | jq | `[A-Za-z_][A-Za-z0-9_]*`, not a jq keyword | `["…"]` (JSON escapes) |
//! | JavaScript | `[A-Za-z_$][A-Za-z0-9_$]*`, not a reserved word | `["…"]` (JSON escapes, plus U+2028 / U+2029) |
//! | JSON Pointer | — | `/key` with `~` → `~0`, `/` → `~1` |
//!
//! `$` isn't allowed in JSONPath's or jq's dot form (`$` starts a variable in jq), so
//! those formats quote keys that JavaScript writes with a dot.

mod decode;

#[cfg(test)]
mod tests;

pub use decode::decode_key;
pub(crate) use decode::{CHECK_EVERY, decode_escapes, strip_quotes};

use std::fmt::Write as _;

use crate::nav::PathSegment;

/// Default root name of JavaScript paths.
pub const DEFAULT_JS_ROOT: &str = "data";

/// A path syntax (stored in the settings as `"jsonpath"`, `"jq"`, `"javascript"`,
/// `"pointer"`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum PathFormat {
    /// `$.orders[3].customer.email` · `$['weird key'][0]`
    #[default]
    JsonPath,
    /// `.orders[3].customer.email` · `.["weird key"][0]`
    Jq,
    /// `data.orders[3].customer.email` · `data["weird key"][0]`
    JavaScript,
    /// `/orders/3/customer/email`
    Pointer,
}

impl PathFormat {
    /// Every format, in menu order.
    pub const ALL: [PathFormat; 4] = [
        PathFormat::JsonPath,
        PathFormat::Jq,
        PathFormat::JavaScript,
        PathFormat::Pointer,
    ];

    /// Name for menus and messages ("Copied JSONPath").
    pub fn label(self) -> &'static str {
        match self {
            PathFormat::JsonPath => "JSONPath",
            PathFormat::Jq => "jq",
            PathFormat::JavaScript => "JavaScript",
            PathFormat::Pointer => "JSON Pointer",
        }
    }
}

/// Formats `path` (keys are spans into `bytes`) in `format`. `js_root` is the root name
/// of JavaScript paths (see [`DEFAULT_JS_ROOT`]); other formats ignore it.
///
/// # Example
/// ```
/// use braceray_core::nav::{PathSegment, Span};
/// use braceray_core::path::{format_path, PathFormat};
///
/// let bytes = br#"{"a b": [{"id": 1}]}"#;
/// let key = |s: u64, e: u64| PathSegment::Key(Span { start: s, end: e });
/// let path = [key(1, 6), PathSegment::Index(0), key(10, 14)];
/// assert_eq!(format_path(bytes, &path, PathFormat::JsonPath, "data"), "$['a b'][0].id");
/// assert_eq!(format_path(bytes, &path, PathFormat::Jq, "data"), r#".["a b"][0].id"#);
/// assert_eq!(format_path(bytes, &path, PathFormat::JavaScript, "data"), r#"data["a b"][0].id"#);
/// assert_eq!(format_path(bytes, &path, PathFormat::Pointer, "data"), "/a b/0/id");
/// ```
pub fn format_path(
    bytes: &[u8],
    path: &[PathSegment],
    format: PathFormat,
    js_root: &str,
) -> String {
    let mut out = String::new();
    match format {
        PathFormat::JsonPath => out.push('$'),
        PathFormat::Jq => {}
        PathFormat::JavaScript => out.push_str(js_root),
        PathFormat::Pointer => {}
    }
    for seg in path {
        match seg {
            PathSegment::Index(i) => {
                let _ = match format {
                    PathFormat::Pointer => write!(out, "/{i}"),
                    PathFormat::Jq if out.is_empty() => write!(out, ".[{i}]"),
                    _ => write!(out, "[{i}]"),
                };
            }
            PathSegment::Key(span) => {
                let name = decode_key(span.slice(bytes));
                push_key(&mut out, &name, format);
            }
        }
    }
    if format == PathFormat::Jq && out.is_empty() {
        out.push('.');
    }
    out
}

/// The JSONPath of `path`: [`format_path`] with [`PathFormat::JsonPath`].
///
/// # Example
/// ```
/// use braceray_core::nav::{PathSegment, Span};
/// use braceray_core::path::jsonpath;
///
/// let bytes = br#"{"a b": [{"id": 1}]}"#;
/// let key = |s: u64, e: u64| PathSegment::Key(Span { start: s, end: e });
/// let path = [key(1, 6), PathSegment::Index(0), key(10, 14)];
/// assert_eq!(jsonpath(bytes, &path), "$['a b'][0].id");
/// ```
pub fn jsonpath(bytes: &[u8], path: &[PathSegment]) -> String {
    format_path(bytes, path, PathFormat::JsonPath, DEFAULT_JS_ROOT)
}

/// Appends one decoded key in `format`.
fn push_key(out: &mut String, name: &str, format: PathFormat) {
    match format {
        PathFormat::JsonPath => {
            if is_identifier(name, false) {
                out.push('.');
                out.push_str(name);
            } else {
                out.push_str("['");
                push_escaped(out, name, '\'', false);
                out.push_str("']");
            }
        }
        PathFormat::Jq => {
            if is_identifier(name, false) && !JQ_KEYWORDS.contains(&name) {
                out.push('.');
                out.push_str(name);
            } else {
                // `.["k"]` at the start; `.a["k"]` after a step (jq 1.6 rejects `.a.["k"]`).
                if out.is_empty() {
                    out.push('.');
                }
                out.push_str("[\"");
                push_escaped(out, name, '"', false);
                out.push_str("\"]");
            }
        }
        PathFormat::JavaScript => {
            if is_identifier(name, true) && !JS_RESERVED.contains(&name) {
                out.push('.');
                out.push_str(name);
            } else {
                out.push_str("[\"");
                push_escaped(out, name, '"', true);
                out.push_str("\"]");
            }
        }
        PathFormat::Pointer => {
            out.push('/');
            for c in name.chars() {
                match c {
                    '~' => out.push_str("~0"),
                    '/' => out.push_str("~1"),
                    c => out.push(c),
                }
            }
        }
    }
}

/// Appends `name` inside `quote` quotes: the quote and `\` get a backslash, `\n` `\r`
/// `\t` their short escapes, other control characters `\uXXXX` (and, with `js`, the line
/// separators U+2028 / U+2029, which older JavaScript doesn't accept in strings).
fn push_escaped(out: &mut String, name: &str, quote: char, js: bool) {
    for c in name.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() || (js && matches!(c, '\u{2028}' | '\u{2029}')) => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
}

/// `true` for ASCII identifiers: a letter or `_` (or `$` with `dollar`) first, then
/// letters, digits, `_` (and `$`).
fn is_identifier(name: &str, dollar: bool) -> bool {
    let ok = |c: char| c.is_ascii_alphanumeric() || c == '_' || (dollar && c == '$');
    let mut chars = name.chars();
    chars.next().is_some_and(|c| ok(c) && !c.is_ascii_digit()) && chars.all(ok)
}

/// jq keywords (`.if` is a syntax error before jq 1.7).
const JQ_KEYWORDS: &[&str] = &[
    "__loc__", "and", "as", "catch", "def", "elif", "else", "end", "foreach", "if", "import",
    "include", "label", "not", "or", "reduce", "then", "try",
];

/// JavaScript reserved words, strict-mode reserved words and literals.
const JS_RESERVED: &[&str] = &[
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];
