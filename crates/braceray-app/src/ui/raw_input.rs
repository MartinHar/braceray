//! Input of the raw view: keys, the byte-mapped scrollbar, clicks and drags.

use braceray_core::job::Repaint;
use eframe::egui::{self, Key, Modifiers, Rect, Role, Sense, pos2, vec2};

use super::raw::{RawContext, RawOutput, SCROLLBAR, SCROLLBAR_LABEL};
use crate::raw_view::text::RowText;
use crate::raw_view::{RawRow, RawView};
use crate::tree_view::TreeView;

pub(super) fn keyboard(
    ctx: &egui::Context,
    raw: &mut RawView,
    tree: &TreeView,
    page: usize,
    repaint: &Repaint,
    out: &mut RawOutput,
) {
    let mut moves = Vec::new();
    let mut home_end = None;
    ctx.input_mut(|i| {
        for (key, rows) in [
            (Key::ArrowUp, -1.0),
            (Key::ArrowDown, 1.0),
            (Key::PageUp, -(page as f64)),
            (Key::PageDown, page as f64),
        ] {
            while i.consume_key(Modifiers::NONE, key) {
                moves.push(rows);
            }
        }
        if i.consume_key(Modifiers::NONE, Key::Home)
            || i.consume_key(Modifiers::COMMAND, Key::ArrowUp)
        {
            home_end = Some(false);
        }
        if i.consume_key(Modifiers::NONE, Key::End)
            || i.consume_key(Modifiers::COMMAND, Key::ArrowDown)
        {
            home_end = Some(true);
        }
        if i.events.iter().any(|e| matches!(e, egui::Event::Copy)) {
            out.copy = true;
        }
    });
    for m in moves {
        raw.scroll_by(tree, m, page, repaint);
    }
    match home_end {
        Some(false) => raw.go_to(tree, 0, 0, repaint),
        Some(true) => raw.go_to(tree, raw.len(), page, repaint),
        None => {}
    }
}

pub(super) fn scrollbar(
    ui: &mut egui::Ui,
    rect: Rect,
    raw: &mut RawView,
    tree: &TreeView,
    full_rows: usize,
    cx: &RawContext<'_>,
) {
    let track = Rect::from_min_max(pos2(rect.right() - SCROLLBAR, rect.top()), rect.max);
    let response = ui.interact(
        track,
        egui::Id::unique(("raw_scrollbar", cx.tab_id)),
        Sense::click_and_drag(),
    );
    super::label_icon(&response, Role::ScrollBar, SCROLLBAR_LABEL);
    // A thumb about one screen of rows long, but never tiny.
    let thumb_h = (track.height() * full_rows as f32 * 40.0 / raw.len().max(1) as f32)
        .clamp(24.0, track.height());
    let free = (track.height() - thumb_h).max(1.0);
    if let Some(p) = response.interact_pointer_pos()
        && (response.dragged() || response.clicked() || response.is_pointer_button_down_on())
    {
        let t = f64::from(((p.y - track.top() - thumb_h / 2.0) / free).clamp(0.0, 1.0));
        let target = (t * raw.len() as f64) as u64;
        // Re-seek only when the target moved by more than a few rows' worth of bytes.
        if target.abs_diff(raw.top_offset()) > 64 || t == 0.0 || t == 1.0 {
            let context = if t >= 1.0 { full_rows } else { 0 };
            raw.go_to(tree, target, context, cx.repaint);
        }
    }
    let t = raw.fraction() as f32;
    let thumb = Rect::from_min_size(
        pos2(track.left() + 2.0, track.top() + t * free),
        vec2(SCROLLBAR - 4.0, thumb_h),
    );
    let visuals = ui.visuals();
    let color = if response.hovered() || response.dragged() {
        visuals.widgets.hovered.fg_stroke.color.gamma_multiply(0.6)
    } else {
        visuals
            .widgets
            .inactive
            .fg_stroke
            .color
            .gamma_multiply(0.35)
    };
    ui.painter().rect_filled(thumb, 4.0, color);
}

/// Clicks and drags in the text: selection, focus, and the offset for tree sync.
#[allow(clippy::too_many_arguments)]
pub(super) fn pointer(
    ui: &mut egui::Ui,
    raw: &mut RawView,
    laid_out: &[(RawRow, RowText, std::sync::Arc<egui::Galley>, egui::Pos2)],
    text_rect: Rect,
    bytes: &[u8],
    tab_id: u64,
    out: &mut RawOutput,
) {
    let response = ui.interact(
        text_rect,
        egui::Id::unique(("raw_text", tab_id)),
        Sense::click_and_drag(),
    );
    let Some(p) = response.interact_pointer_pos() else {
        return;
    };
    // `glyph`: the character under the pointer (for the tree); otherwise the nearest
    // caret position (for selections).
    let offset_at = |p: egui::Pos2, glyph: bool| -> Option<u64> {
        if laid_out.is_empty() {
            return None;
        }
        let k = laid_out
            .iter()
            .rposition(|(_, _, _, o)| o.y <= p.y)
            .unwrap_or(0);
        let (row, text, galley, origin) = &laid_out[k];
        let rel = p - *origin;
        let mut c = galley.cursor_from_pos(rel).index.0;
        if glyph && c > 0 && galley.pos_from_cursor(egui::text::CCursor::new(c)).min.x > rel.x {
            c -= 1;
        }
        Some(text.offset_of_char(bytes, c, row.text.end))
    };
    let (shift, pressed) = ui.input(|i| (i.modifiers.shift, i.pointer.primary_pressed()));
    if pressed && response.hovered() {
        out.focus = true;
        if let Some(off) = offset_at(p, false) {
            match (shift, raw.selection) {
                (true, Some((a, _))) => raw.selection = Some((a, off)),
                _ => raw.selection = Some((off, off)),
            }
        }
    } else if response.dragged()
        && let Some(off) = offset_at(p.clamp(text_rect.min, text_rect.max), false)
        && let Some((a, _)) = raw.selection
    {
        raw.selection = Some((a, off));
    }
    if response.clicked() && !shift {
        out.clicked = offset_at(p, true);
    }
}
