//! The raw text view: a header (mode, Go to), then virtualized rows with a gutter, syntax
//! colours, the text selection and the flash of a revealed value, and a scrollbar mapped
//! to byte offsets (0 … file length). Work per frame is proportional to the rows on
//! screen.

use std::time::Instant;

use braceray_core::job::Repaint;
use braceray_core::pretty::TokenKind;
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Align2, Color32, FontId, Rect, RichText, Role, Sense, pos2, vec2};

use super::raw_input;
use super::tree_row::Palette;
use crate::format;
use crate::raw_view::text::{RowText, layout};
use crate::raw_view::{FLASH, Gutter, RawMode, RawRow, RawView};
use crate::search_view::TabSearch;
use crate::theme;
use crate::tree_view::TreeView;

/// Height of one row in points.
pub const ROW_HEIGHT: f32 = 18.0;
/// Width of the scrollbar.
pub(super) const SCROLLBAR: f32 = 12.0;
/// Accessible name of the scrollbar.
pub const SCROLLBAR_LABEL: &str = "Raw scrollbar";
/// Labels of the mode buttons.
pub const PRETTY: &str = "Pretty";
/// See [`PRETTY`].
pub const ORIGINAL: &str = "Original";
/// Label of the Go to button.
pub const GO_TO: &str = "Go to…";
/// Characters scanned at most for a gutter tooltip's column.
const COLUMN_SCAN: usize = 4 << 20;
/// Indentation per level (in characters).
const INDENT_CHARS: f32 = 2.0;

/// What the raw view needs besides itself.
pub struct RawContext<'a> {
    /// Stable id of the tab.
    pub tab_id: u64,
    /// Wakes the UI when a seek job finishes.
    pub repaint: &'a Repaint,
    /// The raw view has the keyboard focus.
    pub focused: bool,
    /// The tab's search, for highlighting matches.
    pub search: Option<&'a TabSearch>,
}

/// Matches highlighted at most per frame.
const MAX_HIGHLIGHTS: usize = 4096;

/// What happened in the raw view this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RawOutput {
    /// A click at this source offset: select the value there in the tree.
    pub clicked: Option<u64>,
    /// The pointer went down in the view (it takes the keyboard focus).
    pub focus: bool,
    /// The Go to button was pressed.
    pub go_to: bool,
    /// Copy the selection (Ctrl/Cmd+C).
    pub copy: bool,
    /// Rows drawn.
    pub rows_drawn: usize,
}

/// Draws the raw view of `tree`'s document into the remaining space of `ui`.
pub fn show(
    ui: &mut egui::Ui,
    raw: &mut RawView,
    tree: &TreeView,
    cx: RawContext<'_>,
) -> RawOutput {
    let mut out = RawOutput::default();
    raw.poll(tree);
    raw.sync_source(tree, cx.repaint);
    header(ui, raw, tree, &cx, &mut out);

    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    if raw.is_empty() {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Empty",
            FontId::proportional(14.0),
            ui.visuals().weak_text_color(),
        );
        return out;
    }
    let font = FontId::monospace(13.0);
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0')).max(1.0);
    let gutter_chars = match raw.mode {
        RawMode::Original => format::group_digits(tree.nav().index().lines().line_count()).len(),
        RawMode::Pretty => format::group_digits(raw.len()).len(),
    } as f32;
    let gutter_w = gutter_chars * char_w + 16.0;
    let text_rect = Rect::from_min_max(
        pos2(rect.left() + gutter_w, rect.top()),
        pos2(rect.right() - SCROLLBAR, rect.bottom()),
    );
    let full_rows = ((rect.height() / ROW_HEIGHT).floor() as usize).max(1);
    let visible = (rect.height() / ROW_HEIGHT).ceil() as usize + 1;

    // Input that moves the view comes first, so this frame draws the new position.
    let ctx = ui.ctx().clone();
    if ui.rect_contains_pointer(rect) {
        let delta = ui.input(|i| i.smooth_scroll_delta);
        if delta.y != 0.0 {
            raw.scroll_by(
                tree,
                f64::from(-delta.y / ROW_HEIGHT),
                full_rows,
                cx.repaint,
            );
        }
        raw.hscroll = (raw.hscroll - delta.x).max(0.0);
    }
    if cx.focused && !ctx.egui_wants_keyboard_input() && !egui::Popup::is_any_open(&ctx) {
        raw_input::keyboard(&ctx, raw, tree, full_rows, cx.repaint, &mut out);
    }
    raw_input::scrollbar(ui, rect, raw, tree, full_rows, &cx);
    raw.fill_screen(tree, full_rows, cx.repaint);

    let rows: Vec<RawRow> = raw.rows(tree, visible).to_vec();
    out.rows_drawn = rows.len();
    let nav = tree.nav();
    let bytes = nav.bytes();
    let palette = Palette::new(
        ui.visuals().dark_mode,
        theme::accent(ui),
        ui.visuals().text_color(),
        ui.visuals().weak_text_color(),
    );
    let error = theme::error_color(ui.visuals().dark_mode);
    let selection = raw.selected_span();
    let flash = raw.flash.and_then(|(span, at)| {
        let t = at.elapsed().as_secs_f32() / FLASH.as_secs_f32();
        (t < 1.0).then_some((span, 1.0 - t))
    });
    if flash.is_some() {
        ctx.request_repaint();
    } else {
        raw.flash = None;
    }
    let visible_span = rows
        .first()
        .zip(rows.last())
        .map(|(a, b)| (a.span.start, b.span.end));
    let hits = match (cx.search, visible_span) {
        (Some(search), Some((a, b))) => search.matches_in(a, b, MAX_HIGHLIGHTS),
        _ => Vec::new(),
    };
    let current = cx.search.and_then(|s| s.current).map(|c| c.m);
    let soft = theme::search_soft(ui);
    let strong = theme::search_strong(ui);
    let sel_color = ui.visuals().selection.bg_fill;
    let flash_color = theme::accent(ui);
    let painter = ui.painter_at(rect);
    let text_painter = ui.painter_at(text_rect);
    let max_levels = ((text_rect.width() * 0.4) / (INDENT_CHARS * char_w)).max(1.0) as u32;
    let mut widest = 0.0f32;
    let mut laid_out = Vec::with_capacity(rows.len());

    for (k, row) in rows.iter().enumerate() {
        let top = rect.top() + (k as f32 - raw.frac) * ROW_HEIGHT;
        let row_rect = Rect::from_min_size(
            pos2(text_rect.left(), top),
            vec2(text_rect.width(), ROW_HEIGHT),
        );
        let levels = row.depth.min(max_levels);
        let x = text_rect.left() + 4.0 + levels as f32 * INDENT_CHARS * char_w - raw.hscroll;
        let text = layout(row, bytes, raw.mode);
        let job = highlight(&text, &palette, error, font.clone());
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let origin = pos2(x, top + (ROW_HEIGHT - galley.size().y) / 2.0);
        widest = widest.max(x + raw.hscroll + galley.size().x - text_rect.left());

        // Backgrounds: search matches, the current match, the flash, the selection.
        let first_hit = hits.partition_point(|m| m.end() <= row.span.start);
        for m in hits[first_hit..]
            .iter()
            .take_while(|m| m.offset < row.span.end)
        {
            let color = if current == Some(*m) { strong } else { soft };
            fill_chars(
                &text_painter,
                &galley,
                origin,
                &text,
                bytes,
                span_of(*m),
                color,
            );
        }
        if let Some(m) = current
            && !hits.contains(&m)
            && m.offset < row.span.end
            && m.end() > row.span.start
        {
            fill_chars(
                &text_painter,
                &galley,
                origin,
                &text,
                bytes,
                span_of(m),
                strong,
            );
        }
        if let Some((span, alpha)) = flash {
            fill_chars(
                &text_painter,
                &galley,
                origin,
                &text,
                bytes,
                span,
                flash_color.gamma_multiply(0.35 * alpha),
            );
        }
        if let Some(sel) = selection {
            fill_chars(&text_painter, &galley, origin, &text, bytes, sel, sel_color);
        }
        if row.depth > max_levels {
            text_painter.text(
                pos2(x - 4.0, row_rect.center().y),
                Align2::RIGHT_CENTER,
                format!("depth {}", format::group_digits(u64::from(row.depth))),
                FontId::proportional(10.0),
                palette.punct,
            );
        }
        if let Some(at) = raw.reveal_x
            && at >= row.span.start
            && at < row.span.end
        {
            reveal_x(raw, &text, &galley, origin, text_rect, bytes, at);
            ctx.request_repaint();
        }
        text_painter.galley(origin, galley.clone(), palette.text);
        gutter(
            ui, &painter, rect, gutter_w, top, row, &font, &palette, cx.tab_id, k, tree,
        );

        // Accessible rows (kittest, egui-mcp) only for rows on screen.
        if row_rect.intersects(text_rect) {
            let label = text.text.clone();
            let id = egui::Id::unique(("raw_row", cx.tab_id, k));
            let response = ui.interact(row_rect.intersect(text_rect), id, Sense::hover());
            response.widget_info(|| egui::WidgetInfo::labeled(Role::Row, true, label.clone()));
        }
        laid_out.push((row.clone(), text, galley, origin));
    }
    let max_h = (widest - text_rect.width() + 40.0).max(0.0);
    raw.hscroll = raw.hscroll.min(max_h);

    raw_input::pointer(ui, raw, &laid_out, text_rect, bytes, cx.tab_id, &mut out);
    out
}

fn header(
    ui: &mut egui::Ui,
    raw: &mut RawView,
    tree: &TreeView,
    cx: &RawContext<'_>,
    out: &mut RawOutput,
) {
    ui.horizontal(|ui| {
        for (mode, label) in [(RawMode::Pretty, PRETTY), (RawMode::Original, ORIGINAL)] {
            if ui.selectable_label(raw.mode == mode, label).clicked() {
                raw.set_mode(mode, tree, cx.repaint);
            }
        }
        ui.separator();
        if ui
            .button(GO_TO)
            .on_hover_text("Go to a line, an offset (@200m) or a path ($.a[3])")
            .clicked()
        {
            out.go_to = true;
        }
        if raw.is_seeking() {
            ui.spinner();
        }
        if let Some((text, until)) = &raw.notice {
            if Instant::now() < *until {
                ui.label(RichText::new(text).color(theme::error_color(ui.visuals().dark_mode)));
                ui.ctx().request_repaint_after(*until - Instant::now());
            } else {
                raw.notice = None;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let pct = raw.fraction() * 100.0;
            ui.label(
                RichText::new(format!(
                    "{} · {pct:.1} %",
                    format::group_digits(raw.top_offset())
                ))
                .small()
                .weak(),
            )
            .on_hover_text("Byte offset of the top row");
        });
    });
}

fn highlight(text: &RowText, palette: &Palette, error: Color32, font: FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut at = 0usize; // byte index into text.text
    let mut char_at = 0usize;
    let char_bytes: Vec<usize> = text.text.char_indices().map(|(i, _)| i).collect();
    let byte_of = |c: usize| char_bytes.get(c).copied().unwrap_or(text.text.len());
    let format = |color| TextFormat {
        font_id: font.clone(),
        color,
        ..TextFormat::default()
    };
    for p in &text.pieces {
        if p.char_start > char_at {
            let s = byte_of(p.char_start);
            job.append(&text.text[at..s], 0.0, format(palette.punct));
            at = s;
        }
        let e = byte_of(p.char_start + p.chars);
        let color = match p.kind {
            Some(TokenKind::Key) => palette.key,
            Some(TokenKind::String) => palette.string,
            Some(TokenKind::Number) => palette.number,
            Some(TokenKind::Literal) => palette.literal,
            Some(TokenKind::Punct) => palette.punct,
            Some(TokenKind::Error) => error,
            None => palette.text,
        };
        job.append(&text.text[at..e], 0.0, format(color));
        at = e;
        char_at = p.char_start + p.chars;
    }
    if at < text.text.len() {
        job.append(&text.text[at..], 0.0, format(palette.punct));
    }
    job
}

/// Sets the horizontal scroll so `at` (in this row) is a third into the view.
fn reveal_x(
    raw: &mut RawView,
    text: &RowText,
    galley: &egui::Galley,
    origin: egui::Pos2,
    text_rect: Rect,
    bytes: &[u8],
    at: u64,
) {
    raw.reveal_x = None;
    let span = braceray_core::nav::Span {
        start: at,
        end: at + 1,
    };
    let Some((a, _)) = text.chars_of(bytes, span) else {
        return;
    };
    let x = origin.x + galley.pos_from_cursor(egui::text::CCursor::new(a)).min.x;
    if x < text_rect.left() + 8.0 || x > text_rect.right() - 40.0 {
        let target = text_rect.left() + text_rect.width() / 3.0;
        raw.hscroll = (raw.hscroll + x - target).max(0.0);
    }
}

fn span_of(m: braceray_core::search::Match) -> braceray_core::nav::Span {
    braceray_core::nav::Span {
        start: m.offset,
        end: m.end(),
    }
}

/// Fills the characters of `span` (source offsets) in a row.
#[allow(clippy::too_many_arguments)]
fn fill_chars(
    painter: &egui::Painter,
    galley: &egui::Galley,
    origin: egui::Pos2,
    text: &RowText,
    bytes: &[u8],
    span: braceray_core::nav::Span,
    color: Color32,
) {
    let Some((a, b)) = text.chars_of(bytes, span) else {
        return;
    };
    let x0 = galley.pos_from_cursor(egui::text::CCursor::new(a)).min.x;
    let x1 = galley.pos_from_cursor(egui::text::CCursor::new(b)).min.x;
    let r = Rect::from_min_max(
        pos2(origin.x + x0, origin.y),
        pos2(origin.x + x1.max(x0 + 2.0), origin.y + galley.size().y),
    );
    painter.rect_filled(r, 2.0, color);
}

#[allow(clippy::too_many_arguments)]
fn gutter(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    rect: Rect,
    width: f32,
    top: f32,
    row: &RawRow,
    font: &FontId,
    palette: &Palette,
    tab_id: u64,
    k: usize,
    tree: &TreeView,
) {
    let text = match row.gutter {
        Gutter::Line(n) => format::group_digits(n),
        Gutter::Offset(o) => format::group_digits(o),
        Gutter::Continuation => "↩".to_owned(),
    };
    let r = Rect::from_min_size(pos2(rect.left(), top), vec2(width, ROW_HEIGHT));
    painter.text(
        pos2(r.right() - 8.0, r.center().y),
        Align2::RIGHT_CENTER,
        text,
        font.clone(),
        palette.punct.gamma_multiply(0.8),
    );
    if !r.intersects(rect) {
        return;
    }
    let response = ui.interact(
        r.intersect(rect),
        egui::Id::unique(("raw_gutter", tab_id, k)),
        Sense::hover(),
    );
    let offset = row.tokens.first().map_or(row.span.start, |t| t.span.start);
    response.on_hover_ui(|ui| {
        let nav = tree.nav();
        let p = nav
            .index()
            .lines()
            .position(nav.bytes(), offset, COLUMN_SCAN);
        let col = if p.column_in_bytes {
            "byte column"
        } else {
            "column"
        };
        ui.label(format!(
            "Offset {} · line {}, {col} {}",
            format::group_digits(offset),
            format::group_digits(p.line),
            format::group_digits(p.column)
        ));
    });
}
