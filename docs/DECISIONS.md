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
