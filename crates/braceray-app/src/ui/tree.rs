//! The tree view: a virtualized list of rows with its own scroll position (in rows, as
//! `f64`, so millions of rows keep pixel precision), keyboard navigation and a context
//! menu. Work per frame is proportional to the rows on screen.

use braceray_core::job::Repaint;
use braceray_core::tree::{Row, RowItem};
use eframe::egui::{
    self, Align2, Color32, FontId, Key, Modifiers, Rect, Role, Sense, Stroke, StrokeKind, pos2,
    vec2,
};

use super::Action;
use super::tree_row::{self, Palette};
use crate::format;
use crate::theme;
use crate::tree_view::{TreeKey, TreeView};

/// Height of one row in points.
pub const ROW_HEIGHT: f32 = 22.0;
/// Indentation per level.
const INDENT: f32 = 16.0;
use super::vlist::SCROLLBAR;
/// Values up to this size can be copied.
pub const COPY_LIMIT: u64 = 16 << 20;
/// Accessible name of the scrollbar.
pub const SCROLLBAR_LABEL: &str = "Tree scrollbar";
/// Context menu item that scrolls the raw view to the row.
pub const SHOW_IN_RAW: &str = "Show in raw view";

/// What the tree area needs besides the tree itself.
pub struct TreeContext<'a> {
    /// Stable id of the tab.
    pub tab_id: u64,
    /// Wakes the UI when a prepare job finishes.
    pub repaint: &'a Repaint,
    /// Rows drawn this frame (debug overlay).
    pub rows_drawn: &'a mut usize,
    /// The tree has the keyboard focus.
    pub keys: bool,
    /// The tab's search, for match markers.
    pub search: Option<&'a crate::search_view::TabSearch>,
}

/// Draws the tree into the remaining space of `ui`.
pub fn show(
    ui: &mut egui::Ui,
    tree: &mut TreeView,
    cx: TreeContext<'_>,
    actions: &mut Vec<Action>,
) {
    if let Some(ask) = tree.ask_continue {
        ui.horizontal(|ui| {
            ui.label(format!(
                "Expanded {} rows. Expand more?",
                format::group_digits(ask.added)
            ));
            if ui.button("Continue").clicked() {
                tree.expand_all(ask.row, ask.levels, cx.repaint);
            }
            if ui.button("Stop").clicked() {
                tree.ask_continue = None;
            }
        });
    }

    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let list = Rect::from_min_max(rect.min, pos2(rect.right() - SCROLLBAR, rect.bottom()));
    let total = tree.model.row_count();
    let full_rows = ((list.height() / ROW_HEIGHT).floor() as u64).max(1);
    let max_scroll = total.saturating_sub(full_rows) as f64;

    // Keyboard (only when nothing else wants it).
    let ctx = ui.ctx().clone();
    if cx.keys && !ctx.egui_wants_keyboard_input() && !egui::Popup::is_any_open(&ctx) {
        keyboard(&ctx, tree, full_rows, cx.repaint);
    }

    // Scrolling: wheel, then "bring into view", then clamp.
    if ui.rect_contains_pointer(rect) {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        tree.scroll -= f64::from(dy / ROW_HEIGHT);
    }
    if let Some(r) = tree.scroll_to.take() {
        let r = r as f64;
        if r < tree.scroll {
            tree.scroll = r;
        } else if r + 1.0 > tree.scroll + full_rows as f64 {
            tree.scroll = r + 1.0 - full_rows as f64;
        }
    }
    super::vlist::scrollbar(
        ui,
        rect,
        list,
        &mut tree.scroll,
        total,
        full_rows,
        egui::Id::unique(("tree_scrollbar", cx.tab_id)),
        SCROLLBAR_LABEL,
    );
    tree.scroll = tree.scroll.clamp(0.0, max_scroll);

    let first = tree.scroll.floor() as u64;
    let offset = ((tree.scroll - first as f64) as f32) * ROW_HEIGHT;
    let visible = (list.height() / ROW_HEIGHT).ceil() as u64 + 1;
    let rows = tree.rows(first..first + visible).clone();
    for c in &rows.pending {
        tree.request_prepare(*c, cx.repaint);
    }
    *cx.rows_drawn = rows.rows.len();

    let dark = ui.visuals().dark_mode;
    let palette = Palette::new(
        dark,
        theme::accent(ui),
        ui.visuals().text_color(),
        ui.visuals().weak_text_color(),
    );
    let font = FontId::monospace(13.0);
    let cut_at = (tree.is_preview() && tree.browsed_len() < tree.document().len())
        .then(|| tree.browsed_len());
    let painter = ui.painter_at(list);
    let doc = std::sync::Arc::clone(tree.document());
    let bytes = doc.bytes();
    let mut clicked = None;
    for (k, row) in rows.rows.iter().enumerate() {
        let index = first + k as u64;
        let top = list.top() + k as f32 * ROW_HEIGHT - offset;
        let row_rect = Rect::from_min_size(pos2(list.left(), top), vec2(list.width(), ROW_HEIGHT));
        let id = egui::Id::unique(("tree_row", cx.tab_id, index));
        let response = ui.interact(row_rect.intersect(list), id, Sense::CLICK);
        let selected = tree.selected == Some(index);
        let (job, label) = tree_row::row_text(bytes, row, &palette, font.clone(), cut_at);
        response
            .widget_info(|| egui::WidgetInfo::selected(Role::Row, true, selected, label.clone()));

        if selected {
            painter.rect_filled(row_rect, 0.0, ui.visuals().selection.bg_fill);
        } else if response.hovered() {
            painter.rect_filled(row_rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        // Deep rows stop indenting at 40 % of the width and show their depth instead.
        let max_levels = ((list.width() * 0.4) / INDENT).max(1.0) as u32;
        let x = list.left() + 6.0 + row.depth.min(max_levels) as f32 * INDENT;
        if row.depth > max_levels {
            painter.text(
                pos2(x - 4.0, row_rect.center().y),
                Align2::RIGHT_CENTER,
                format!("depth {}", format::group_digits(u64::from(row.depth))),
                FontId::proportional(10.0),
                palette.punct,
            );
        }
        let arrow = Rect::from_center_size(pos2(x + 6.0, row_rect.center().y), vec2(12.0, 12.0));
        if row.expandable() {
            disclosure(&painter, arrow, row.expanded, palette.punct);
        }
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        painter.galley(
            pos2(x + 16.0, row_rect.center().y - galley.size().y / 2.0),
            galley,
            palette.text,
        );
        badge(&painter, row_rect, row, palette.punct);
        if let (Some(search), RowItem::Value(v)) = (cx.search, &row.item) {
            let start = v.child.key.map_or(v.child.value.start, |k| k.start);
            if search.has_stored_in(start, v.child.value.end) {
                let current = search
                    .current
                    .is_some_and(|c| c.m.offset >= start && c.m.offset < v.child.value.end);
                let color = if current {
                    theme::search_strong(ui)
                } else {
                    theme::search_soft(ui).gamma_multiply(2.0)
                };
                let bar = Rect::from_min_size(
                    pos2(row_rect.left(), row_rect.top() + 3.0),
                    vec2(3.0, ROW_HEIGHT - 6.0),
                );
                painter.rect_filled(bar, 1.5, color);
            }
        }

        if response.clicked() {
            let on_arrow = response
                .interact_pointer_pos()
                .is_some_and(|p| p.x < arrow.right() + 2.0 && p.x > arrow.left() - 4.0);
            clicked = Some((index, on_arrow && row.expandable()));
        }
        if response.double_clicked() {
            clicked = Some((index, true));
        }
        response.context_menu(|ui| context_menu(ui, tree, index, row, bytes, actions, cx.repaint));
    }
    if let Some((index, toggle)) = clicked {
        tree.selected = Some(index);
        if toggle {
            tree.toggle(index, cx.repaint);
        }
    }
}

fn keyboard(ctx: &egui::Context, tree: &mut TreeView, page: u64, repaint: &Repaint) {
    let keys = [
        (Key::ArrowUp, TreeKey::Up),
        (Key::ArrowDown, TreeKey::Down),
        (Key::ArrowRight, TreeKey::Right),
        (Key::ArrowLeft, TreeKey::Left),
        (Key::Home, TreeKey::Home),
        (Key::End, TreeKey::End),
        (Key::PageUp, TreeKey::PageUp),
        (Key::PageDown, TreeKey::PageDown),
        (Key::Enter, TreeKey::Toggle),
    ];
    let mut pressed = Vec::new();
    ctx.input_mut(|i| {
        for (key, action) in keys {
            // Every press, including key repeat.
            while i.consume_key(Modifiers::NONE, key) {
                pressed.push(action);
            }
        }
        let star = i
            .events
            .iter()
            .filter(|e| matches!(e, egui::Event::Text(t) if t == "*"))
            .count();
        pressed.extend(std::iter::repeat_n(TreeKey::ExpandChildren, star));
    });
    for key in pressed {
        tree.key(key, page, repaint);
    }
}

/// ▸ / ▾ as a filled triangle.
fn disclosure(painter: &egui::Painter, r: Rect, expanded: bool, color: Color32) {
    let c = r.center();
    let s = 4.0;
    let points = if expanded {
        vec![
            pos2(c.x - s, c.y - s * 0.6),
            pos2(c.x + s, c.y - s * 0.6),
            pos2(c.x, c.y + s * 0.7),
        ]
    } else {
        vec![
            pos2(c.x - s * 0.6, c.y - s),
            pos2(c.x + s * 0.7, c.y),
            pos2(c.x - s * 0.6, c.y + s),
        ]
    };
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

/// Type badge at the right end of the row.
fn badge(painter: &egui::Painter, row_rect: Rect, row: &Row, color: Color32) {
    let text = match &row.item {
        RowItem::Value(v) => match v.child.kind {
            braceray_core::nav::ValueKind::Object => "object",
            braceray_core::nav::ValueKind::Array => "array",
            braceray_core::nav::ValueKind::String => "string",
            braceray_core::nav::ValueKind::Number => "number",
            braceray_core::nav::ValueKind::Bool => "bool",
            braceray_core::nav::ValueKind::Null => "null",
        },
        RowItem::Group { .. } => "group",
        RowItem::Loading { .. } => return,
    };
    let pos = pos2(row_rect.right() - 10.0, row_rect.center().y);
    let r = painter.text(
        pos,
        Align2::RIGHT_CENTER,
        text,
        FontId::proportional(11.0),
        color,
    );
    painter.rect_stroke(
        r.expand2(vec2(5.0, 1.0)),
        4.0,
        Stroke::new(1.0, color.gamma_multiply(0.4)),
        StrokeKind::Outside,
    );
}

fn context_menu(
    ui: &mut egui::Ui,
    tree: &mut TreeView,
    index: u64,
    row: &Row,
    bytes: &[u8],
    actions: &mut Vec<Action>,
    repaint: &Repaint,
) {
    tree.selected = Some(index);
    if row.expandable() {
        let label = if row.expanded { "Collapse" } else { "Expand" };
        if ui.button(label).clicked() {
            tree.toggle(index, repaint);
            ui.close();
        }
        if ui.button("Expand all below").clicked() {
            tree.expand_all(index, u32::MAX, repaint);
            ui.close();
        }
        ui.separator();
    }
    let (key, value) = match &row.item {
        RowItem::Value(v) => (v.child.key, Some(v.child.value)),
        _ => (None, None),
    };
    let key_text = key.map(|k| {
        let raw = k.slice(bytes);
        let raw = raw.strip_prefix(b"\"").unwrap_or(raw);
        String::from_utf8_lossy(raw.strip_suffix(b"\"").unwrap_or(raw)).into_owned()
    });
    if ui
        .add_enabled(key_text.is_some(), egui::Button::new("Copy key"))
        .clicked()
    {
        actions.extend(key_text.map(Action::Copy));
        ui.close();
    }
    let size = value.map_or(0, |v| v.end - v.start);
    let copy = ui
        .add_enabled(
            value.is_some() && size <= COPY_LIMIT,
            egui::Button::new("Copy value"),
        )
        .on_disabled_hover_text(format!(
            "Too big to copy ({}); the limit is {}.",
            format::bytes(size),
            format::bytes(COPY_LIMIT)
        ));
    if copy.clicked() {
        if let Some(v) = value {
            actions.push(Action::Copy(
                String::from_utf8_lossy(v.slice(bytes)).into_owned(),
            ));
        }
        ui.close();
    }
    ui.add_enabled(false, egui::Button::new("Copy path"))
        .on_disabled_hover_text("JSONPath / jq / JavaScript paths arrive with the breadcrumb bar.");
    if ui.button(SHOW_IN_RAW).clicked() {
        actions.push(Action::ShowInRaw);
        ui.close();
    }
}
