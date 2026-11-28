# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Cargo workspace with `braceray-core`, `braceray-app` (binary `braceray`) and `tools/fixtures` (`gen-fixtures`).
- Empty "Braceray" window showing "Drop a JSON file here".
- CI on Windows, macOS and Linux (fmt, clippy, release build) plus a `cargo deny` job. Tests run locally only for now.
- MIT license, README, placeholder app icon.
- `gen-fixtures`: deterministic, parallel test-data generator (`all`, `orders`, `ndjson`, `deep`, `wide`, `prim`, `string`, `invalid`, `encoding`, `diff`) with needle, invalid-position, encoding and diff manifests. 2 GB in ~1.5 s.
- Golden test fixtures in `crates/braceray-core/tests/fixtures/` (valid edge cases, invalid, encoding, small generated samples).
- `braceray_core::Document`: opens files of any size (tested with 5 GiB) in O(1) via a read-only memory map; skips a UTF-8 BOM; rejects UTF-16/32 (with convert commands for macOS and Windows), binary, empty, whitespace-only and non-JSON files with clear messages; records a `FileIdentity` (canonical path, size, mtime, xxh3 of the first and last 1 MiB) and detects changes on disk; sniffs JSON vs NDJSON.
- Structural index (reference implementation): RFC 8259 validation with an explicit stack (any depth), exact error position ("Invalid JSON at line 1, column 17 (offset 20): expected ',' or '}', found '\"'"), 24-byte container nodes, line table, cancellation and progress.
- SIMD block classifier (NEON, SSE2, AVX2, `pclmulqdq`, scalar fallback) with runtime CPU detection: 5.25 GB/s single-threaded on Apple M3 Pro.
- Parallel index builder (`build_index`): indexes and validates a 2 GB file in ~1.2 s on all cores, with exactly the same result and error messages as the reference; adaptive mode keeps the index under 25 % of the file; cancellable within 50 ms.
- Navigation (`nav::Navigator`): root, children of any range (checkpoint every 1024 children), child counts, parents, value and JSON path at any byte offset, line/column; works in adaptive mode too.
- Criterion benchmarks (`cargo bench -p braceray-core`) and a `cargo-fuzz` target `index_any_bytes`.
- App shell: open files with the toolbar button or Ctrl/Cmd+O (native dialog, JSON / NDJSON / all files), drag & drop (with a drop highlight), the command line (`braceray file.json`) or the recent files list (last 15; missing files greyed out with "Remove"; "Clear recent").
- Tabs for several files (close with ×, middle-click or Ctrl/Cmd+W); opening a file that is already open focuses its tab.
- Indexing progress with percentage, speed, time left and Cancel; on completion "Indexed 2.15 GB in 3.2 s · 19,603,166 nodes". The window stays responsive while indexing.
- Error banner for invalid JSON (line, column, offset, copyable details, "Open anyway"); clear messages for empty, UTF-16/32, binary and missing files.
- Status bar: file size, index source ("Index: from cache (0.04 s)" / "Index: built (3.2 s)"), line:col of the selection, process memory.
- Dark / Light / System theme with a cyan accent; settings window (theme, index cache on/off, max cache size, min file size, "Clear cache (size)", max stored search results, tree group size), saved to a JSON file in the OS config dir; bad values reset to defaults.
- "File changed on disk. Reload?" banner (checked every 5 s and when the window gets focus).
- Keyboard shortcuts dialog (`?`).
- Debug builds can be driven by Claude Code through `egui_mcp` (egui inspection protocol on 127.0.0.1:5719).
- `braceray_core::job::{JobHandle, JobEvent, Repaint}`: background jobs with partial results, progress, cancellation and a repaint callback.
- Index cache (`braceray_core::cache`), on by default: the second open of a file loads its index from the OS cache dir (2 GB file: 14 ms instead of ~1–3 s). Stale, corrupted or old-format entries are detected, deleted and rebuilt. Entries are written in the background with temp file + atomic rename (safe with several app instances), and least-recently-used entries are evicted beyond 4 GiB. Settings: on/off, max size, minimum file size (20 MiB); `clear_index_cache()`.

- Tree view: a virtualized tree of the whole document that scrolls smoothly at any size (10 M rows: frames under 1.2 ms). Rows show `key : value` with type colours (keys cyan, strings green, numbers blue, `true`/`false`/`null` purple), child counts (`{12}`, `[3,401]`), a type badge and a size badge for values over 1 MB. Strings are cut at 120 characters; Unicode (Armenian, Cyrillic, CJK, emoji) renders through the system fonts, and invalid UTF-8 shows as `�`.
- Huge arrays and objects are grouped: `[0 … 999]`, `[1,000 … 1,999]`, … (nested above 1,000,000 children; group size in Settings). Expanding a 2.45 M-element root is instant; opening a group deep inside it reads children in the background ("loading…").
- Preview while indexing: the tree of the first 4 MB appears within ~150 ms of opening a 2 GB file ("Preview: indexing… 37 %"), then switches to the full index keeping what was expanded and selected.
- Tree keyboard: ↑/↓, ← (collapse or go to parent), → (expand or go to first child), Home/End, PageUp/PageDown, Enter (toggle), `*` (expand the selection and its children, asks before adding more than 10,000 rows). Click selects, the triangle or a double click toggles.
- Tree context menu: Expand / Collapse, Expand all below (asks every 10,000 rows), Copy key, Copy value.
- Debug overlay (`Ctrl+Shift+D`): frame times (p50 / p99 / max), rows drawn, time to first rows, tree size.
- `braceray_core::tree` (row model, preview index) and `nav::NavCache` (navigation cache shared with background jobs, non-blocking `children_if_ready` / `child_count_if_ready`, cancellable `prepare`).
- Raw text view next to the tree (resizable split): syntax-highlighted, virtualized, scrolls smoothly through any file (2 GB single-line file: frames under 16 ms while dragging the scrollbar end to end).
  - **Pretty** mode formats minified files on the fly, only for the rows on screen; it is the default when lines are longer than 2,000 bytes on average. The gutter shows each row's byte offset (hover: line and column).
  - **Original** mode shows the file's own lines with line numbers; lines over 400 bytes wrap (↩).
  - The scrollbar maps to byte offsets: dragging it to 63 % shows the row at 63 % of the file at once, in either mode. Wheel, ↑/↓, PageUp/PageDown, Home/End scroll it when it has the focus.
  - A 200 MB string scrolls row by row in both directions without reading the rest of it.
- Tree ↔ raw sync: selecting a tree row scrolls the raw view to it and highlights it briefly; clicking in the raw view selects the innermost value there in the tree, opening groups and parents as needed (any depth). Tree context menu "Show in raw view".
- Text selection in the raw view (drag, Shift+click) and Ctrl/Cmd+C copy the exact source bytes (up to 50 MB; larger selections show a message).
- Go to (Ctrl/Cmd+L or the raw view's "Go to…" button): a line (`12345`), `line:col`, a byte offset (`@88120551`, `@200m`, `@1.5g`) or a simple path (`$.items[3].id`, `$["a b"][0]`); clear messages for lines, offsets or keys that don't exist.
- `braceray_core::pretty` (on-the-fly pretty printer: rows from any position, anchors from the index, budgeted and resumable seeking, rows backwards; original-mode rows; highlight lexer), `braceray_core::goto`, `Navigator::address_of`, `LineTable::{line_start, line_offset, position}`. Fuzz target `pretty_range`, criterion bench `pretty`.
- Search engine (`braceray_core::search`): plain-text search over the whole file on all cores, with an exact match count (2 GB file: ~20 ms case-sensitive, < 70 ms case-insensitive once it's in memory), results in document order as they come in (first result in ~10 ms), whole-word and case-sensitive options, Unicode case-insensitive matching (`ЁЛКА` finds `ёлка`, `ԱՐԵՎ` finds `արև`), and cancellation within a few ms. Up to 5,000,000 results are stored; Next/Previous keep working past that. Matches don't overlap (`aa` occurs twice in `aaaaa`), as in editors. Works before indexing finishes. Fuzz target `search_any`, criterion bench `search`.
- Search UI: a search bar under the toolbar (Ctrl/Cmd+F focuses it and selects its text). Searching starts as you type (after a 150 ms pause), shows "Searching… 1,204 matches (37 %)" → "18,442 matches" → "match 312 of 18,442", and works while the file is still indexing. `Aa` = match case, `ab|` = whole word (both re-run the search at once); regex and keys/values scope are shown disabled ("Coming in v1"). Enter / Shift+Enter, F3 / Shift+F3 (and ⌘G / ⌘⇧G on macOS) go to the next / previous match from the current selection, wrapping around with a "Wrapped to top" note; Esc clears the search, a second Esc closes the results. The last 20 searches are kept in a history dropdown.
- Results panel (Ctrl/Cmd+Shift+F, resizable): every stored match with its JSONPath, the text around it and its line, e.g. `#312  $.orders[48213].customer.email  …anna.p@example.com…  line 1,204,331`; paths and lines are computed in the background for the rows on screen. It scrolls smoothly through millions of results (2.45 M: frames under 6 ms). Click a result, or move with ↑/↓ and press Enter, to select the value in the tree and scroll the raw view to the match. "Copy all paths" copies the paths to the clipboard, or writes them to a text file you can open when there are more than 10,000.
- Highlights: every match on screen in the raw view gets a soft background and the current match a strong one (the view also scrolls sideways to it); tree rows holding a match get a marker. The status bar shows the count and the current match.
- `braceray_core::path::{jsonpath, decode_key}`, `SearchHandle::{matches_in, nav}` and `SearchNav` (next / previous from any thread).
- Breadcrumb bar under the toolbar: `$ › orders › [48213] › customer › email`. Click a segment to select it; hover a segment for a list of its siblings (first 200, "… N more") to move sideways, also in arrays with millions of elements; long paths collapse in the middle (`$ › … › customer › email`, click `…` for every level).
- Copy path in four formats: JSONPath (`$.a['weird key'][0]`), jq (`.a["weird key"][0]`), JavaScript (`data.a["weird key"][0]`, root name in Settings) and JSON Pointer (`/a/weird key/0`). From the breadcrumb button (with a format menu; the last format is remembered), the tree's context menu, or Ctrl/Cmd+Shift+C. Keys are decoded from their JSON escapes and quoted correctly for each format.
- Copy value as in the file, formatted (pretty) or minified, Copy key (decoded) and Copy string contents (decoded, without quotes), from the context menu; Ctrl/Cmd+C copies the selected value when the tree has focus. Big values copy in the background with progress and Cancel; "Copied 12.4 MB" notices.
- Values over 50 MB (Settings) ask first: "This value is 1.2 GB. Copying it may use a lot of memory. Export to file instead?" with Export… (save the raw bytes to a file), Copy anyway, Cancel.
- `braceray_core::path::format_path` / `PathFormat` and `braceray_core::copy` (streaming raw / pretty / minified / string-contents formatter, no DOM, cancellable, bounded output).

### Changed
- Searches run on their own thread pool, so a search started while a 2 GB file is indexing shows its first results as fast as on an indexed file.
- Reading children of huge containers is ~2.7× faster (galloping node lookup), and repeated reads near the last position no longer re-parse from the previous checkpoint.
- The "Indexed 2.15 GB in 3.2 s · N nodes" summary moved from the content area (now the tree) to the hover text of the status bar's index part.

### Fixed
- "Copy all paths" creates its temp file with `create_new` (O_EXCL), so an existing file or a symlink planted in a shared temp dir is never overwritten, and on Unix with mode 0600 so other users can't read it.
- The status bar's line:col no longer scans back to the start of the line every frame: on a 2 GB single-line file it shows the column in bytes ("Ln 1, Byte 1,158,735,112") past the first 1 MB of a line.
- Selecting a 200 MB string in the tree no longer re-reads it every frame (the selection's spans are cached), which made the app unresponsive.
- Recent files store the canonical absolute path, so a file opened as `./a.json` and as `/full/path/a.json` no longer shows up twice, and a relative command-line path keeps working when the app is started from another folder.
