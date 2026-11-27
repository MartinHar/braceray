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
