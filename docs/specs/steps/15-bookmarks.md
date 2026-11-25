# Step 15 — Bookmarks

**Phase:** v1 · **Depends on:** 07, 11

## Goal
Mark important nodes, name them, and get back to them instantly, even after closing the app.

## Behaviour
- `Ctrl/Cmd+B` or the context menu → "Toggle bookmark" on the selected node. A ★ appears in the tree gutter and the raw view gutter.
- Optional label (double-click the bookmark to rename; default label = last path segment).
- **Bookmarks panel** (left side, collapsible, `Ctrl/Cmd+Shift+B`): list of `★ label · $.path · line`, click to jump, drag to reorder, delete (Del key / ✕), "Copy all as JSONPath list".
- Navigation: `F2` / `Shift+F2` = next/previous bookmark in document order.

## Storage
- Persist in the app data dir, `bookmarks.json`: map keyed by **file identity** (canonical path + size + mtime + head/tail hash from `FileIdentity`).
- Each bookmark: `{ label, jsonpath, byte_offset, created_at }`.
- On open: if the identity matches exactly, use the byte offsets. If the same path has a different identity (file changed), re-resolve each bookmark by its JSONPath via the step-14 evaluator; mark unresolved ones as "⚠ not found" (kept until the user deletes them).
- Limit: 10,000 bookmarks per file; total store size is trimmed by LRU file (keep the last 200 files).

## Acceptance criteria
- [ ] Add, rename, jump, reorder, delete and next/prev all work.
- [ ] Bookmarks survive an app restart.
- [ ] After the file is regenerated with the same structure (different mtime), bookmarks re-resolve by path.
- [ ] Bookmarks on nodes deep inside grouped arrays jump correctly.

## Tests
Core: storage round-trip, identity matching, re-resolution. kittest: toggle bookmark, panel lists it, click jumps.
