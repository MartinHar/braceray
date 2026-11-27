//! Keyboard navigation of the tree.

use braceray_core::job::Repaint;

use super::TreeView;

impl TreeView {
    /// Handles a navigation key; `page` = rows per page. Returns `true` if it was used.
    pub fn key(&mut self, key: TreeKey, page: u64, repaint: &Repaint) -> bool {
        let count = self.model.row_count();
        if count == 0 {
            return false;
        }
        let Some(sel) = self.selected else {
            self.select(0);
            return true;
        };
        match key {
            TreeKey::Up => self.select(sel.saturating_sub(1)),
            TreeKey::Down => self.select(sel + 1),
            TreeKey::Home => self.select(0),
            TreeKey::End => self.select(count - 1),
            TreeKey::PageUp => self.select(sel.saturating_sub(page.max(1))),
            TreeKey::PageDown => self.select(sel + page.max(1)),
            TreeKey::Right => {
                if self.model.is_expanded(sel) {
                    self.select(sel + 1);
                } else {
                    self.expand(sel, repaint);
                    self.scroll_to = Some(sel);
                }
            }
            TreeKey::Left => {
                if self.model.is_expanded(sel) {
                    self.collapse(sel);
                } else if let Some(p) = self.model.parent(sel) {
                    self.select(p);
                }
                self.scroll_to = self.selected;
            }
            TreeKey::Toggle => self.toggle(sel, repaint),
            TreeKey::ExpandChildren => self.expand_all(sel, 2, repaint),
        }
        true
    }
}

/// Keys the tree reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeKey {
    /// ↑
    Up,
    /// ↓
    Down,
    /// → expand, or go to the first child
    Right,
    /// ← collapse, or go to the parent
    Left,
    /// Home
    Home,
    /// End
    End,
    /// PageUp
    PageUp,
    /// PageDown
    PageDown,
    /// Enter
    Toggle,
    /// `*`
    ExpandChildren,
}
