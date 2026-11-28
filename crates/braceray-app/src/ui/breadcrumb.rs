//! The breadcrumb bar under the toolbar: `$ › orders › [48213] › customer › email`.
//!
//! Each segment selects its value. Hovering a segment opens a list of its siblings (the
//! first [`SIBLINGS`] children of its parent, virtualized) for lateral moves. Paths that
//! don't fit collapse in the middle (`$ › … › customer › email`); `…` shows every level.
//! At the right end: "Copy <format>" and the format menu, copy progress and notices.
//! Work per frame is proportional to the segments shown, not to the depth.

use braceray_core::job::Repaint;
use braceray_core::path::{PathFormat, format_path};
use braceray_core::tree::RowItem;
use eframe::egui::{self, Align, Layout, Rect, RichText, Role};

use super::{Action, label_icon};
use crate::copy_job::CopyState;
use crate::format;
use crate::theme;
use crate::tree_view::{SIBLINGS, TreeView};

/// Hover time before a segment opens its sibling list (seconds).
pub const HOVER_DELAY: f64 = 0.3;
/// Accessible name of the `…` that shows every level.
pub const SHOW_ALL: &str = "Show all levels";
/// Accessible name of the format menu button.
pub const FORMAT_MENU: &str = "Path format";
/// Accessible name of the cancel button of a running copy.
pub const CANCEL_COPY: &str = "Cancel copy";
/// Segments shown when every level is shown (head and tail halves beyond that).
pub const MAX_EXPANDED: usize = 400;
/// Most height the bar takes when every level is shown (it scrolls beyond).
const EXPANDED_HEIGHT: f32 = 132.0;
/// Height of one sibling row.
const SIBLING_ROW: f32 = 20.0;

/// Label of the copy-path button.
pub fn copy_label(format: PathFormat) -> String {
    format!("Copy {}", format.label())
}

/// Breadcrumb state of one tab.
#[derive(Debug, Default)]
pub struct BreadcrumbState {
    /// Every level is shown for this selected row.
    expanded: Option<u64>,
    /// Segment hovered and since when.
    hover: Option<(usize, f64)>,
    /// The open sibling list.
    open: Option<OpenList>,
}

#[derive(Debug, Clone, Copy)]
struct OpenList {
    /// Row of the segment whose siblings are listed.
    row: u64,
    /// Row of its parent.
    parent: u64,
    /// The segment's rect.
    anchor: Rect,
    /// The list's rect (last frame).
    popup: Option<Rect>,
}

impl BreadcrumbState {
    /// `true` while a sibling list is open.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

/// What the bar needs besides the tree.
pub struct BarContext<'a> {
    /// Stable id of the tab.
    pub tab_id: u64,
    /// Wakes the UI when a prepare job finishes.
    pub repaint: &'a Repaint,
    /// Format of the copy button.
    pub format: PathFormat,
    /// Root name of JavaScript paths.
    pub js_root: &'a str,
}

/// Draws the bar.
pub fn show(
    ui: &mut egui::Ui,
    tree: &mut TreeView,
    state: &mut BreadcrumbState,
    copy: &mut CopyState,
    cx: &BarContext<'_>,
    actions: &mut Vec<Action>,
) {
    let rows = tree.crumb_rows();
    if state.expanded.is_some() && state.expanded != tree.selected {
        state.expanded = None;
    }
    if let Some(open) = state.open
        && !rows.contains(&open.row)
        && !rows.contains(&open.parent)
    {
        state.open = None;
    }
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            right_controls(ui, tree, copy, cx, actions);
            if state.expanded.is_some() {
                // Every level: wrapped lines, scrolling past a few of them.
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(EXPANDED_HEIGHT)
                        .min_scrolled_height(EXPANDED_HEIGHT)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                segments(ui, tree, state, &rows, cx.repaint);
                            });
                        });
                });
            } else {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    segments(ui, tree, state, &rows, cx.repaint);
                });
            }
        });
    });
    siblings_popup(ui, tree, state, cx);
}

fn right_controls(
    ui: &mut egui::Ui,
    tree: &TreeView,
    copy: &mut CopyState,
    cx: &BarContext<'_>,
    actions: &mut Vec<Action>,
) {
    let menu = ui.menu_button("▾", |ui| {
        for f in PathFormat::ALL {
            let mark = if f == cx.format { "✔ " } else { "    " };
            if ui.button(format!("{mark}{}", copy_label(f))).clicked() {
                actions.push(Action::CopyPath(Some(f)));
                ui.close();
            }
        }
    });
    label_icon(&menu.response, Role::Button, FORMAT_MENU);
    let has_selection = tree.selected.is_some();
    let button = ui.add_enabled(has_selection, egui::Button::new(copy_label(cx.format)));
    let button = button.on_hover_ui(|ui| {
        let text = tree
            .selected_path()
            .map(|p| format_path(tree.document().bytes(), &p, cx.format, cx.js_root));
        ui.label(RichText::new(text.unwrap_or_default()).monospace());
    });
    if button.clicked() {
        actions.push(Action::CopyPath(None));
    }
    if let Some((fraction, what)) = copy.progress() {
        let cancel = ui.small_button("×");
        label_icon(&cancel, Role::Button, CANCEL_COPY);
        if cancel.clicked() {
            actions.push(Action::CancelCopy);
        }
        ui.add(
            egui::ProgressBar::new(fraction)
                .desired_width(120.0)
                .text(format!("{what} {:.0} %", fraction * 100.0)),
        );
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    } else if let Some(n) = &copy.notice {
        let color = if n.error {
            ui.visuals().error_fg_color
        } else {
            theme::accent(ui)
        };
        ui.label(RichText::new(&n.text).color(color).italics());
        let left = n.until.saturating_duration_since(std::time::Instant::now());
        ui.ctx().request_repaint_after(left);
    }
}

/// Which segments to draw: indices into the chain, `None` for a `…`.
fn layout(ui: &egui::Ui, tree: &TreeView, rows: &[u64], expanded: bool) -> Vec<Option<usize>> {
    let n = rows.len();
    if expanded {
        if n <= MAX_EXPANDED {
            return (0..n).map(Some).collect();
        }
        let half = MAX_EXPANDED / 2;
        return (0..half)
            .map(Some)
            .chain([None])
            .chain((n - half..n).map(Some))
            .collect();
    }
    let font = egui::TextStyle::Button.resolve(ui.style());
    let pad = ui.spacing().button_padding.x * 2.0 + ui.spacing().item_spacing.x * 2.0 + 12.0;
    let width = |i: usize| {
        let label = tree
            .value_row(rows[i])
            .map_or_else(String::new, |v| tree.crumb_label(&v));
        let galley = ui.fonts_mut(|f| f.layout_no_wrap(label, font.clone(), egui::Color32::WHITE));
        galley.size().x + pad
    };
    let budget = ui.available_width();
    let mut used = 0.0;
    let mut tail = Vec::new();
    for i in (0..n).rev() {
        let w = width(i);
        // Keep room for `$ › … ›` when something is left out.
        let reserve = if i > 0 { 2.0 * pad + 20.0 } else { 0.0 };
        if used + w + reserve > budget && !tail.is_empty() {
            break;
        }
        used += w;
        tail.push(i);
    }
    tail.reverse();
    match tail.first() {
        Some(0) | None => tail.into_iter().map(Some).collect(),
        Some(_) => [Some(0), None]
            .into_iter()
            .chain(tail.into_iter().map(Some))
            .collect(),
    }
}

fn segments(
    ui: &mut egui::Ui,
    tree: &mut TreeView,
    state: &mut BreadcrumbState,
    rows: &[u64],
    repaint: &Repaint,
) {
    if rows.is_empty() {
        ui.label(RichText::new("No selection").weak());
        return;
    }
    let shown = layout(ui, tree, rows, state.expanded.is_some());
    let now = ui.input(|i| i.time);
    let mut hovered = None;
    let mut select = None;
    for (k, item) in shown.iter().enumerate() {
        if k > 0 {
            ui.label(RichText::new("›").weak());
        }
        let Some(i) = *item else {
            let hidden = rows.len() - (shown.len() - 1);
            let dots = ui
                .add_enabled(
                    state.expanded.is_none(),
                    egui::Button::new("…").frame(false),
                )
                .on_hover_text(format!(
                    "{} more levels",
                    format::group_digits(hidden as u64)
                ));
            label_icon(&dots, Role::Button, SHOW_ALL);
            if dots.clicked() {
                state.expanded = tree.selected;
            }
            continue;
        };
        let Some(v) = tree.value_row(rows[i]) else {
            // Not readable until its container is prepared (a 1M-child array right after
            // a jump): a placeholder, and make sure the prepare job runs.
            if let Some(RowItem::Loading { container, .. }) = tree.row(rows[i]).map(|r| r.item) {
                tree.request_prepare(container, repaint);
            }
            ui.add(egui::Spinner::new().size(10.0));
            continue;
        };
        let label = tree.crumb_label(&v);
        let last = i + 1 == rows.len();
        let text = if last {
            RichText::new(label).strong()
        } else {
            RichText::new(label)
        };
        let r = ui.add(egui::Button::selectable(
            last && tree.selected == Some(rows[i]),
            text,
        ));
        if r.clicked() {
            select = Some(rows[i]);
        }
        if r.hovered() && i > 0 {
            hovered = Some((i, r.rect));
        }
    }
    if let Some(row) = select {
        tree.select(row);
        state.open = None;
    }
    // Hover → open the sibling list after a short delay.
    match hovered {
        Some((i, rect)) => {
            let since = match state.hover {
                Some((h, t)) if h == i => t,
                _ => {
                    state.hover = Some((i, now));
                    now
                }
            };
            let waited = now - since;
            let already = state.open.is_some_and(|o| o.row == rows[i]);
            if waited >= HOVER_DELAY && !already {
                state.open = Some(OpenList {
                    row: rows[i],
                    parent: rows[i - 1],
                    anchor: rect,
                    popup: None,
                });
            } else if !already {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(
                        (HOVER_DELAY - waited).max(0.01),
                    ));
            }
        }
        None => state.hover = None,
    }
}

fn siblings_popup(
    ui: &mut egui::Ui,
    tree: &mut TreeView,
    state: &mut BreadcrumbState,
    cx: &BarContext<'_>,
) {
    let Some(open) = state.open else {
        return;
    };
    let ctx = ui.ctx().clone();
    // Close when the pointer leaves the segment and the list, or on Esc.
    let pointer = ctx.input(|i| i.pointer.hover_pos());
    let inside = |p: egui::Pos2| {
        open.anchor.expand(4.0).contains(p)
            || open.popup.is_some_and(|r| r.expand(8.0).contains(p))
            || Rect::from_min_max(
                open.anchor.left_bottom(),
                open.anchor.right_bottom() + egui::vec2(0.0, 8.0),
            )
            .contains(p)
    };
    if pointer.is_some_and(|p| !inside(p)) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.open = None;
        return;
    }
    let (Some(parent), Some(current)) = (tree.value_row(open.parent), tree.value_row(open.row))
    else {
        state.open = None;
        return;
    };
    let Some(container) = parent.container else {
        state.open = None;
        return;
    };
    let siblings = tree.siblings(container, cx.repaint);
    let mut chosen = None;
    let area = egui::Area::new(egui::Id::unique(("crumb_siblings", cx.tab_id)))
        .order(egui::Order::Foreground)
        .fixed_pos(open.anchor.left_bottom())
        .show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(160.0);
                ui.set_max_width(360.0);
                let Some((list, count)) = siblings else {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading…");
                    });
                    return;
                };
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .auto_shrink([true, true])
                    .show_rows(ui, SIBLING_ROW, list.len(), |ui, range| {
                        for s in &list[range] {
                            let on = s.index == current.index;
                            if ui.add(egui::Button::selectable(on, &s.label)).clicked() {
                                chosen = Some(s.index);
                            }
                        }
                    });
                if count > SIBLINGS {
                    ui.label(
                        RichText::new(format!("… {} more", format::group_digits(count - SIBLINGS)))
                            .weak(),
                    );
                }
            });
        });
    if let Some(o) = state.open.as_mut() {
        o.popup = Some(area.response.rect);
    }
    if let Some(i) = chosen {
        if let Some(mut address) = tree.model.address(open.parent) {
            address.push(i);
            tree.reveal_address(address, cx.repaint);
        }
        state.open = None;
    }
}
