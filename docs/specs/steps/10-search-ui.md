# Step 10 — Search UI

**Phase:** MVP · **Depends on:** 07, 08, 09

## Goal
The search experience users came for: type, see a live match count, browse every result with its JSON path, and jump to any of them in the tree and the raw view.

## UI
- **Search bar** (top, `Ctrl/Cmd+F` focuses it and selects its text):
  `[🔍 search text…            ] [Aa] [ab|] [.*]  [Keys|Values|All]   18,442 matches  ‹ ›  ✕`
  - `Aa` = case-sensitive, `ab|` = whole word (MVP). `.*` and the scope selector appear disabled with a tooltip "Coming in v1" until step 13 (or hidden; your choice, note it in DECISIONS.md).
  - Search starts **as you type**, debounced 150 ms; Enter = next, Shift+Enter = previous. Esc clears or closes.
  - Count area: "Searching… 1,204 matches (37 %)" → "18,442 matches" → "No matches". It shows "match 312 of 18,442" when one is selected.
  - Search history dropdown (last 20, persisted).
- **Results panel** (bottom, collapsible, resizable, toggled with `Ctrl/Cmd+Shift+F`):
  - Virtualized list; each row: `#312  $.orders[48213].customer.email   …anna.p@**example**.com…   line 1,204,331`
  - Path and preview computed lazily for visible rows (LRU cache) with a placeholder while computing.
  - Click → select the match: the tree expands the path and selects the node, the raw view scrolls to it with the match highlighted.
  - Keyboard: ↑/↓ in the list, Enter jumps.
  - Header: total count, "showing first 5,000,000" if capped, a "Copy all paths" action (writes to a temp file if > 10k rows and offers to open it).
- **Highlighting:** every visible occurrence in the raw view gets a soft background; the current match gets a strong accent background. In the tree, rows containing a match show a small marker, and the current match's row is highlighted.
- **Navigation:** F3 / Shift+F3 (and Cmd+G / Cmd+Shift+G on macOS) go to next/previous from the **current selection's offset**, wrapping around with a subtle "Wrapped to top" toast.
- Status bar shows the count and current index.

## Acceptance criteria
- [ ] Typing "needle_common" in the 2 GB file: the first results appear in < 200 ms; the final count matches the manifest.
- [ ] The results list scrolls smoothly through millions of rows.
- [ ] Clicking any result (first, middle, last, beyond the stored cap via F3) lands on the right node in the tree and the right bytes in the raw view.
- [ ] Changing a toggle re-runs the search instantly; typing cancels the previous search (no stale results).
- [ ] Search works while indexing is still running; paths fill in when the index is ready.
- [ ] All keyboard shortcuts work on Windows and macOS.

## Tests (kittest + core)
Search → count label; Enter/F3 move the selection; result click selects the node; toggles re-run; Esc clears. Core test: path for each of the first 1000 matches equals the serde walk path (small fixture).
