# Decisions

Short ADR-style entries for technical choices made inside a step. Newest last.

<!--
## NNN · YYYY-MM-DD · Step NN · Title
**Context:** …
**Decision:** …
**Consequences:** …
-->

## 001 · 2026-11-25 · Step 01 · App crate is a lib + thin binary
**Context:** `egui_kittest` integration tests in `crates/braceray-app/tests/` need to drive the real UI code.
**Decision:** `braceray-app` has a library target (`BracerayApp`, `show(&mut Ui)`) and `main.rs` only sets up eframe and the allocator.
**Consequences:** UI tests exercise exactly the code the window renders; the binary stays tiny.

## 002 · 2026-11-25 · Step 01 · No `rust-version` (MSRV) pin
**Context:** With `rust-version` set, Cargo's MSRV-aware resolver picked old releases (e.g. eframe 0.32 instead of 0.37), against the "latest stable dependency" rule.
**Decision:** Don't declare `rust-version`; `rust-toolchain.toml` pins the `stable` channel instead.
**Consequences:** Contributors need a recent stable toolchain, which rustup provides automatically.

## 003 · 2026-11-25 · Step 01 · CI does not run tests (for now)
**Context:** Martin asked not to run tests in CI during early development.
**Decision:** CI runs fmt, clippy, the release build on Windows/macOS/Linux, and `cargo deny`. `cargo test` runs locally only (it's still a mandatory pre-commit gate in CLAUDE.md).
**Consequences:** The `egui_kittest` harness isn't proven in CI yet. Re-enable the test step in step 12 (backlog item I-001).

## 004 · 2026-11-25 · Step 02 · Fixtures: per-record seeds + block-parallel writer
**Context:** Fixtures must be byte-identical for a seed and fast (2 GB in < 10 s).
**Decision:** Record `i` uses its own PRNG (`fastrand`) seeded with splitmix64(`seed`, `i`). rayon generates blocks of 1,024 records in parallel and they're written in order through an 8 MiB `BufWriter`. The size cut-off is checked per record, sequentially.
**Consequences:** Output doesn't depend on thread count (tested with 1 and 4 threads); ~1.5–1.9 GiB/s on an M3 Pro. Needle positions and diff changes are functions of the record index, so manifests are exact by construction. Changing any generator changes the bytes, which the golden-fixture test catches.

## 005 · 2026-11-25 · Step 02 · Invalid-fixture positions follow serde_json
**Context:** Step 02 requires invalid files to "fail at exactly the manifest's offset", checked with serde's line/column.
**Decision:** `error_offset` is the offending byte (file length for unexpected EOF); `line`/`column` are 1-based with a **byte** column. This matches serde everywhere except raw control characters in strings, where serde reports the byte before. The test maps that one case explicitly.
**Consequences:** Step 04's validator should report the offending byte too, and can be checked against these manifests directly.

## 006 · 2026-11-25 · Step 03 · Document offsets and early checks
**Context:** Step 03 wants a BOM skip, encoding and binary detection and an early first-byte check, all in O(1).
**Decision:** `Document::bytes()` is the content after a UTF-8 BOM. `len()` and every offset (including `Error::NotJson::offset`) are relative to it. `FileIdentity::size` is the raw file size. The NUL-pattern / binary checks use the first 4 KiB. Binary needs > 1 % control bytes **and at least 8 of them**, so a tiny file with one raw control character still opens and step 04's validator reports its exact position (`invalid_control_char.json` is 12 bytes). UTF-32 without a BOM is detected too (`Encoding::Utf32Le/Be`).
**Consequences:** Index and search work on one slice with no BOM arithmetic. The first-byte scan is linear only while it skips leading whitespace, so it's instant except for pathological whitespace-only files.

## 007 · 2026-11-25 · Step 03 · `memory-stats` for the RSS check
**Context:** The criterion "memory grows < 5 MB after open" needs the process's resident memory on macOS and Windows.
**Decision:** Use the `memory-stats` crate (MIT OR Apache-2.0) as a **dev-dependency** only, in the ignored `document_big` test.
**Consequences:** No platform-specific code of our own. The app can reuse it later for the status-bar memory readout (step 06).

## 008 · 2026-11-25 · Step 04 · 24-byte packed `Node`
**Context:** architecture.md §3.3 sketches a 28-byte node (32 with alignment). Pretty `orders` data has one container per ~110 bytes, so 32-byte nodes would be ~29 % of the file, over the 25 % budget.
**Decision:** `Node` is 24 bytes: `start` (bit 63 = kind), `end` (low 40 bits) + depth (high 24 bits), `parent: u32`, `child_count: u32`. Limits: files ≤ 1 TiB (`Error::FileTooLarge`), depth ≤ 16,777,215 (a `JsonError` "less nesting"), ≤ 2³² − 1 stored nodes. Child counts that don't fit a `u32` are stored as `UNKNOWN_COUNT` and computed on demand.
**Consequences:** `orders_2g.json` → 19.6 M nodes, 21.9 % of the file. Minified data (~72 B per container) still exceeds 25 % and will use adaptive mode (step 4c). Field access goes through accessor methods.

## 009 · 2026-11-25 · Step 04 · Validation semantics of the index
**Context:** The scalar reference indexer defines the output the fast builder must match exactly, including for invalid input. Step 04 also asks for character columns, while the invalid-fixture manifest (ADR 005) stores byte columns.
**Decision:**
- Grammar is RFC 8259, matching `serde_json`'s `IgnoredAny`: no UTF-8 validation inside strings and no `\u` surrogate pairing (both would need a full-file pass). Leading/trailing JSON whitespace only.
- The error offset is the first offending byte (the length for an unexpected end); `expected` is a short phrase such as `',' or '}'`.
- `JsonError.column` counts **characters** (what an editor shows). Tests check the manifest's byte column via `offset − line start + 1`.
- Partial index: containers opened before the error are kept. Those still open end at the error offset and have `UNKNOWN_COUNT` children.
**Consequences:** Validity agrees with serde on all property tests. A file with invalid UTF-8 inside a string is "valid" for Braceray (as for serde's `IgnoredAny`); display code must decode lossily.

## 010 · 2026-11-25 · Step 04 · SIMD classifier details
**Context:** Step 4b asks for scalar / SSE4.2 / AVX2 / NEON classifiers with runtime dispatch, and equivalence tests for each. Development happens on Apple Silicon.
**Decision:**
- The "SSE4.2" path only needs SSE2 instructions (`cmpeq`, `movemask`, `min_epu8`), so it requires SSE2, which every x86_64 CPU has. The scalar fallback then only runs on other architectures.
- Dispatch: `simd::best()` picks once (cached in a `OnceLock`) a `Simd` value holding two plain function pointers, `classify` and `prefix_xor`. The x86 `#[target_feature]` functions sit behind private safe wrappers that `available()` hands out only after `is_x86_feature_detected!`.
- Escape mask: our own branchless form (split backslash runs into even/odd-start runs with one addition). Unlike simdjson's "odd ends" mask, it also marks escaped backslashes, which escape validation needs. It's checked against a byte-by-byte reference for every run length at every position around block boundaries.
- x86 is tested on this Mac by building for `x86_64-apple-darwin` and running under Rosetta with `ROSETTA_ADVERTISE_AVX=1`, so SSE2, AVX2 and pclmul all actually run.
**Consequences:** All four implementations are proven equal locally. Rosetta speeds say nothing about real x86 hardware; measure on Windows before the release (step 12).

## 011 · 2026-11-25 · Step 04 · Parallel builder: SIMD tokens + the reference state machine
**Context:** Step 4c must equal the scalar reference exactly (node table, line table and validation result, including the error's offset and `expected` text) for any input and any chunk size, while running on all cores.
**Decision:**
- Phase A (parallel) summarizes chunks assuming no string or escape carry in. Phase B (sequential) fixes each chunk's real start state: an escape carried in flips only the byte after the leading backslash run, and the carry out only if the whole chunk is backslashes.
- Phase C (parallel, in batches of 2 × threads chunks) emits one `u32` token per value / structural (kind in the top 4 bits, 28-bit offset, so chunks ≤ 256 MiB). Clean strings, exact literals/numbers and `"key":` pairs are pre-validated. Strings with a backslash or control byte, strings crossing a chunk end, and invalid scalar runs become *suspicious*.
- A sequential consumer (overlapped with the next batch via `rayon::join`) drives the **same** `Parser` as the reference: `fast_token` applies a token exactly as `step` would, or declines. Declined and suspicious tokens go to the byte-level `step`; after a hand-off, `catch_up` steps until the next token so no byte is skipped (`null0`). All errors therefore come from the reference code itself.
- Adaptive mode compacts the node table when it exceeds the budget: it keeps the depths that fit in half the budget, remaps parents and stack ids, and stores only those depths from then on. The automatic budget is max(25 % of the file, 16 MiB).
- The spec's alternative (per-chunk grammar validation plus a parallel stitch) was not needed for the targets; see backlog I-003.
**Consequences:** Exact equivalence is checked with 30k proptest cases × every classifier × 5 chunk sizes. 2 GB warm: 1.18 s (pretty), 1.94 s (minified). The sequential consumer (~0.9 s for 230 M tokens) is now the bottleneck.

## 012 · 2026-11-25 · Step 04 · Navigation, oracles and fuzzing
**Context:** Step 4d asks for child iteration with an LRU of child offset tables (budget 256 MB) and element checkpoints for containers with > 10,000 children. It also asks that `node_at_offset` + `path_of` match a serde walk, and for a `cargo-fuzz` crate.
**Decision:**
- One mechanism for every container: a **checkpoint table** (offset of every 1024th child plus the child count), built on first use and cached by container start offset in a byte-budgeted LRU (default 256 MB). Children are re-parsed from the nearest checkpoint, so a request costs at most ~1024 extra child parses. Full per-child offset tables aren't needed: 1M children take 6 ms to build and ~8 KB to keep.
- Navigation works on a `Container` value (start, end, kind, optional `NodeRef`), so containers that adaptive mode didn't store expand the same way. Their end is found with the SIMD classifier.
- `Location` = innermost stored node plus the steps below it; `path_of` adds each ancestor's key/index.
- Path oracle: the proptest generator records every value's path and exact span while serializing. A serde walk can't give byte spans, so this oracle is strictly stronger.
- `fuzz/` is a standalone crate, excluded from the workspace because it needs nightly. Its `Cargo.lock` is committed.
- `[lib] bench = false` in `braceray-core`, so `cargo bench -- --quick` (the CLAUDE.md gate) only runs criterion.
**Consequences:** Memory for navigation stays tiny. The UI (step 07) groups huge arrays using `children(range)` directly.

## 013 · 2026-11-27 · Step 05 · Index cache format and loading
**Context:** Step 05 asks for an on-disk index cache that is safe against stale and corrupted entries and loads a 2 GB file's index in < 1 s, zero-copy via `bytemuck`. The spec suggested checksumming only sampled pages for entries over 256 MB.
**Decision:**
- One `<xxh3-128 of FileIdentity>.brx` per file. A 256-byte header holds magic, format version, app version, the file's size / content length / mtime (ns) / head-tail hash, the effective `memory_budget` (it changes the index in adaptive mode), counts, the validation result and the adaptive depth. The raw `Node` table follows (8-byte aligned), then the line checkpoints, then a checksum.
- **Full checksum, not sampled:** xxh3-64 over the xxh3-64 of every 16 MiB block, computed on all cores. That's about 10 ms for the 471 MB entry of `orders_2g.json`, so sampling isn't needed, and every flipped byte is detected. After the checksum, cheap parallel checks run that navigation relies on (offsets ≤ length, parent before child, monotonic line table), as a second line of defence.
- `Index` keeps its node table in a private `NodeStore`: an owned `Vec` after a build, or a view of the mapped entry (`Arc<MappedFile>` + range, `bytemuck::try_cast_slice`, so no new `unsafe`). `Node` derives `Pod`.
- The error's `expected` text is stored as a string and mapped back to the parser's `&'static str` (`EXPECTED_TEXTS`). An unknown text makes the entry unusable, so the index is rebuilt.
- Writes go to `<key>.<pid>.<n>.tmp` and are then renamed over the entry, with no fsync (a torn entry fails the checksum). Any bad entry is deleted on load (logged with `tracing`). Big-endian targets don't use the cache.
- `index.json` (serde_json) holds the original path, size and last access per entry. It is advisory: eviction takes sizes from the directory listing, falls back to file mtime when an entry has no metadata, and deletes `.tmp` files older than 1 h. Concurrent instances can at worst lose a last-access update.
- An entry larger than the whole size limit isn't written. `load_or_build` is the one call the app needs, and returns `IndexSource` + elapsed time for the status bar.
**Consequences:** 2 GB re-open in 14 ms warm. A cached index uses page-cache memory (shared, evictable) instead of 471 MB of heap. On Windows, a mapped entry can't be deleted or replaced while another instance holds it; that case is handled (kept, counted as written) but not yet tested on Windows (I-006).

## 014 · 2026-11-27 · Step 05 · Cache dir without the `directories` crate
**Context:** The stack lists `directories`, but its dependency `option-ext` is MPL-2.0, which isn't on the CLAUDE.md license allow-list, so `cargo deny check licenses` fails.
**Decision:** `IndexCache::default_dir()` computes the same paths as `ProjectDirs::from("io.github", "martinhar", "braceray")` from the environment with `std` only: `$HOME/Library/Caches/io.github.martinhar.braceray` on macOS, `%LOCALAPPDATA%\martinhar\braceray\cache` on Windows, `$XDG_CACHE_HOME/braceray` or `~/.cache/braceray` elsewhere, plus `/index`. No license exception was added.
**Consequences:** No new dependency. Windows uses the `LOCALAPPDATA` variable instead of the Known Folder API (the same value in practice). Martin confirmed this on 2026-11-27: keep the std version and use it for the settings dir in step 06 too. If `LOCALAPPDATA` / `HOME` is missing, there is no cache dir and the app just rebuilds the index each time.

## 015 · 2026-11-27 · Step 06 · Job plumbing: `JobHandle<P, T>` on a dedicated thread
**Context:** architecture.md §5 sketches `JobEvent<T>` with `Partial(T)` and `Done(T)`. The open job's partial result (the mapped `Arc<Document>`) has another type than its final result (`LoadedIndex`).
**Decision:** `JobEvent<P, T>` / `JobHandle<P, T>` with separate partial and final types. `JobHandle::spawn` runs the work on a named `std::thread` (the work itself uses the rayon pool). Partials go through an mpsc channel; the final event is stored in a slot and announced with an end marker, so a worker that panics (debug builds) shows up as `Error::JobPanicked` instead of hanging. A result that arrives after `cancel()` counts as `Cancelled`. Dropping the handle cancels the job (closing a tab cancels its indexing). The repaint callback is `Repaint = Arc<dyn Fn() + Send + Sync>`.
**Consequences:** Core stays UI-free; the app polls `try_recv` each frame. New errors: `Error::Thread`, `Error::JobPanicked`.

## 016 · 2026-11-27 · Step 06 · App shell choices
**Context:** Step 06 leaves a few technical choices open.
**Decision:**
- **Settings** are one JSON file in the OS config dir, computed with std like the cache dir (ADR 014), not eframe's `persistence` feature. Each field is parsed and range-checked on its own, so one bad value resets only that value. Saves go through a background thread (temp file + rename) in call order; `flush()` on exit and in tests.
- **File dialog:** `rfd::AsyncFileDialog` is created on the UI thread (macOS needs that) and awaited with `pollster` on a worker; the UI keeps rendering.
- **Blocking calls off the UI thread:** opening/mapping, the change check (`stat`), the "is this recent file missing?" check, cache size and cache clear all run as jobs. The memory readout (`memory-stats`, one syscall every 2 s) and `canonicalize` for tab de-duplication stay on the UI thread.
- **Icons:** egui 0.37 bundles no emoji font, so the UI uses text labels ("Settings", "?", "×") rather than adding ~1 MB of emoji fonts.
- **Inspection:** `egui_inspection` (feature `plugin`) is attached only under `cfg(debug_assertions)` on `127.0.0.1:5719` (`EGUI_INSPECTION=0` turns it off). The crate is still compiled in release builds, where its code is dead and removed by LTO.
- **Test hook:** `AppConfig::hold_indexing` (hidden) pauses the open job after mapping, so kittest can see the indexing state and press Cancel deterministically.
**Consequences:** Every interactive widget has an accessible name (kittest's accessibility check enforces it).

## 017 · 2026-11-27 · Step 06 · macOS "Open With" / double-click: follow-up
**Context:** Finder sends the files to open as an Apple Event (`application:openURLs:`), not as command-line arguments. winit 0.30 (used by eframe 0.37) doesn't forward it; the documented way is a custom `NSApplicationDelegate` declared with `objc2`, which needs `unsafe` in the app crate (not allowed by CLAUDE.md) and a bundled `.app` to test.
**Decision:** Not implemented in step 06. The command line, the dialog, drag & drop and recent files all work. Revisit in step 12 (packaging), where the `.app` bundle and `Info.plist` document types exist (backlog I-007).
**Consequences:** Until then, double-clicking a `.json` file associated with Braceray starts the app without opening the file.

## 018 · 2026-11-27 · Step 07 · Tree row model: segments, nested groups, addresses
**Context:** The tree must show millions of rows without materialising them, expand a 1 M-child node without per-child work, survive 100,000 nesting levels, and keep its state when the preview index is replaced by the full one.
**Decision:** `braceray_core::tree::TreeModel` stores the visible rows as a `Vec` of segments ("items `a..b` of this item list at depth `d`", the `RowSpan` variant the step allows). An item list is the children of a container, or groups of `unit` children where `unit` is the smallest power of the group size that leaves ≤ group size items (so groups nest above group size²; 2.45 M children → 3 groups of 1,000,000 → 1,000 groups of 1,000). Expanding splits a segment; collapsing removes the deeper segments and merges the halves back, and forgets the expansion state below. Rows are found by binary search over segment start rows; expand / collapse cost O(#segments). The expansion state is captured as a pre-order list of (parent entry, child index or group range) and re-applied with `restore`; selections and the top visible row are kept as child-index addresses and re-found with `reveal`. Groups that don't exist in the new layout are dropped and their children are re-attached to the container.
**Consequences:** Expand of a 10 M-child array: 68 ns; 50 rows for a viewport: 4.9 µs; 100k levels by "→": 26 ms in total; capture + restore of 100k levels: 30 ms. Collapsing a node and expanding it again starts from a collapsed subtree (like most tree views). Oracle proptests compare rows, addresses, parents, ranges, restore (same and other group size), reveal and preview → full against a walk of the generator's tree.

## 019 · 2026-11-27 · Step 07 · Non-blocking navigation: shared cache, `*_if_ready`, `prepare` on workers
**Context:** Reading child `k` of a big container needs its checkpoint table (every 1024th child offset). Building it for the 2.45 M-element root of `orders_2g.json` took 295 ms warm and ~1–3 s when the file's pages had to be faulted in again, which is far above the 4 ms UI-thread budget.
**Decision:** The checkpoint LRU moved into `nav::NavCache`, shared by `Arc` between the UI thread and workers; `Navigator::with_cache` borrows it. The UI thread only uses `children_if_ready` / `child_count_if_ready`, which never build a table for a container larger than 64 KiB (`SYNC_SCAN_BYTES`); children below index 1024 or near the last read position (a per-container cursor) are always ready. Anything else becomes a "loading…" row plus a `prepare(container, cancel)` job (at most 4 at once); an expand that needs a table runs again when its job finishes, unless the tree changed meanwhile. Walking children now finds container children with a galloping search from the previous node instead of a binary search over the whole node table (295 → ~110 ms warm for the 2 GB root). The tree also prefetches the root's table in the background right after opening.
**Consequences:** The UI thread never scans a big container. A deep group of a huge array can show "loading…" rows for up to ~1 s on first use when the file isn't in the page cache. In adaptive mode, listing the children of a record that contains a big *unstored* container still finds that container's end with a SIMD scan on the UI thread (backlog I-009).

## 020 · 2026-11-27 · Step 07 · Tree UI choices
**Context:** egui's `ScrollArea::show_rows` keeps the offset in `f32` points; 10 M rows × 22 pt = 220 M pt, where `f32` steps are 16 pt.
**Decision:**
- **Own virtual list:** the scroll position is an `f64` row offset with a hand-drawn scrollbar (wheel, drag, click), so any row count keeps pixel precision. Rows are painted directly (one `LayoutJob` each) and registered with `Sense::CLICK` without `FOCUSABLE`, so egui's arrow-key focus navigation doesn't compete with the tree keys; each row has an accessible `Row` label (`key : preview`) for kittest and egui-mcp.
- **Keys** are read when no text field wants the keyboard and no popup is open. `*` expands the selection and its direct children; "Expand all below" is recursive; both stop at 10,000 new rows and ask "Expand more?".
- **Deep rows** stop indenting at 40 % of the width and show "depth N".
- **Fonts:** eframe's default `system_fonts` feature already asks the OS for glyphs the bundled fonts lack (Armenian, CJK, emoji, colour emoji on macOS), so nothing is bundled.
- **Copy value** is limited to 16 MB of raw JSON; "Copy path" and "Show in raw view" are disabled hooks for steps 11 and 08.
- **Debug overlay** (`Ctrl+Shift+D`, also in release builds): UI-code time and eframe's full CPU frame time (p50 / p99 / max of the last 600 frames), rows drawn, time to first rows, last expand / prepare, tree size, plus an "Expand everything (no limit)" button for frame-time tests (UI thread, test tool only).
**Consequences:** About 400 lines of list code we own instead of egui's. Frame times over a 10,010,011-row tree: wheel max 0.53 ms, scrollbar drag top → bottom max 1.15 ms (full CPU frame, M3 Pro).

## 021 · 2026-11-27 · Step 08 · On-the-fly pretty printing: a layout that doesn't depend on where it starts
**Context:** The raw view must show row N of a formatted 2 GB single-line file without formatting rows 1 … N−1, scroll back up from any row, and jump to any byte offset (scrollbar at 63 %). Global formatted line numbers would need a full pass.
**Decision:**
- **Rows are a pure function of the bytes.** A row start is a `PrettyPos` (byte offset + nesting depth, plus the token start when the row continues a long token). The generator is a small state machine over bytes (no stack: depth is a number), so starting at any valid row start gives exactly the rows a pass from the file start gives. Tests check this for every row and many offsets (`seek`/`prev_line` == a full pass), on generated documents in every layout, golden fixtures, 10 MB fixtures, and in the fuzz target.
- **Layout:** one member/element per row; `{`/`[` end a row (empty containers stay `{}`); closing brackets start their own row; `:` gets one space. Whitespace is assigned to rows so their source spans tile the file (scroll position = byte offset; the scrollbar maps `0..len`).
- **Long tokens** (a 200 MB string) are split every `wrap` (400) **bytes from the token start**, moved back onto a UTF-8 boundary and off an unpaired backslash. So the row before or after any row inside the token is computed in O(1), and the row holding any offset inside it too; only scanning for the token's *end* is proportional to its length (memchr, ≈ 8× cheaper than formatting, done once per jump). Rows are counted in bytes, not characters (the spec's "400 chars"): identical for ASCII, and it keeps every step O(1).
- **Anchors** (`anchor_before`): a row start ≤ an offset, from the index only: the first child or the latest cached checkpoint of the innermost stored container, or the end of the last stored container closed before the offset (+ its comma or the parent's close). The gap to the offset contains no stored container start; a jump formats forward from there (`seek`).
- **Budgets:** `seek` stops after a byte budget and returns a resumable `SeekState` (always making progress, even inside a token). The UI thread spends ≤ 1 MiB (≈ 1–2 ms); the rest runs on a worker, which first prepares the innermost container's checkpoint table if it isn't cached (e.g. 10 M numbers in one array).
- **Original mode** rows are file lines soft-wrapped every 400 bytes from the line start; `LineTable::line_start` uses the checkpoints to find the previous newline in O(log n + 64 KiB), so a 2 GB line is never scanned backwards. Highlighting carries the lexer state from row to row; going up, the start state of a wrapped row is the candidate (in string / outside / escaped) that reproduces the known end state; a far jump into the middle of a line takes the state from the pretty row at that offset.
**Consequences:** Every operation costs O(rows on screen) or O(distance to the nearest anchor); the anchor distance is small for real data (a record) but can be large for pathological shapes (huge arrays of scalars before their checkpoint table exists, a key or value hundreds of MB long), which is what the worker path is for. Bench on 100 MB minified: a viewport 2.6 µs, jump to any offset 2.2 µs, a viewport back 1.4 ms.

## 022 · 2026-11-27 · Step 08 · Raw view UI: byte-anchored scrolling, shared navigation, sync and copy
**Context:** The raw view sits next to the tree, must stay at 60 fps on 2 GB files and follow the tree both ways.
**Decision:**
- **Position = top row + fraction.** No global row index; wheel/keys move by rows (`scroll_by`), the scrollbar seeks to a byte offset. Near the end the view steps back so the screen stays full.
- **Shares the tree's document, source (preview / full index) and `NavCache`**, so the root checkpoint table the tree prefetches also gives the raw view good anchors; a source switch (preview → full) re-seeks to the same offset.
- **Gutter:** line numbers in Original mode; byte offsets in Pretty mode (line:col of a formatted row would need a scan back to the line start, up to 2 GB): hovering shows line and column, in bytes past 4 MB into a line. The status bar does the same past 1 MB.
- **Sync:** tree selection → raw view scrolls the value (key included) to the third row and flashes it for 1.2 s. Raw click → the innermost value at the clicked **character** (not the nearest caret position) is located on a worker (`node_at_offset` + `address_of` may build checkpoints) and revealed in the tree (`TreeView::reveal_address`, waiting for prepare jobs); a selection change caused by the raw view doesn't scroll the raw view back.
- **Keyboard focus** belongs to the pane last clicked (tree by default); arrow keys go only to that pane.
- **Selection/copy:** drag or Shift+click selects source offsets (caret positions); Ctrl/Cmd+C copies the exact source bytes (lossy UTF-8 only for invalid bytes), up to 50 MB.
- **Go to** parses and resolves on a worker (`goto::{parse, resolve_line, resolve_path}`); a line target selects the value the line starts with.
**Consequences:** Measured in an optimized build with the inspection plugin (M3 Pro): `orders_min_2g` thumb drag end to end, 600 steps, CPU frame p99 7.2 ms / max 15.4 ms (a first, cold pass had one 36 ms frame: new glyphs and page faults); keyboard paging p99 8.8 ms; `huge_string.json` (200 MB) paging p99 10.7 ms. In this session egui-mcp's wheel events sent at a raw position reported no target and moved neither pane, so wheel scrolling is covered by kittests (both panes) and the frame times use keys and the scrollbar, which go through the same `scroll_by` / `go_to` code.


## 023 · 2026-11-27 · Step 09 · Search engine: non-overlapping chain stitched across chunks, capped store, no index argument
**Context:** Step 09 asks for an exact, parallel, cancellable count with document-ordered streaming, non-overlapping leftmost-first matches, a stored cap (5 M) with Next/Previous beyond it, and Unicode case-insensitive search including `ԱՐԵՎ` → `արև`.
**Decision:**
- **Engines:** `memmem::Finder` when case-sensitive (or the query has no ASCII letters), Aho-Corasick `ascii_case_insensitive` + leftmost-first for other ASCII queries, `regex::bytes` (escaped, `case_insensitive`, Unicode simple case folding) for non-ASCII case-insensitive queries. Max match length: query length, or 4 bytes per query character for the regex.
- **Armenian `և`:** the ligature has no uppercase letter (Unicode maps it to `ԵՒ`; reformed spelling writes `ԵՎ`), so simple case folding can't match `ԱՐԵՎ` to `արև`. The regex builder turns `ե`/`Ե` + `վ`/`Վ`/`ւ`/`Ւ`, and `և` itself, into `(?:և|եվ|եւ)`.
- **Whole word:** a match whose first (last) byte is a word byte (ASCII alnum, `_`, any byte ≥ 0x80) needs a non-word byte before (after) it, like `\b`. A rejected candidate consumes nothing (the scan goes on at `start + 1`).
- **Chain stitching:** each 8 MiB chunk is scanned on its own from its start, through a window `max_len − 1` bytes past its end, keeping matches that *start* in it. Self-overlapping needles (`aa` in `aaaa`) or whole-word rejects can make the true chain enter a chunk later than its start (the previous match straddles the border). The publisher, which handles chunks in document order under a mutex, then rescans from the carry until it meets a match of the chunk's own chain; from there both chains are identical. For normal text that is zero or a few matches; in the worst case (a chain that never resyncs, e.g. `aa` in 2 GB of `a` with a carry at an odd offset) it is one serial rescan per chunk.
- **Store and memory:** offsets (`u64`) and lengths (`u32`) in two vectors, 12 bytes per match, at most `max_stored_results`. Workers keep the first 64 matches of a chunk for free and reserve more in blocks of 4096 from a budget of `max_stored_results` entries, so unpublished chunks never hold more than ~60 MB extra; if a chunk's matches weren't kept but are needed for the store, the publisher rescans that chunk. The count is always exact.
- **Beyond the cap:** the store keeps the chain position at the start of every published chunk, so `next_after` / `prev_before` walk only the chunk(s) around the offset (≤ 8 MiB each) instead of the file. They answer only within the published part of the file.
- **API differences from the step file:** `start_search(bytes: Arc<impl AsRef<[u8]>>, &SearchQuery, SearchOptions, Repaint) -> Result<SearchHandle, Error>`. There's no `Arc<Index>` argument: the search only needs bytes (it must run before indexing finishes), and step 13's keys/values scope can add the index when it needs it. It returns a `Result` because step 13's regex compile errors need one; regex and keys/values scope give `Error::SearchUnsupported` for now. `Document: AsRef<[u8]>`, so tests and tools can search plain buffers. Extra methods: `is_running`, `is_capped`, `duration`, `wait`; `SearchOptions::threads`.
- **Workers** are `rayon::spawn` tasks (one per pool thread) that take chunk numbers from an atomic counter, so chunks finish roughly in order and the published prefix grows at the full rate. Cancellation is checked between chunks and every 1024 matches.
**Consequences:** 2 GB warm on M3 Pro: case-sensitive 19–20 ms, case-insensitive 19–82 ms, `e` (110 M matches) 298 ms, first result 4–13 ms, cancel ≤ 8 ms. The first search on a freshly mapped warm file adds ~50–70 ms of page faults; a file partly out of the page cache is limited by the disk. Workers share the global rayon pool, so a search started during an index build may wait for pool threads (I-014, step 10). With tiny test chunks (64 B) the publisher mutex dominates (lock contention), which only matters for tests.

## 024 · 2026-11-27 · Step 10 · Search UI: one bar for all tabs, cursor from the selection, lazy row info, own search pool
**Context:** Step 10 asks for search-as-you-type with a results list of millions of rows, jumps that land on the right node and bytes, next/previous from the current selection beyond the stored cap, and search while indexing.
**Decision:**
- **One search bar** for the app (text + toggles); each tab runs the *settled* query on its own file (`TabSearch::ensure`), so switching tabs searches the other file with the same query. Typing settles after 150 ms; toggles, Enter, history picks and clearing settle at once. A new query drops the tab's old `SearchHandle`, which cancels it.
- **Disabled, not hidden:** `.*` and the scope selector (`All ▾`) are shown disabled with the tooltip "Coming in v1", so the bar keeps its final layout.
- **Next / previous** start from the current match while the tree selection still holds it (or the tree is still revealing it), otherwise from the start of the selected value (key included), otherwise from the file's start / end. While every match is stored this is a binary search on the UI thread; beyond the cap it runs on a worker with a cloneable `SearchNav`. A step that finds nothing in the part searched so far is retried when more results are published (Enter right after typing jumps to the first match once it exists); "wrapped" only happens once the search is done.
- **Jumps** select the value through the raw view's locate job (`node_at_offset` + `address_of` on a worker) and reveal the match in the raw view (vertical position plus a horizontal scroll so a match far into a long line is visible). A match past the preview while indexing waits for the full index ("Indexing… the match is shown when the index is ready").
- **Results list:** own virtual list with an `f64` row offset (shared scrollbar with the tree, `ui::vlist`). The preview text comes from the mapped bytes on the UI thread (±32/48 bytes, whitespace runs collapsed); path and line come from one worker job per batch of visible rows that aren't cached, needing the full index; an LRU cache of 4,096 rows by match offset. ↑/↓ move a list selection, Enter jumps (a click jumps at once).
- **Highlights:** the raw view asks `SearchHandle::matches_in` for the bytes on screen (stored matches by binary search; beyond the cap it scans the visible range, exactly from the chunk's chain start when that is ≤ 256 KiB before it). Tree markers use only stored matches (`has_stored_in`, never scans), so container rows show a marker when any stored match is inside them.
- **History:** the last 20 settled texts used to navigate (Enter / F3 / a result click), not every debounced prefix; persisted in settings.
- **Copy all paths:** a worker job; ≤ 10,000 paths go to the clipboard, more to `braceray-paths-<pid>-<ms>.txt` in the temp dir with "Open" (system default app) and "Copy file path".
- **Own search pool (I-014):** `search` spawns its workers on a dedicated rayon pool (one thread per core), so a search during an index build no longer waits for the global pool. Both pools share the cores through the OS scheduler.
- **Measuring "first result < 200 ms":** from the start of the search (after the 150 ms debounce the same step asks for) to the frame showing the result. From the keystroke it is debounce + one or two frames (≈ 170–220 ms).
**Consequences:** Measured on M3 Pro: first result visible 17–35 ms after the search starts on `orders_2g.json` (also during indexing); results list over 2.45 M rows CPU frame p99 4.5 ms. Shortcuts are tested in kittests with egui told it runs on Windows (Ctrl) or macOS (Cmd, Cmd+G); a real Windows run is still to do (I-015).

