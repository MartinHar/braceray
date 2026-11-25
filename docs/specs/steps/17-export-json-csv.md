# Step 17 — Export: matching nodes → JSON, arrays → CSV · release v1.0.0

**Phase:** v1 · **Depends on:** 09, 13, 14, 16

## Goal
Get data **out**: save search or JSONPath results as a new JSON file, or turn an array of objects into a CSV that opens in Excel or Google Sheets. Streaming, at disk speed, with progress and cancel.

## Export results → JSON
- Entry points: results panel header "Export…", context menu on the selection "Export node…".
- Options dialog:
  - **What to export per match:** the matched value · the nearest enclosing object · the top-level record/element containing the match (default when the root is an array or NDJSON). Duplicates are removed (several matches in the same object → exported once).
  - **Format:** JSON array (`[ … ]`) · NDJSON (one per line) · keep the original formatting / pretty / minified.
  - Optionally include the path: wrap each item as `{"path": "...", "value": ...}`.
- Implementation: **copy raw byte ranges** from the mmap into a `BufWriter` (16 MiB) → no parsing needed for "original" format; streaming pretty/minify via `pretty.rs` otherwise. Write to `<target>.part` and rename on success. Cancel deletes the partial file.
- Must run in the background with progress in the status bar; the app stays usable.

## Export array of objects → CSV
- Available when the selected node (or JSONPath result set) is an array of objects, or the file is NDJSON.
- **Column discovery:** sample the first 10,000 elements (fast) and gather keys in first-seen order; checkbox "Scan all elements for columns" (exact, slower, with progress). The user can reorder and untick columns in the dialog.
- **Nested values:** option A (default) — nested objects/arrays as compact JSON text in the cell; option B — flatten one level of nested objects with dot keys (`customer.email`).
- Values: strings unescaped (JSON escapes decoded); numbers as written; `true`/`false`; `null` → empty cell (configurable `null` text).
- RFC 4180 quoting (`"` doubled, fields with `,` `"` `\n` `\r` quoted). Delimiter `,` / `;` / tab. Line ending CRLF (default for Excel) / LF. Optional UTF-8 BOM (default **on**, so Excel shows Unicode correctly).
- Missing keys → empty cells. Show a summary at the end: rows written, columns, elements skipped (not objects), time.
- Excel row limit note: if rows > 1,048,576, warn that Excel will truncate (the file is still written fully).

## Acceptance criteria
- [ ] Exporting 1,000,000 matched records from the 2 GB file runs at ≥ 500 MB/s of output in original format, and the output parses with serde as a valid array / NDJSON.
- [ ] Pretty/minified exports re-parse to the same values.
- [ ] CSV from `orders` (array of objects) opens in LibreOffice/Excel with correct Unicode, quotes and newlines; a CSV parser round-trip test (`csv` crate as oracle) passes.
- [ ] Cancel removes the partial file; disk-full errors are reported clearly.
- [ ] Version bumped to **1.0.0**, CHANGELOG updated, release via the tag workflow, PERF.md updated with v1 numbers (regex, JSONPath, export).

## Tests
Core oracle tests for both exporters (small fixtures, proptest for CSV escaping), dedup logic, cancel behaviour, kittest for the dialogs.
