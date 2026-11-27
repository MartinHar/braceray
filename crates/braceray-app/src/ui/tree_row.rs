//! The text of one tree row: `key : value-preview   {12}` with type colours, plus a
//! plain-text label for screen readers and tests.

use braceray_core::index::ContainerKind;
use braceray_core::nav::ValueKind;
use braceray_core::tree::{Row, RowItem, ValueRow};
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{Color32, FontId};

use crate::format;

/// Characters shown of a string, key or number.
pub const PREVIEW_CHARS: usize = 120;

/// Strings longer than this get a size badge.
pub const BIG_VALUE_BYTES: u64 = 1 << 20;

/// Colours of the row parts.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Object keys.
    pub key: Color32,
    /// String values.
    pub string: Color32,
    /// Numbers.
    pub number: Color32,
    /// `true`, `false`, `null`.
    pub literal: Color32,
    /// Brackets, indices, counts.
    pub punct: Color32,
    /// Other text.
    pub text: Color32,
}

impl Palette {
    /// The palette for a dark or light background.
    pub fn new(dark: bool, accent: Color32, text: Color32, weak: Color32) -> Self {
        let rgb = Color32::from_rgb;
        if dark {
            Palette {
                key: accent,
                string: rgb(0x86, 0xEF, 0xAC),
                number: rgb(0x93, 0xC5, 0xFD),
                literal: rgb(0xD8, 0xB4, 0xFE),
                punct: weak,
                text,
            }
        } else {
            Palette {
                key: accent,
                string: rgb(0x15, 0x80, 0x3D),
                number: rgb(0x1D, 0x4E, 0xD8),
                literal: rgb(0x7E, 0x22, 0xCE),
                punct: weak,
                text,
            }
        }
    }
}

/// Builds the row text into `job` / `label`.
struct Writer<'p> {
    job: LayoutJob,
    label: String,
    font: FontId,
    palette: &'p Palette,
}

impl Writer<'_> {
    fn push(&mut self, text: &str, color: Color32) {
        self.label.push_str(text);
        self.job.append(
            text,
            0.0,
            TextFormat {
                font_id: self.font.clone(),
                color,
                ..TextFormat::default()
            },
        );
    }

    fn badge(&mut self, text: &str) {
        self.label.push_str("  ");
        self.job.append(
            text,
            12.0,
            TextFormat {
                font_id: FontId::proportional(self.font.size - 1.5),
                color: self.palette.punct,
                ..TextFormat::default()
            },
        );
        self.label.push_str(text);
    }
}

/// The laid-out text and accessible label of `row`.
///
/// `cut_at` is the end of the preview prefix in preview mode: a container reaching it may
/// have more children than counted, shown as `[1,234+]`.
pub fn row_text(
    bytes: &[u8],
    row: &Row,
    palette: &Palette,
    font: FontId,
    cut_at: Option<u64>,
) -> (LayoutJob, String) {
    let mut w = Writer {
        job: LayoutJob::default(),
        label: String::new(),
        font,
        palette,
    };
    w.job.wrap.max_rows = 1;
    w.job.wrap.break_anywhere = true;
    match &row.item {
        RowItem::Value(v) => value_row(&mut w, bytes, v, cut_at),
        RowItem::Group { range, .. } => {
            let text = format!(
                "[{} … {}]",
                format::group_digits(range.start),
                format::group_digits(range.end - 1)
            );
            w.push(&text, palette.punct);
            w.badge(&format!(
                "{} items",
                format::group_digits(range.end - range.start)
            ));
        }
        RowItem::Loading { index, .. } => {
            w.push(
                &format!("[{}]", format::group_digits(*index)),
                palette.punct,
            );
            w.push("  loading…", palette.punct);
        }
    }
    (w.job, w.label)
}

fn value_row(w: &mut Writer<'_>, bytes: &[u8], v: &ValueRow, cut_at: Option<u64>) {
    let palette = *w.palette;
    match (v.parent, v.child.key) {
        (None, _) => w.push("root", palette.punct),
        (Some(ContainerKind::Object), Some(key)) => {
            w.push(&preview_text(inner(key.slice(bytes))).0, palette.key)
        }
        _ => w.push(
            &format!("[{}]", format::group_digits(v.index)),
            palette.punct,
        ),
    }
    w.push(" : ", palette.punct);
    let raw = v.child.value.slice(bytes);
    let len = v.child.value.end - v.child.value.start;
    match v.child.kind {
        ValueKind::String => {
            let (text, cut) = preview_text(inner(raw));
            w.push("\"", palette.string);
            w.push(&text, palette.string);
            w.push(if cut { "…\"" } else { "\"" }, palette.string);
            if len > BIG_VALUE_BYTES {
                w.badge(&format::bytes(len));
            }
        }
        ValueKind::Number => w.push(&preview_text(raw).0, palette.number),
        ValueKind::Bool | ValueKind::Null => w.push(&preview_text(raw).0, palette.literal),
        ValueKind::Object | ValueKind::Array => {
            let (open, close) = if v.child.kind == ValueKind::Object {
                ("{", "}")
            } else {
                ("[", "]")
            };
            let cut_off = cut_at.is_some_and(|at| v.child.value.end >= at);
            let count = match v.count {
                Some(0) if !cut_off => String::new(),
                Some(n) if cut_off => format!("{}+", format::group_digits(n)),
                Some(n) => format::group_digits(n),
                None => "…".to_owned(),
            };
            w.push(&format!("{open}{count}{close}"), palette.punct);
            if len > BIG_VALUE_BYTES && !cut_off {
                w.badge(&format::bytes(len));
            }
        }
    }
}

/// Bytes between the quotes of a string or key.
fn inner(raw: &[u8]) -> &[u8] {
    let raw = raw.strip_prefix(b"\"").unwrap_or(raw);
    raw.strip_suffix(b"\"").unwrap_or(raw)
}

/// Up to [`PREVIEW_CHARS`] characters of `raw`, decoded lossily (escapes stay as
/// written, invalid UTF-8 and control characters become `�`); `true` if cut.
pub fn preview_text(raw: &[u8]) -> (String, bool) {
    // 4 bytes per char at most, so the window never cuts the shown characters.
    let window = &raw[..raw.len().min(PREVIEW_CHARS * 4)];
    let decoded = String::from_utf8_lossy(window);
    let mut out = String::with_capacity(window.len().min(PREVIEW_CHARS * 4));
    let mut chars = decoded.chars();
    for c in chars.by_ref().take(PREVIEW_CHARS) {
        out.push(if c.is_control() { '\u{FFFD}' } else { c });
    }
    let cut = chars.next().is_some() || window.len() < raw.len();
    (out, cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews() {
        assert_eq!(preview_text(b"abc"), ("abc".to_owned(), false));
        assert_eq!(preview_text("Բարև 世界 🎉".as_bytes()).0, "Բարև 世界 🎉");
        assert_eq!(preview_text(br"a\nb\u00e9").0, r"a\nb\u00e9");
        assert_eq!(preview_text(b"ok\xFF\xFEend").0, "ok\u{FFFD}\u{FFFD}end");
        assert_eq!(preview_text(b"tab\there").0, "tab\u{FFFD}here");
        let long = "é".repeat(500);
        let (text, cut) = preview_text(long.as_bytes());
        assert!(cut);
        assert_eq!(text.chars().count(), PREVIEW_CHARS);
        assert!(!text.contains('\u{FFFD}'));
        let emoji = "🎉".repeat(PREVIEW_CHARS);
        assert_eq!(preview_text(emoji.as_bytes()), (emoji.clone(), false));
    }
}
