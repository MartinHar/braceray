# Step 07 — Tree view (lazy, virtualized)

**Phase:** MVP · **Depends on:** 04, 06

## Goal
A fast, beautiful tree of the JSON that scrolls at 60 fps on any file size, expands nodes with millions of children instantly, and only ever touches the bytes on screen.

## Design
- **Flattened visible-row model.** The tree is rendered as a flat list of visible rows. Rows are produced from `expanded: HashSet<NodeKey>` + index navigation. Don't materialise all rows. Keep a **row cursor**: for scroll position R, compute the rows R..R+visible by walking from the nearest cached anchor. Store an anchor every 1,000 rows of the flattened list and invalidate anchors after the changed row when a node expands/collapses.
  - Simpler acceptable v1: maintain `Vec<RowSpan>` segments ("node X, children a..b") instead of rows; total row count = sum of segment lengths. Pick either approach, but scrolling must stay O(visible rows) and expanding a 1M-child node must not allocate per child.
- **Grouping:** containers with more than `group_size` (default 1000, setting) children show group rows `[0 … 999]`, `[1000 … 1999]`… which expand to children (nested groups if > group_size² children). Use element checkpoints (step 04) to jump to a group's first child in O(1).
- **Row content** (one line, monospace, fixed height):
  `▸ key : value-preview    {12} / [3,401]    ·  type badge`
  - key in accent colour, string values green, numbers blue, booleans/null purple (theme-aware palette)
  - value preview: strings truncated to ~120 chars (bytes decoded lossily, escapes shown as written), containers show `{…}` / `[…]` with child count
  - array children labelled by index `[42]`
- **Before indexing completes (preview mode):** show the root and its first children found by a quick sequential scan of the first few MB, labelled "Preview: indexing… 37 %". Switch seamlessly to the real index when ready, keeping the expanded state and selection.
- **Selection & keyboard:** click selects; ↑/↓ move; → expands or moves to the first child; ← collapses or moves to the parent; Home/End; PageUp/PageDown; `*` expands all children of the selection (limit 10,000 rows, then ask); Enter toggles.
- **Context menu (right-click):** Expand / Collapse, Expand all below (guarded), Copy key, Copy value, Copy path ▸ (JSONPath / jq / JS, implemented in step 11; leave the hook), Show in raw view.
- **Selection model** shared across views: `Selection { node_or_value_span, path_cache }`. Selecting in the tree tells the raw view (step 08) to scroll there.
- Large single values (e.g. a 200 MB string) show a size badge and "Show in raw view"; never decode them fully.

## Acceptance criteria
- [ ] `orders_2g.json`: expanding the root array (millions of elements, grouped) takes **< 200 ms**.
- [ ] `prim_array_10m.json` and `wide_1m.json`: expand in < 200 ms; scrolling to the end is smooth.
- [ ] Scrolling with the mouse wheel and dragging the scrollbar from top to bottom of a fully expanded big array: frames stay < 16 ms (log frame times in debug overlay `Ctrl+Shift+D`).
- [ ] `deep_100k.json`: expanding 100,000 levels by keyboard works without stack overflow.
- [ ] Preview mode shows content within 1 s of opening the 2 GB file.
- [ ] Unicode keys and values render correctly (Armenian, Cyrillic, CJK, emoji); invalid UTF-8 inside strings renders with � and never panics.
- [ ] All keyboard shortcuts work; context menu actions work.

## Tests
- Core: the row-model unit tests (row count after expand/collapse, row at index N equals the expected key path) against serde walks of small fixtures.
- kittest: expand/collapse by click and keyboard; group rows appear for > 1000 children; selection moves.
- Bench: `rows_for_viewport` on a 10M-child array (must be µs-level).
