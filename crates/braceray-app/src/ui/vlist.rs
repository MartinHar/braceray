//! Pieces shared by the virtual lists (tree, results): a scrollbar over an `f64` row
//! offset, so millions of rows keep pixel precision.

use eframe::egui::{self, Rect, Role, Sense, pos2, vec2};

/// Width of the scrollbar.
pub const SCROLLBAR: f32 = 12.0;

/// Draws a scrollbar in the strip right of `list` and moves `scroll` (in rows) by
/// dragging or clicking it. Nothing is drawn when every row fits.
#[allow(clippy::too_many_arguments)]
pub fn scrollbar(
    ui: &mut egui::Ui,
    rect: Rect,
    list: Rect,
    scroll: &mut f64,
    total: u64,
    full_rows: u64,
    id: egui::Id,
    label: &str,
) {
    if total <= full_rows {
        return;
    }
    let max_scroll = total.saturating_sub(full_rows) as f64;
    let track = Rect::from_min_max(pos2(list.right(), rect.top()), rect.max);
    let response = ui.interact(track, id, Sense::click_and_drag());
    super::label_icon(&response, Role::ScrollBar, label);
    let thumb_h = (track.height() * full_rows as f32 / total as f32).clamp(24.0, track.height());
    let free = (track.height() - thumb_h).max(1.0);
    if let Some(p) = response.interact_pointer_pos()
        && (response.dragged() || response.clicked() || response.is_pointer_button_down_on())
    {
        let t = ((p.y - track.top() - thumb_h / 2.0) / free).clamp(0.0, 1.0);
        *scroll = f64::from(t) * max_scroll;
    }
    let t = if max_scroll > 0.0 {
        (scroll.clamp(0.0, max_scroll) / max_scroll) as f32
    } else {
        0.0
    };
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
