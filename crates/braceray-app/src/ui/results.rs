//! The results panel: a header (count, cap note, "Copy all paths") and a virtual list of
//! the stored matches, `#312  $.orders[48213].customer.email  …anna.p@example.com…  line
//! 1,204,331`. Paths and lines come from a worker for the rows on screen.

use braceray_core::job::Repaint;
use braceray_core::search::Match;
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Align2, FontId, Key, Modifiers, Rect, RichText, Role, Sense, pos2, vec2};

use super::Action;
use super::vlist::{SCROLLBAR, scrollbar};
use crate::format;
use crate::search_view::{COPY_TO_FILE_ABOVE, CopyOutcome, TabSearch};
use crate::theme;
use crate::tree_view::TreeView;

/// Height of one row in points.
pub const ROW_HEIGHT: f32 = 20.0;
/// Accessible name of the scrollbar.
pub const SCROLLBAR_LABEL: &str = "Results scrollbar";
/// Label of the copy button.
pub const COPY_ALL: &str = "Copy all paths";
/// Placeholder while a row's path is computed.
pub const PENDING: &str = "…";
/// Bytes of context shown before / after a match.
const BEFORE: u64 = 32;
const AFTER: u64 = 48;

/// What the panel needs besides the search.
pub struct ResultsContext<'a> {
    /// Stable id of the tab.
    pub tab_id: u64,
    /// Wakes the UI when row info arrives.
    pub repaint: &'a Repaint,
    /// The list has the keyboard focus.
    pub focused: bool,
}

/// What happened in the panel this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ResultsOutput {
    /// Jump to stored match `i`.
    pub jump: Option<usize>,
    /// The pointer went down in the list (it takes the keyboard focus).
    pub focus: bool,
    /// Rows drawn.
    pub rows_drawn: usize,
}

/// Draws the header and the list into `ui`.
pub fn show(
    ui: &mut egui::Ui,
    search: &mut TabSearch,
    tree: Option<&TreeView>,
    cx: ResultsContext<'_>,
    actions: &mut Vec<Action>,
) -> ResultsOutput {
    let mut out = ResultsOutput::default();
    header(ui, search, tree, cx.repaint, actions);
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let total = search.stored() as u64;
    if total == 0 {
        let text = if search.query().text.is_empty() {
            "Type in the search box to find text in the file."
        } else if search.is_done() {
            "No matches."
        } else {
            "Searching…"
        };
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(13.0),
            ui.visuals().weak_text_color(),
        );
        return out;
    }
    let list = Rect::from_min_max(rect.min, pos2(rect.right() - SCROLLBAR, rect.bottom()));
    let full_rows = ((list.height() / ROW_HEIGHT).floor() as u64).max(1);
    let max_scroll = total.saturating_sub(full_rows) as f64;
    let ctx = ui.ctx().clone();

    if cx.focused && !ctx.egui_wants_keyboard_input() && !egui::Popup::is_any_open(&ctx) {
        keyboard(&ctx, search, total, full_rows, &mut out);
    }
    if ui.rect_contains_pointer(rect) {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        search.scroll -= f64::from(dy / ROW_HEIGHT);
    }
    if let Some(r) = search.scroll_to.take() {
        let r = r as f64;
        if r < search.scroll {
            search.scroll = r;
        } else if r + 1.0 > search.scroll + full_rows as f64 {
            search.scroll = r + 1.0 - full_rows as f64;
        }
    }
    scrollbar(
        ui,
        rect,
        list,
        &mut search.scroll,
        total,
        full_rows,
        egui::Id::unique(("results_scrollbar", cx.tab_id)),
        SCROLLBAR_LABEL,
    );
    search.scroll = search.scroll.clamp(0.0, max_scroll);

    let first = search.scroll.floor() as u64;
    let shift = ((search.scroll - first as f64) as f32) * ROW_HEIGHT;
    let visible = ((list.height() / ROW_HEIGHT).ceil() as u64 + 1).min(total - first);
    let rows: Vec<(usize, Match)> = (first..first + visible)
        .filter_map(|i| search.get(i as usize).map(|m| (i as usize, m)))
        .collect();
    out.rows_drawn = rows.len();
    if let Some(tree) = tree {
        let offsets: Vec<u64> = rows.iter().map(|(_, m)| m.offset).collect();
        search.info.request(&offsets, tree, cx.repaint);
    }

    let font = FontId::monospace(12.5);
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0')).max(1.0);
    let num_w = (format::group_digits(total).len() as f32 + 2.0) * char_w + 8.0;
    let line_w = 16.0 * char_w;
    let rest = (list.width() - num_w - line_w).max(100.0);
    let path_w = (rest * 0.42).max(80.0);
    let painter = ui.painter_at(list);
    let accent = theme::accent(ui);
    let weak = ui.visuals().weak_text_color();
    let text_color = ui.visuals().text_color();
    let bytes = tree.map(|t| t.document().bytes());
    let selected_bg = ui.visuals().selection.bg_fill;
    let current = search.current.and_then(|c| c.index);

    for (k, (i, m)) in rows.iter().enumerate() {
        let top = list.top() + k as f32 * ROW_HEIGHT - shift;
        let row_rect = Rect::from_min_size(pos2(list.left(), top), vec2(list.width(), ROW_HEIGHT));
        let id = egui::Id::unique(("result_row", cx.tab_id, *i));
        let response = ui.interact(row_rect.intersect(list), id, Sense::CLICK);
        let is_current = current == Some(*i);
        let is_selected = search.list_selected == Some(*i);
        if is_current || is_selected {
            painter.rect_filled(row_rect, 0.0, selected_bg);
        } else if response.hovered() {
            painter.rect_filled(row_rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        let cy = row_rect.center().y;
        let num = format!("#{}", format::group_digits(*i as u64 + 1));
        painter.text(
            pos2(list.left() + num_w - 8.0, cy),
            Align2::RIGHT_CENTER,
            &num,
            font.clone(),
            weak,
        );
        let info = search.info.get(m.offset).cloned();
        let path = info.as_ref().map_or(PENDING, |r| r.path.as_str());
        let path_rect = Rect::from_min_size(
            pos2(list.left() + num_w, top),
            vec2(path_w - 8.0, ROW_HEIGHT),
        );
        ui.painter_at(path_rect.intersect(list)).text(
            pos2(path_rect.left(), cy),
            Align2::LEFT_CENTER,
            path,
            font.clone(),
            if info.is_some() { accent } else { weak },
        );
        let (before, hit, after) = bytes.map(|b| preview(b, *m)).unwrap_or_default();
        let prev_rect = Rect::from_min_max(
            pos2(path_rect.right() + 8.0, top),
            pos2(list.right() - line_w, top + ROW_HEIGHT),
        );
        let mut job = LayoutJob::default();
        let fmt = |color, background| TextFormat {
            font_id: font.clone(),
            color,
            background,
            ..TextFormat::default()
        };
        job.append(&before, 0.0, fmt(weak, egui::Color32::TRANSPARENT));
        job.append(&hit, 0.0, fmt(text_color, theme::search_soft(ui)));
        job.append(&after, 0.0, fmt(weak, egui::Color32::TRANSPARENT));
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        ui.painter_at(prev_rect.intersect(list)).galley(
            pos2(prev_rect.left(), cy - galley.size().y / 2.0),
            galley,
            text_color,
        );
        let line = info.as_ref().map_or(String::new(), |r| {
            format!("line {}", format::group_digits(r.line))
        });
        painter.text(
            pos2(list.right() - 6.0, cy),
            Align2::RIGHT_CENTER,
            &line,
            font.clone(),
            weak,
        );
        let label = format!("{num} {path} {before}{hit}{after} {line}");
        response.widget_info(|| {
            egui::WidgetInfo::selected(Role::Row, true, is_current || is_selected, label.clone())
        });
        if response.clicked() {
            search.list_selected = Some(*i);
            out.jump = Some(*i);
        }
    }
    let pressed = ui.input(|i| i.pointer.primary_pressed() || i.pointer.secondary_pressed());
    if pressed && ui.rect_contains_pointer(rect) {
        out.focus = true;
    }
    out
}

/// ↑ / ↓ / PageUp / PageDown / Home / End move the list selection; Enter jumps to it.
fn keyboard(
    ctx: &egui::Context,
    search: &mut TabSearch,
    total: u64,
    page: u64,
    out: &mut ResultsOutput,
) {
    let last = total.saturating_sub(1) as i64;
    let mut at = search
        .list_selected
        .or(search.current.and_then(|c| c.index))
        .map(|i| i as i64);
    let mut moved = false;
    ctx.input_mut(|i| {
        let page = page.max(1) as i64;
        for (key, delta) in [
            (Key::ArrowDown, 1),
            (Key::ArrowUp, -1),
            (Key::PageDown, page),
            (Key::PageUp, -page),
        ] {
            while i.consume_key(Modifiers::NONE, key) {
                at = Some(at.map_or(0, |a| (a + delta).clamp(0, last)));
                moved = true;
            }
        }
        if i.consume_key(Modifiers::NONE, Key::Home) {
            at = Some(0);
            moved = true;
        }
        if i.consume_key(Modifiers::NONE, Key::End) {
            at = Some(last);
            moved = true;
        }
        if i.consume_key(Modifiers::NONE, Key::Enter) {
            out.jump = at.map(|a| a as usize);
        }
    });
    if moved && let Some(a) = at {
        search.list_selected = Some(a as usize);
        search.scroll_to = Some(a as usize);
    }
}

fn header(
    ui: &mut egui::Ui,
    search: &mut TabSearch,
    tree: Option<&TreeView>,
    repaint: &Repaint,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Results").strong());
        if !search.query().text.is_empty() {
            ui.label(RichText::new(search.count_text()).weak());
        }
        if search.handle().is_some_and(|h| h.is_capped()) {
            ui.label(
                RichText::new(format!(
                    "· showing first {}",
                    format::group_digits(search.stored() as u64)
                ))
                .weak(),
            )
            .on_hover_text(
                "The count is exact. F3 / Shift+F3 also reach the matches after the stored ones.",
            );
        }
        let preview = tree.is_none_or(TreeView::is_preview);
        if search.copy.is_busy() {
            ui.add(
                egui::ProgressBar::new(search.copy.progress())
                    .desired_width(120.0)
                    .text("Copying paths…"),
            );
            if ui.small_button("Cancel").clicked() {
                search.copy.cancel();
            }
        } else {
            let rows = search.stored();
            let hint = if rows > COPY_TO_FILE_ABOVE {
                format!(
                    "Writes {} paths to a text file you can open",
                    format::group_digits(rows as u64)
                )
            } else {
                "Copies the path of every result to the clipboard".to_owned()
            };
            let b = ui
                .add_enabled(
                    rows > 0 && search.is_done() && !preview,
                    egui::Button::new(COPY_ALL).small(),
                )
                .on_hover_text(hint)
                .on_disabled_hover_text(if preview {
                    "Available when indexing finishes"
                } else {
                    "Available when the search finishes"
                });
            if b.clicked()
                && let (Some(nav), Some(tree)) = (search.nav(), tree)
            {
                search.copy.start(nav, rows, tree, repaint);
            }
        }
        match search.copy.outcome.clone() {
            Some(CopyOutcome::Clipboard { text, rows }) => {
                actions.push(Action::Copy(text));
                search.copy.outcome = None;
                search.toast(&format!(
                    "Copied {} paths",
                    format::group_digits(rows as u64)
                ));
            }
            Some(CopyOutcome::File { path, rows }) => {
                ui.label(format!(
                    "Saved {} paths to {}",
                    format::group_digits(rows as u64),
                    path.display()
                ));
                if ui.small_button("Open").clicked() {
                    actions.push(Action::OpenExternal(path.clone()));
                }
                if ui.small_button("Copy file path").clicked() {
                    actions.push(Action::Copy(path.display().to_string()));
                }
                if ui.small_button("×").clicked() {
                    search.copy.outcome = None;
                }
            }
            Some(CopyOutcome::Failed(e)) => {
                ui.label(RichText::new(e).color(theme::error_color(ui.visuals().dark_mode)));
                if ui.small_button("×").clicked() {
                    search.copy.outcome = None;
                }
            }
            None => {}
        }
    });
}

/// Context around a match: `(before, match, after)`, on character boundaries, with
/// line breaks, tabs and runs of spaces shown as one space and "…" where the text is cut.
pub fn preview(bytes: &[u8], m: Match) -> (String, String, String) {
    let len = bytes.len() as u64;
    let s = m.offset.min(len);
    let e = m.end().min(len);
    let mut a = s.saturating_sub(BEFORE);
    while a < s && (bytes[a as usize] & 0xC0) == 0x80 {
        a += 1;
    }
    let mut b = (e + AFTER).min(len);
    while b > e && b < len && (bytes[b as usize] & 0xC0) == 0x80 {
        b -= 1;
    }
    // Control characters become spaces and runs of whitespace one space (pretty files).
    let clean = |r: std::ops::Range<u64>| -> String {
        let mut out = String::new();
        for c in String::from_utf8_lossy(&bytes[r.start as usize..r.end as usize]).chars() {
            let c = if c.is_control() { ' ' } else { c };
            if !(c == ' ' && out.ends_with(' ')) {
                out.push(c);
            }
        }
        out
    };
    let mut before = clean(a..s);
    if a > 0 {
        before.insert(0, '…');
    }
    let mut after = clean(e..b);
    if b < len {
        after.push('…');
    }
    (before, clean(s..e), after)
}
