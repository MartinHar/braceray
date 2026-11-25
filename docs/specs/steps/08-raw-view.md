# Step 08 — Raw text view (highlighted, on-the-fly pretty print)

**Phase:** MVP · **Depends on:** 04, 07

## Goal
A virtualized text view of the file with syntax highlighting. It shows minified (single-line) files pretty-printed on the fly, stays in sync with the tree, and supports "Go to line" and "Go to offset".

## Modes
1. **Original:** the file's own lines. Long lines are soft-wrapped at a display width (default 400 chars per visual row, with a ↩ marker). Line numbers come from the line table.
2. **Pretty (auto for minified files):** the view generates formatted lines **only for the visible region**. Pretty printing starts from a known structural position (a node start or an element checkpoint) and emits indented tokens until the viewport is filled. Virtual line numbers are not global (counting them for 2 GB would need a full pass); the gutter shows the **byte offset** and `line:col` of the source position instead. A toggle switches modes; the default is Pretty when the average line length > 2,000 bytes.

## Implementation notes
- `braceray-core::pretty`: `fn pretty_lines(doc, index, start: StructuralPos, max_lines: usize, indent: u8) -> Vec<PrettyLine>`, where `PrettyLine { depth, tokens: SmallVec<Token>, source_span }`. It's pure and testable.
- **Scrolling** is anchored to a byte offset, not a line number:
  - The scrollbar maps 0..file_len ↔ position. Dragging the thumb to 63 % → find the nearest structural position ≥ 63 % (node table binary search or a local scan) and render from there.
  - Wheel scrolling moves by rendered lines; when it runs out of rendered lines, render more from the last position (forward) or from the previous anchor (backward: step back to the enclosing node start or an earlier checkpoint, render forward, keep the tail).
- **Highlighting:** a tokenizer over the visible bytes only (keys, strings, numbers, literals, punctuation), using the same palette as the tree. Search matches get highlighted backgrounds (wired in step 10).
- **Sync:** selecting in the tree scrolls the raw view so the node is near the top and briefly flashes it. Clicking in the raw view selects the innermost node at that offset in the tree (via `node_at_offset`) and expands the tree path to it.
- **Text selection & copy:** selecting text within the rendered region and `Ctrl/Cmd+C` copies the source bytes of that span (cap 50 MB with a warning).
- **Go to:** `Ctrl/Cmd+L` dialog accepting `line`, `line:col`, `@offset` (bytes, also `@1.5g`, `@200m`), or a JSONPath-like simple path `$.a.b[3]` (resolved via index navigation; full JSONPath comes in step 14).
- Huge single strings: render visible slices only (never decode all 200 MB).

## Acceptance criteria
- [ ] `orders_min_2g.json` (single line): opens in Pretty mode; scrolling and thumb dragging stay at 60 fps.
- [ ] Pretty output re-parsed equals the source value (test on fixtures ≤ 10 MB, comparing serde values).
- [ ] Original mode on the pretty 2 GB fixture shows correct line numbers; "Go to line 12,345,678" lands correctly.
- [ ] Tree ↔ raw sync works both ways, including deep and grouped nodes.
- [ ] `huge_string.json` scrolls through the string without freezing.
- [ ] Copy of a selected span gives the exact source bytes.

## Tests
Core unit tests for `pretty_lines` (indentation, empty containers, escapes, UTF-8 split safety at slice edges), proptest round-trip, fuzz target `pretty_range`. kittest: toggle mode, go-to-line, tree click scrolls the raw view.
