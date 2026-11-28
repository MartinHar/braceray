# Braceray — Build progress

> **Claude Code maintains this file.** Read it at the start of every session and update it while you work (rules in `CLAUDE.md` → *Progress tracking*).
> Humans: this is the single place to see where the build stands.

## Legend
`⬜ Not started` · `🟦 In progress` · `✅ Done` · `⛔ Blocked` · `⚠️ Done with known issues`

---

## Current status
| Field | Value |
|---|---|
| **Current step** | 12 — MVP hardening & release v0.1.0 |
| **Status** | 🟦 In progress |
| **Current sub-task** | 12b robustness (backlog items left), then 12c branding |
| **Next action** | 12b backlog: I-012/I-016, then I-009, I-007; then 12c icon + About |
| **Last updated** | 2026-11-28 |
| **Last session by** | Claude Code |
| **Latest version** | — (no release yet) |
| **CI on `main`** | Run 46114991172 (commit 6530f2d): Windows ✅ macOS ✅ Linux ✅ fixtures + benches ✅ deny ✅ — tests run in CI again |

## Blockers / questions for Martin
- [x] 2026-11-25 · Step 01 · GitHub repo `MartinHar/braceray` doesn't exist yet · Resolved: Martin approved a public repo; asked that CI not run tests until near the end
- [x] 2026-11-27 · Step 05 · `directories` depends on `option-ext` (MPL-2.0, not on the allow-list) · Resolved: Martin chose to keep the std-only folder lookup (ADR 014); no `directories`, no MPL exception
- [x] 2026-11-27 · Step 06 · Installing `egui_mcp` was blocked by Claude Code's permission policy · Resolved: Martin installed it (binary `egui-mcp`); driven over stdio JSON-RPC. Register with `claude mcp add egui -- egui-mcp` to get the tools natively
- [x] 2026-11-27 · Step 10 · "First results < 200 ms" is met from the search start (17–35 ms), but from the keystroke it is 150 ms debounce + 1–2 frames ≈ 170–220 ms · Is measuring from the search start OK, or should the debounce go down (e.g. 100 ms) so it holds from the keystroke too? Not blocking · Resolved: Martin checked the app ("all looks good"); the 150 ms debounce stays
<!-- Format: - [ ] YYYY-MM-DD · Step NN · question or blocker · what you need from me -->

---

## Phase overview
| # | Step | Phase | Status | Started | Finished | Commit / PR |
|---|---|---|---|---|---|---|
| 01 | Project setup & CI | MVP | ✅ | 2026-11-25 | 2026-11-25 | a3a1b0a (CI run 45650452101) |
| 02 | Test fixture generator | MVP | ✅ | 2026-11-25 | 2026-11-25 | (see git log) |
| 03 | File loading (mmap) | MVP | ✅ | 2026-11-25 | 2026-11-25 | (see git log) |
| 04 | Structural index & validation | MVP | ✅ | 2026-11-25 | 2026-11-25 | eac9cbd, 1c02d57, 16b7adc, 4d commit |
| 05 | Index cache | MVP | ✅ | 2026-11-27 | 2026-11-27 | bbc18fb |
| 06 | App shell | MVP | ✅ | 2026-11-27 | 2026-11-27 | eccc3e5, 509580d |
| 07 | Tree view | MVP | ✅ | 2026-11-27 | 2026-11-27 | fe313f8, e202057 |
| 08 | Raw text view | MVP | ✅ | 2026-11-27 | 2026-11-27 | ebda464 (8a), d4c6a23 (8b) |
| 09 | Search engine (core) | MVP | ✅ | 2026-11-27 | 2026-11-27 | 2419a48 |
| 10 | Search UI | MVP | ⚠️ | 2026-11-27 | 2026-11-27 | de4c6e2 (10a), e8bbdd6 (10b) |
| 11 | Breadcrumbs & copy | MVP | ✅ | 2026-11-28 | 2026-11-28 | fd02f47 (11a), 2bb48fc (fix), d6bf770 (11b) |
| 12 | MVP hardening & release v0.1.0 | MVP | 🟦 | 2026-11-28 | | |
| 13 | Advanced search (regex, keys/values) | v1 | ⬜ | | | |
| 14 | JSONPath queries | v1 | ⬜ | | | |
| 15 | Bookmarks | v1 | ⬜ | | | |
| 16 | NDJSON / JSON Lines | v1 | ⬜ | | | |
| 17 | Export JSON / CSV · release v1.0.0 | v1 | ⬜ | | | |
| 18 | Diff two files · release v2.0.0 | v2 | ⬜ | | | |

**MVP:** 11 / 12 · **v1:** 0 / 5 · **v2:** 0 / 1

### MVP time estimate (made 2026-11-25, after step 01)
About **15–20 working sessions** for steps 02–12, or **2–4 calendar weeks**. At 1–2 sessions a day it's about 2–3 weeks; at a few sessions a week, about 5–6. Step 01 took one session. Update this when real numbers differ.

| Step | Est. sessions | Notes |
|---|---|---|
| 02 Fixture generator | 1 | Simple, but everything after it depends on it |
| 03 File loading (mmap) | 1 | Small and well-defined |
| 04 Structural index & validation | 2–3 | Hardest step: SIMD scanning, < 5 s / 2 GB, ≤ 25 % memory |
| 05 Index cache | 1 | |
| 06 App shell | 1 | |
| 07 Tree view | 2 | Virtualization, 1M-child expand < 200 ms, p99 frame time |
| 08 Raw text view | 1–2 | |
| 09 Search engine (core) | 2 | Perf targets (< 1.5 s plain, < 3 s case-insensitive) |
| 10 Search UI | 1 | |
| 11 Breadcrumbs & copy | 1 | |
| 12 Hardening & v0.1.0 | 2–3 | Tests back on in CI, fuzzing, release packaging |

**Risks that could stretch it**
- Perf targets in steps 04 / 07 / 09: a first version that misses them could add 2–5 sessions of profiling and rework.
- Windows: development is on macOS, so Windows problems only show up in CI, where they take longer to debug. Do at least one real Windows check once the UI exists.
- Release packaging (step 12): the budget is $0 (`overview.md`), so builds ship unsigned / ad-hoc signed and users go through the Gatekeeper / SmartScreen "run anyway" flow. Getting the `.dmg` (universal binary, ad-hoc signing) and the Windows `.zip` right takes some trial runs of the release workflow.
- Review time: each step ends with a stop for Martin's review; same-day reviews keep the calendar estimate.

---

## Performance targets tracker
Fill in with real measurements (details and machine info in `docs/PERF.md`). Mark ✅ met / ❌ missed / — not measured yet.

| Target (from overview.md) | Goal | Measured | Machine | Step | Met? |
|---|---|---|---|---|---|
| First visible content (2 GB) | < 1 s | 134–163 ms open → first tree rows (preview), in-app | M3P | 07 | ✅ |
| Full index, warm (2 GB) | < 5 s | 1.18 s (pretty) / 1.94 s (minified) | M3P | 04 | ✅ |
| Full index, cold NVMe (2 GB) | < 10 s | 2.78 s (pretty) / 3.48 s (minified) after `purge` (page cache dropped; not a reboot); first content cold 16 / 20 ms | M3P | 12 | ✅ |
| Re-open from cache (2 GB) | < 1 s | 14 ms (pretty, entry 471 MB) / 3.6 ms (minified), warm | M3P | 05 | ✅ |
| Plain search count, warm (2 GB) | < 1.5 s | 19–20 ms (72–93 ms first search on a fresh mapping) | M3P | 09 | ✅ |
| Case-insensitive search (2 GB) | < 3 s | 19–82 ms (ASCII), 30 ms Unicode; `e` with 110 M matches 298 ms | M3P | 09 | ✅ |
| Simple regex (2 GB) | < 5 s | | | 13 | — |
| First search result | < 200 ms | UI: 17–35 ms from the search start to the result row on screen (also while indexing); 187–222 ms from the keystroke incl. the 150 ms debounce | M3P | 10 | ✅ |
| `$..price` JSONPath (2 GB) | < 5 s | | | 14 | — |
| Expand 1M-child node | < 200 ms | < 0.01 ms in the app (groups); 10M array 56 ns bench; deep group children ~110 ms warm in the background | M3P | 07 | ✅ |
| Scrolling p99 frame time | < 16 ms | tree: p99 0.94 ms, max 1.15 ms (10 M rows, drag); raw view: thumb drag on `orders_min_2g` CPU frame p99 7.2 / max 15.4 ms, paging p99 8.8 ms, 200 MB string p99 10.7 ms; results list (2.45 M rows) p99 4.5 / max 6.0 ms | M3P | 07/08/10 | ✅ |
| App memory excl. mapped file | < 300 MB + index | 476 MB with a 471 MB index (≈ 5 MB besides it), peak 979 MB during the build; 5 MB re-opened from cache (`alloc-stats` build) | M3P | 12 | ✅ |
| Index memory (% of file) | ≤ 25 % | 21.9 % (pretty) / 4.2 % (minified, adaptive) | M3P | 04 | ✅ |
| Diff 2 × 1 GB, warm | < 30 s | | | 18 | — |

---

## Step details
Template for each step. When a step **starts**, copy its acceptance criteria from `docs/specs/steps/NN-*.md` into its section and tick them as they are met.

<!--
### Step NN — Title
**Status:** 🟦 In progress · **Started:** YYYY-MM-DD · **Finished:** —

**Plan**
- [ ] sub-task 1
- [ ] sub-task 2

**Acceptance criteria** (copied from the step file)
- [ ] …

**Quality gates (last run)**
- [ ] fmt · [ ] clippy · [ ] tests (N passed) · [ ] cargo deny · [ ] benches (if relevant)

**What was done**
- Specs moved into the repo layout from `docs/specs/README.md`; GitHub constants in `overview.md` filled with `MartinHar` (from the `gh` login).
- Workspace: `braceray-core` (`version()`), `braceray-app` (lib `BracerayApp` + bin `braceray`, eframe 0.37, mimalloc), `tools/fixtures` (`gen-fixtures` prints "todo").
- Release binary: 7.4 MB (macOS arm64, stripped, fat LTO). Window verified: title "Braceray", 1400×900, "Drop a JSON file here" centred.
- `deny.toml`: licenses / advisories / bans / sources all ok; egui fonts allowed via `OFL-1.1` + `Ubuntu-font-1.0` (commented). `THIRD_PARTY_LICENSES.md` generated.
- CI: 3-OS matrix (fmt on Linux, clippy, release build — no tests, per Martin) + `cargo deny` job.

**Known limitations / follow-ups**
- CI has never run: no GitHub remote yet (waiting on Martin, see Blockers).
- `braceray-core` is not yet a dependency of the app (added back when the app first uses it, step 03/06).
- Window icon not wired into the app yet (final icon + embedding in step 12).
- ~20 duplicate transitive crate versions from eframe/winit (warn only).

**Decisions** (also in docs/DECISIONS.md)
- 001: app crate = lib + thin binary, so kittest drives the real UI code.
- 002: no `rust-version` pin (it made Cargo resolve old eframe 0.32).
- 003: CI does not run tests for now (Martin's request); re-enable in step 12.
-->

### Step 01 — Project setup & CI
**Status:** ✅ Done · **Started:** 2026-11-25 · **Finished:** 2026-11-25

**Plan**
- [x] Move specs into repo layout (`CLAUDE.md`, `docs/PROGRESS.md`, `docs/specs/`), `git init`
- [x] Root `Cargo.toml` (workspace, resolver 3, workspace.package, workspace.dependencies, release + bench profiles)
- [x] `braceray-core` skeleton: `version()` + unit test
- [x] `braceray-app` skeleton: eframe window "Braceray", mimalloc, windows_subsystem, 1400×900 / min 900×600, app id; `egui_kittest` test
- [x] `tools/fixtures` skeleton (`gen-fixtures` prints "todo")
- [x] Repo files: LICENSE, README, CHANGELOG, PERF.md, DECISIONS.md, .gitignore, rustfmt.toml, rust-toolchain.toml
- [x] `deny.toml` (license allow-list, unknown registries denied, duplicate warn, advisories fail)
- [x] `.github/workflows/ci.yml` (3-OS matrix + deny job)
- [x] Placeholder icon (SVG + 1024² PNG) in `crates/braceray-app/assets/`
- [x] Run all quality gates locally; check release binary size
- [x] Create GitHub repo, push, CI green (needs Martin's OK)

**Acceptance criteria** (copied from the step file)
- [x] `cargo run -p braceray-app` opens a window titled "Braceray".
- [x] `cargo build --release` succeeds; the release binary is < 15 MB.
- [x] All four quality gates pass locally.
- [x] CI is green on all three OSes. (run 45650452101 on a3a1b0a; the earlier macOS failure was GitHub not assigning a runner)
- [x] `cargo deny check` passes (fonts bundled by egui are allowed explicitly with a comment).
- [x] `docs/PROGRESS.md` exists (from the template), step 01 is ✅ with its section, quality gates line and a session log entry filled in, and *Current status* points to step 02. (*Current status* had already moved on to step 02/03.)

**Quality gates (last run)**
- [x] fmt · [x] clippy · [x] tests (3 passed: 1 core unit, 1 core doctest, 1 egui_kittest) · [x] cargo deny · benches n/a — run 2026-11-25 on macOS (Apple Silicon), Rust 1.100.0

**What was done**
- Specs moved into the repo layout from `docs/specs/README.md`; GitHub constants in `overview.md` filled with `MartinHar` (from the `gh` login).
- Workspace: `braceray-core` (`version()`), `braceray-app` (lib `BracerayApp` + bin `braceray`, eframe 0.37, mimalloc), `tools/fixtures` (`gen-fixtures` prints "todo").
- Release binary: 7.4 MB (macOS arm64, stripped, fat LTO). Window verified: title "Braceray", 1400×900, "Drop a JSON file here" centred.
- `deny.toml`: licenses / advisories / bans / sources all ok; egui fonts allowed via `OFL-1.1` + `Ubuntu-font-1.0` (commented). `THIRD_PARTY_LICENSES.md` generated.
- CI: 3-OS matrix (fmt on Linux, clippy, release build — no tests, per Martin) + `cargo deny` job.

**Known limitations / follow-ups**
- ~~CI has never run~~ Resolved: repo `MartinHar/braceray` is public, CI green on all 3 OSes.
- `braceray-core` is not yet a dependency of the app (added when the app first uses it, step 06).
- Window icon not wired into the app yet (final icon + embedding in step 12).
- ~20 duplicate transitive crate versions from eframe/winit (warn only).

**Decisions** (also in docs/DECISIONS.md)
- 001: app crate = lib + thin binary, so kittest drives the real UI code.
- 002: no `rust-version` pin (it made Cargo resolve old eframe 0.32).
- 003: CI does not run tests for now (Martin's request); re-enable in step 12.


### Step 02 — Test fixture generator
**Status:** ✅ Done · **Started:** 2026-11-25 · **Finished:** 2026-11-25

**Plan**
- [x] Crate as lib + bin; deps via `cargo add` (clap, fastrand, itoa, rayon, serde, serde_json)
- [x] Streaming JSON emitter (pretty 2-space / minified) into `Vec<u8>`; block-parallel record writer, per-record seeds, 8 MiB `BufWriter`, progress + elapsed
- [x] `orders` (realistic records, Unicode, all escapes, needles `NEEDLE_RARE` ×17 and `needle_common` in 1 % of records → `<file>.manifest.json`)
- [x] `ndjson`, `deep`, `wide`, `prim`, `string`
- [x] `invalid` + `invalid_manifest.json` (incl. error at the end of a big file)
- [x] `encoding` (BOM, UTF-16 LE/BE, UTF-32, empty, whitespace, binary) + manifest
- [x] `diff` (a/b + `diff_manifest.json`)
- [x] `all --max-size`
- [x] Golden fixtures in `crates/braceray-core/tests/fixtures/` (< 50 KB each)
- [x] Tests: determinism, validity (serde), needles, invalid offsets, diff manifest
- [x] `tools/fixtures/README.md`; timings in `docs/PERF.md`

**Acceptance criteria** (copied from the step file)
- [x] `gen-fixtures all --max-size 100m` finishes in < 10 s and creates every fixture type. (0.81 s, 41 files / 852 MiB; test `all_creates_every_fixture_type`)
- [x] Two runs with the same seed produce identical SHA-256 hashes. (`shasum -a 256` over all 41 files of two `all --max-size 100m` runs; test also compares 1- vs 4-thread output)
- [x] Every generated valid file parses with `serde_json` (verify for sizes ≤ 100 MB in a test). (10 MB of each valid kind + deep 100k + ndjson per line)
- [x] Every invalid file fails at exactly the manifest's offset (verify with serde's line/column). (serde reports the byte before a raw control char; mapped explicitly, see ADR 005)
- [x] Needle counts in the manifest match a naive count (test on the 10 MB file). (pretty, minified and ndjson; also checked on the 2 GB file with `grep -o`: 17 / 24,503)
- [x] 2 GB orders file generates in < 10 s on an NVMe machine (report the time in `docs/PERF.md`). (1.34–1.64 s on M3 Pro)

**Quality gates (last run)**
- [x] fmt · [x] clippy · [x] tests (31 passed: gen-fixtures 14 unit + 9 `generated` + 4 `invalid` + 1 doctest; core 1 unit + 1 doctest; app 1 kittest) · [x] cargo deny · benches n/a. Run 2026-11-25 on M3 Pro, macOS.

**What was done**
- `gen-fixtures` lib + bin: `all`, `orders`, `ndjson`, `deep`, `wide`, `prim`, `string`, `invalid`, `encoding`, `diff`, plus `--seed` and `-q`.
- Per-record seeding and a block-parallel writer give output that doesn't depend on thread count, at ~1.5–1.9 GiB/s.
- Manifests: needles (`<file>.manifest.json`), `invalid_manifest.json`, `encoding_manifest.json`, `diff_manifest.json`.
- Golden fixtures: `valid/` (12 hand-written), `invalid/`, `encoding/`, `generated/`. A test fails if the generated ones go stale.
- Dev-profile opt-level 3 for `gen-fixtures`, `serde_json` and `fastrand`, so the fixture tests run in about 0.6 s.

**Known limitations / follow-ups**
- `wide_1m.json` is ~24 MB, not the catalogue's "~50 MB" estimate. Shape is right (1M keys); size isn't significant.
- `deep` is always minified (pretty indentation would grow quadratically with depth).
- CI doesn't generate the 100 MB set yet (CI runs no tests for now, I-001). See I-002.

**Decisions** (also in docs/DECISIONS.md)
- 004: per-record seeds + block-parallel writer.
- 005: invalid-fixture positions follow serde_json (byte column; control-char quirk).

### Step 03 — File loading (mmap)
**Status:** ✅ Done · **Started:** 2026-11-25 · **Finished:** 2026-11-25

**Plan**
- [x] Deps via `cargo add`: `memmap2`, `xxhash-rust` (xxh3), `thiserror`; dev: `tempfile`, `memory-stats`, `serde_json`
- [x] `error.rs`: `Error` (`Io`, `NotAFile`, `EmptyFile`, `WhitespaceOnly`, `Encoding(Encoding)`, `NotJson`, `Mmap`) + `Encoding` (`Utf16Le/Be`, `Utf32Le/Be`, `Binary`) with user-facing messages and convert commands
- [x] `mmap.rs`: the only `unsafe` (map + `// SAFETY:`), `advise_sequential` / `advise_random` (no-op off Unix)
- [x] `document/detect.rs`: BOM / NUL-pattern / binary detection on a 4 KiB sample, first-byte check, `Sniff`
- [x] `document/mod.rs`: `Document::open`, `bytes`, `len`, `path`, `identity`, `has_changed_on_disk`, `sniff`; `FileIdentity` (canonical path, size, mtime, xxh3 head+tail 1 MiB)
- [x] Tests: unit tests for detection; integration tests with `tempfile` + golden encoding fixtures; 5 GB sparse file; change detection; ignored 2 GB timing + memory test; doctest
- [x] Docs: PERF.md (open time), DECISIONS.md, CHANGELOG, PROGRESS, THIRD_PARTY_LICENSES

**Acceptance criteria** (copied from the step file)
- [x] Opening the 2 GB fixture takes < 50 ms (measure in a test with `Instant`, `#[ignore]`d unless fixtures exist). (`tests/document_big.rs`: first open 0.23–5.6 ms, warm 0.14–0.35 ms)
- [x] Process memory after open increases by < 5 MB (mapped pages not counted until touched). (+2.03 MiB RSS, measured with `memory-stats`)
- [x] UTF-8 BOM is skipped; UTF-16 and binary files return the right error variant and message. (all 9 entries of `encoding_manifest.json`, plus UTF-16/32 LE/BE without BOM in unit tests)
- [x] Empty and whitespace-only files give clear errors. ("This file is empty." / "This file contains only whitespace, no JSON value."; also BOM-only files)
- [x] A 5 GB sparse file opens and `len()` is correct. (5 GiB + 3 B, opens in 4.9 ms)
- [x] `has_changed_on_disk()` turns true after appending a byte or touching mtime. (also after deletion, where the OS allows it)

**Quality gates (last run)**
- [x] fmt · [x] clippy · [x] tests (55 passed, 1 ignored (`document_big`, passes with `--ignored`): core 13 unit + 11 integration + 2 doctests; gen-fixtures 14 + 9 + 4 + 1; app 1) · [x] cargo deny · benches n/a (no index/search code). Run 2026-11-25 on M3 Pro, macOS.

**What was done**
- `braceray_core::{Document, FileIdentity, Sniff, Error, Encoding}`. `open` maps the file read-only and touches only the first 4 KiB and 1 MiB at each end.
- Encoding checks: UTF-8 BOM skipped; UTF-16/32 BOMs; UTF-16/32 without BOM from the NUL position pattern; binary (> 1 % control bytes, at least 8). UTF-16/32 messages include `iconv` and PowerShell commands.
- Early checks: empty, BOM-only, whitespace-only, first byte not a JSON value start (`NotJson` with offset and byte), directories (`NotAFile`), missing files (`Io`), mmap failure (`Mmap`, "network drive?"). It never falls back to reading the file into RAM.
- `sniff()`: NDJSON if the first line is a one-line container and another container line follows; otherwise JSON.

**Known limitations / follow-ups**
- Cold-cache open time not measured (needs a reboot or `sudo purge`).
- `Advice::Sequential` / `Random` are no-ops on Windows (memmap2 only supports `madvise` on Unix); step 04 calls them around the index build.
- SIGBUS on truncation by another process isn't caught. Mitigated by change detection (architecture.md §9); the app polls it from step 06.

**Decisions** (also in docs/DECISIONS.md)
- 006: offsets are relative to the content after the BOM; binary needs ≥ 8 control bytes.
- 007: `memory-stats` as a dev-dependency for the RSS check.

### Step 04 — Structural index & validation
**Status:** ✅ Done · **Started:** 2026-11-25 · **Finished:** 2026-11-25

**Plan**
- [x] 4a: `job.rs` (`CancelToken`, `Progress`); `index/{mod,node,lines,scalar}.rs`: 24-byte `Node`, `Index`, `Validation`, `JsonError` (char column), line table, scalar reference indexer (explicit stack, full RFC 8259 validation, optional depth limit); oracle tests vs serde (proptest), invalid manifest, deep_100k
- [x] 4b: `index/simd/` classifier (scalar / SSE4.2 / AVX2 / NEON, pclmul or shift-XOR prefix XOR), runtime dispatch, equivalence tests
- [x] 4c: parallel build (chunks, prefix fix, per-chunk events + validation, stitch, adaptive mode, progress, cancel), `build_index` API; equivalence + chunk-size matrix
- [x] 4d: `nav/` (Navigator, checkpoints + byte-budgeted LRU), criterion benches, 2 GB harness (`tests/document_big.rs`), fuzz crate `index_any_bytes`
- [x] Docs: PERF, DECISIONS, CHANGELOG, PROGRESS, THIRD_PARTY_LICENSES

**Acceptance criteria** (copied from the step file)
- [x] For every golden fixture and 10,000 proptest cases: the SIMD/parallel index == the scalar reference index (node table, line table, validation result). (`tests/index_build.rs`: 3 properties × 10,000 cases (generated, mutated, random bytes) × every classifier; 5 runs native + 1 under Rosetta with SSE2/AVX2)
- [x] Run with chunk sizes 64, 65, 127, 4096 and 8 MiB: identical output. (every equivalence check runs all five)
- [x] Every `invalid_*` fixture reports the manifest's offset, line and column exactly. (both builders; byte column checked via the offset, char column separately, ADR 009)
- [x] `deep_100k.json` indexes without stack overflow. (both builders, plus 1 M unclosed `[`)
- [x] 2 GB orders, warm: **< 5 s** total on reference machines (single-thread SIMD ≥ 1.5 GB/s; parallel limited by memory bandwidth). (1.18 s pretty / 1.94 s minified on M3 Pro; classifier 5.25 GB/s single-thread, tokenizer ≈ 1.5 GB/s; the parallel part is limited by the sequential consumer rather than memory bandwidth, I-003. Windows machine B not measured yet)
- [x] Index memory ≤ 25 % of file size on `orders_2g.json`. Adaptive mode is triggered and correct on a file of tiny objects (e.g. `[{"a":1},{"a":1},…]`). (21.9 %; `adaptive_mode_on_tiny_objects` equals the reference built with the same depth limit; minified 2 GB uses depth ≤ 1, 4.2 %)
- [x] Cancelling mid-build returns `Error::Cancelled` within 50 ms. (`cancel_returns_quickly`, 256 MB, cancel after 0/5/30 ms, passes in debug and release)
- [x] `node_at_offset` + `path_of` give the same path as a serde walk for 10,000 random offsets. (`tests/nav.rs`: 1000 docs × 10 offsets, normal and adaptive mode; oracle = the generator's recorded paths and spans, stricter than a serde walk, ADR 012)

**Quality gates (last run)**
- [x] fmt · [x] clippy (workspace native; core all targets for x86_64-apple-darwin; core lib for x86_64-pc-windows-msvc) · [x] tests (101 passed, 4 ignored big-file tests that pass with `--ignored`: core 31 unit + 11 document + 10 index_scalar + 10 index_build + 3 nav + 7 doctests; gen-fixtures 14 + 9 + 4 + 1; app 1) · [x] cargo deny · [x] benches (`--quick`, first numbers, recorded in PERF.md; nothing earlier to compare) · fuzz 120 s clean. Run 2026-11-25.

**Progress notes**
- 4a done: scalar reference indexer. 2,000 generated docs per run match the generator's own node table exactly, and 2,000 mutated docs agree with serde on validity (8 release runs ≈ 32k cases). All 12 invalid fixtures hit the manifest offset/line/column. 2 GB orders: 0.38 GB/s, 21.9 % memory (baseline).
- 4b done: classifier with scalar / NEON / SSE2 / AVX2 (+pclmul) and runtime dispatch. All implementations equal on every byte value, 2,000 random blocks, 2,000 random multi-block inputs and an exhaustive backslash-run sweep. NEON scans 2 GB at 5.25 GB/s on one thread.
- 4c done: `build_index` / `IndexOptions`. Exactly equal to the reference (incl. errors) by construction: SIMD tokens + the shared `Parser` state machine, hand-off to the byte-level parser for anything doubtful (ADR 011). The proptest found one real bug on the way (`null0`: bytes after a hand-off had no token), fixed with `catch_up`. 2 GB warm 1.18 s.
- 4d done: `nav::Navigator` (root, children(range), child_count, parent, node_at_offset, path_of, line_col), checkpoint tables cached in an LRU; criterion benches; fuzz crate (546,754 runs clean).

**What was done**
- `index::{scalar::build, build_index, build_bytes, IndexOptions, Index, Node, Validation, JsonError, LineTable, simd}`, `nav::Navigator`, `job::{CancelToken, Progress}`.
- Exact equivalence of the parallel builder with the reference is guaranteed by sharing the state machine (ADR 011) and checked by proptest and fuzzing.

**Known limitations / follow-ups**
- The sequential token consumer limits the parallel speed-up (2.2× on 12 cores); targets are met (I-003).
- x86 speed only measured under Rosetta (I-004). A cold-after-reboot index time isn't measured yet.
- Strings aren't UTF-8 validated and `\u` surrogates aren't paired, as with serde's `IgnoredAny` (ADR 009).
- Fuzzing runs manually; a nightly CI job comes with step 12.

- Decisions: ADR 008 (24-byte node), ADR 009 (validation semantics, char columns, partial index), ADR 010 (SSE2 instead of SSE4.2, dispatch, escape mask, Rosetta testing), ADR 011 (parallel builder design), ADR 012 (navigation, oracles, fuzz crate).

### Step 05 — Index cache
**Status:** ✅ Done · **Started:** 2026-11-27 · **Finished:** 2026-11-27

**Plan**
- [x] Deps via `cargo add`: `bytemuck` (derive), `tracing`, `serde` + `serde_json` (cache metadata). `directories` was added, then removed: MPL-2.0 dependency (ADR 014, question for Martin)
- [x] `index/store.rs`: `NodeStore` (owned `Vec<Node>` or zero-copy slice of a mapped cache file); `Node: Pod`
- [x] `cache/format.rs`: `.brx` header (magic, format/app version, identity, budget, counts, validation, adaptive depth), node + line tables, block-parallel xxh3 checksum; decode with `bytemuck::try_cast_slice`
- [x] `cache/meta.rs`: `index.json` (key → path, size, last_access), LRU eviction, stale `.tmp` cleanup
- [x] `cache/mod.rs` + `cache/open.rs`: `IndexCache`, `CacheSettings` (enabled / max bytes / min file bytes), `cache_key` (xxh3-128 of `FileIdentity`), `load`, `store` (tmp + atomic rename), `spawn_store`, `clear` / `clear_index_cache` → bytes freed, `default_dir()`; `load_or_build` + `IndexSource` + `LoadedIndex::status_text`
- [x] Tests (tempdir cache): hit equals build, stale after append / touch, corruption (truncate, flip bytes) → deleted + rebuilt, old format version, disabled → empty dir, eviction under a tiny limit, concurrent writers/readers, small files skipped, invalid-document round trip
- [x] 2 GB re-open test (ignored) + criterion bench `cache_100m`
- [x] Docs: PERF, DECISIONS, CHANGELOG, PROGRESS (THIRD_PARTY_LICENSES unchanged: no new crates, eframe already pulled them in)

**Acceptance criteria** (copied from the step file)
- [x] Re-open the 2 GB fixture: index ready in **< 1 s** (warm disk). (`document_big::cache_reopen_2g`: `Document::open` + `load_or_build`, first 14.5 ms, best 13.8 ms; minified 3.6 ms)
- [x] Modifying the source file (append a byte, touch mtime) → the cache is ignored and the index rebuilt. (`modified_source_is_rebuilt`)
- [x] A corrupted cache file (truncate or flip bytes) → detected, deleted, rebuilt; no crash. (`truncated_entry_is_rebuilt`: 9 truncations/extensions; `flipped_bytes_are_detected`: 15 positions over header, nodes, lines and checksum, plus all-garbage)
- [x] Cache from an older `format_version` → ignored and rebuilt. (`other_format_or_app_version_is_rebuilt`: versions 0 and 2 and another app version)
- [x] With the cache disabled, nothing is written to the cache dir. (`disabled_cache_writes_nothing`: the dir isn't even created; existing entries aren't read)
- [x] Eviction keeps the total under the limit (test with a tiny limit). (`eviction_keeps_total_under_limit`: limit = 2.5 entries, 6 files, LRU order checked; an entry larger than the limit isn't written)
- [x] Two app instances opening the same file at once don't corrupt the cache (temp + rename). (`concurrent_writers_and_readers_never_see_a_broken_entry`: 6 writers × 10 + 6 readers × 40 on one entry, each with its own `IndexCache`; threads in one process, with unique temp names per pid + counter, the same mechanism as for two processes. No leftover temp files)

**Quality gates (last run)**
- [x] fmt · [x] clippy (workspace; core lib also for x86_64-pc-windows-msvc) · [x] tests (120 passed, 5 ignored big-file tests that pass with `--ignored`: core 35 unit + 14 cache + 11 document + 10 index_scalar + 10 index_build + 3 nav + 8 doctests; gen-fixtures 14 + 9 + 4 + 1; app 1; cache tests also pass as x86_64 under Rosetta; cache proptest also run with 5,000 cases) · [x] cargo deny · [x] benches (`--quick`: index within noise except `scalar_index`, which is a code-layout artefact of the build path, see PERF.md / I-005). Run 2026-11-27 on M3 Pro.

**What was done**
- `braceray_core::cache::{IndexCache, CacheSettings, CacheError, StoreOutcome, SkipReason, IndexSource, LoadedIndex, load_or_build, clear_index_cache, cache_key}`; `Index::is_from_cache`.
- Loading maps the entry and uses its node table without copying. Full block-parallel checksum plus structural checks: 2 GB re-open in 14 ms.

**Known limitations / follow-ups**
- Windows-specific paths aren't tested yet (a mapped entry can't be replaced or deleted there; the code keeps it and moves on). CI runs no tests (I-001), see I-006.
- Cold re-open (cache entry not in the page cache) not measured; reading 471 MB from the SSD should still be well under 1 s.
- Not wired into the app yet: settings UI and status text come in step 06.

**Decisions** (also in docs/DECISIONS.md)
- 013: cache format, full checksum instead of sampled, zero-copy `NodeStore`, advisory `index.json`.
- 014: cache dir computed with std instead of `directories` (MPL-2.0 dependency); confirmed by Martin.

### Step 06 — App shell
**Status:** ✅ Done · **Started:** 2026-11-27 · **Finished:** 2026-11-27

**Plan**
- [x] Core `job.rs`: `JobHandle<P, T>`, `JobEvent<P, T>`, `JobContext`, `Repaint` callback, `JobHandle::spawn` (dedicated thread); unit tests (done, partial, failed, cancel, late result after cancel, panic, drop cancels, repaint count) + doctest
- [x] App `settings.rs`: `Settings` (theme, cache on/off, max size, min size, max stored results, tree group size, recent files ≤ 15), JSON in the std config dir (ADR 014 style), per-field validation, background saver thread with `flush()`
- [x] App `theme.rs`: Dark / Light / System, accent `#22D3EE` / `#0891B2`, 6 px rounding, spacing
- [x] App `tab.rs`: `OpenDocument` (open + `load_or_build` job; Opening / Indexing / Ready / Failed / Cancelled; speed, ETA, cancel; change check every 5 s + on focus, off the UI thread)
- [x] App `ui/`: toolbar + tabs (×, middle-click, Ctrl/Cmd+W), empty state (drop zone, Open file…, recent list, missing files greyed + Remove, Clear recent), indexing panel, error banner (Copy details, Details, Open anyway / Reload, Dismiss), status bar, settings window (Clear cache (size)), keyboard shortcuts dialog
- [x] Opening: rfd async dialog (created on the UI thread, awaited with `pollster` on a worker), drag & drop with hover highlight, CLI arg, recent files, tab de-dup by canonical path
- [x] Debug-only inspection plugin (`egui_inspection`, 127.0.0.1:5719), README *Development* section; macOS "Open With" → ADR 017 follow-up (I-007)
- [x] kittest tests (`tests/shell.rs`, 10): open → indexing → done + cancel + retry, invalid banner, UTF-16 / empty / binary / missing messages, tabs open/switch/dedup/close, theme toggle + persistence, settings round trip + bad values, cache "from cache" + disabled cache writes nothing, recent files (open, missing + Remove, Clear), drag & drop, `?` dialog
- [x] Manual run of the release app with the 2 GB fixture (progress, speed, ETA, "Indexed …", "from cache"); docs (CHANGELOG, DECISIONS 015–017, PERF, THIRD_PARTY_LICENSES, README)
- [x] egui_mcp drive-through (open via Recent, status bar, Cancel, resize latency probe)
- [x] Fix found while driving: recent files stored non-canonical paths (duplicate `orders_2g.json`, relative `fixtures-data/…` entry); now the canonical path + kittest `recent_files_store_one_canonical_path_per_file`; README `egui-mcp` command fixed
- [x] Martin: manual checks listed under the acceptance criteria below (2026-11-27: all OK)

**Acceptance criteria** (copied from the step file)
- [x] Opening the 2 GB fixture: the window stays responsive the whole time (drag the window and resize during indexing, no stutter); progress updates smoothly; Cancel works. — *Verified via egui-mcp: resize round trips during a 2 GB build (opt-level 3 build with the inspection plugin) median 2.7 ms, p95 13.8 ms over 896 resizes (idle baseline 16.6 ms median); progress label advanced smoothly 4 % → 100 %; Cancel clicked through the UI → "Indexing cancelled", memory back to 78 MB; closing a ready 2 GB tab ≈ 15 ms. One 1.0 s outlier in the first run, not reproduced in two more runs (likely window start-up). Unoptimized debug build: 94 ms median while the debug builder saturates all cores, expected. Physical window drag during indexing checked by Martin: OK.*
- [x] Drag & drop, dialog, CLI arg and recent files all open files. — *CLI verified in the real app; drag & drop (injected drop event) and recent files in kittest. Native dialog (Open file… / Cmd+O) checked by Martin: OK.*
- [x] Opening a second file opens a tab; switching tabs is instant. (`tabs_open_switch_dedup_and_close`; switching only changes an index, no work)
- [x] Invalid JSON shows the banner with line/column; UTF-16 and empty files show their friendly messages. (`invalid_json_shows_banner_with_position`, `friendly_messages_for_unreadable_files`)
- [x] Theme switch is instant and persisted; "System" follows the OS. — *Instant + persisted verified (`theme_switch_is_instant_and_persisted`). "System" uses egui's `ThemePreference::System` (OS theme from winit); Martin switched the macOS appearance with Theme = System: OK.*
- [x] Settings persist across restarts; the cache toggle really disables cache writes. (`settings_round_trip_through_the_window`, `reopen_shows_from_cache_and_disabled_cache_writes_nothing`)
- [x] Reopening a cached file shows "from cache" in the status bar. (kittest + release app: "Index: from cache (0.04 s)" for the 2 GB fixture)
- [x] Debug build can be driven via egui_mcp (Claude Code: open a fixture through the UI and read the status bar). — *Driven with `egui-mcp` over stdio: attach → click `orders_16k.json` in Recent files → `wait_for "Indexed"` → status bar read: "16.8 KB · Index: built (0.00 s) · Memory 113 MB". Also clicked Settings, Cancel, Cmd+W, resize.*

**Quality gates (last run)**
- [x] fmt · [x] clippy (workspace, all targets) · [x] tests (145 passed, 0 failed, 5 ignored big-file tests; app: 8 unit + 11 shell + 1 window; core +5 job unit tests + 1 doctest) · [x] cargo deny. Benches not needed (no index/search change). Windows cross-clippy of the app can't run on macOS (mimalloc's C build needs MSVC); CI covers it. Run 2026-11-27 (second session) on M3 Pro.

**What was done**
- Core: `braceray_core::job::{JobHandle, JobEvent, JobContext, Repaint, no_repaint}`, `Error::{Thread, JobPanicked}`.
- App modules: `app.rs` (state, actions, keyboard, drag & drop, dialog), `tab.rs`, `settings.rs`, `theme.rs`, `format.rs`, `ui/{toolbar, empty, body, banner, status, settings_window, shortcuts}.rs`. All files < 500 lines.
- New deps (app): `braceray-core`, `rfd`, `pollster`, `egui_inspection` (plugin), `memory-stats`, `serde`, `serde_json`, `tracing`; dev: `tempfile`. All MIT / Apache / Zlib.

**Known limitations / follow-ups**
- macOS "Open With" / double-click doesn't open the file yet (ADR 017, I-007).
- Status-bar memory is the resident set, which includes mapped file pages (I-008).
- The tree area shows a placeholder until step 07; line:col appears once the tree sets a selection.
- Existing stale recent entries (relative paths written before the fix) stay until removed; no migration (no release yet).
- Escape doesn't close the Settings window (not in the spec; could be added with step 11/12 polish).
- `index_progress` speed counts both passes of the build as file bytes (progress is 0 → 2 × length), which is what the user cares about.

**Decisions** (also in docs/DECISIONS.md)
- 015: `JobHandle<P, T>` on a dedicated thread, panic → `JobPanicked`, drop cancels.
- 016: settings JSON file (not eframe persistence), rfd async + pollster, what runs off the UI thread, no emoji fonts, debug-only inspection, test hook.
- 017: macOS "Open With" deferred to step 12.

### Step 07 — Tree view
**Status:** ✅ Done · **Started:** 2026-11-27 · **Finished:** 2026-11-27

**Plan**
- [x] 7a core `nav`: shareable `NavCache` (checkpoint cache usable from the UI thread and workers), non-blocking `children_if_ready` / `child_count_if_ready` (cheap sync path for small containers), cancellable `prepare`
- [x] 7a core `tree`: `TreeModel` with `Vec<Segment>` rows (the "RowSpan" variant of the spec), groups (nested above group_size²), expand / collapse / parent / address (child-index path) / reveal / rebuild; `rows(first, n)`; unit tests vs serde walks + proptest
- [x] 7a core `tree::preview`: index of the first few MB for preview mode (same model, same offsets)
- [x] 7a bench `rows_for_viewport` on a 10M-child array; checkpoint build time on `orders_2g.json`
- [x] 7b app: `ui/tree.rs` virtualized rows (`show_rows`), row painting (colours per type, counts, badges, lossy UTF-8), selection + keyboard (↑↓←→ Home End PgUp PgDn `*` Enter), context menu (copy key / value with size guard; path + raw view hooks), background checkpoint jobs with loading rows
- [x] 7b app: preview mode while indexing ("Preview: indexing… 37 %"), switch to the real index keeping expanded state + selection
- [x] 7b app: system font fallbacks for Armenian / CJK / emoji: already provided by eframe's default `system_fonts` feature (verified in the app, nothing bundled)
- [x] 7b app: debug overlay `Ctrl+Shift+D` with frame times (p50 / p99 / max)
- [x] 7c kittest: expand/collapse by click and keyboard, group rows > 1000 children, selection moves, context menu copy
- [x] 7c manual / egui-mcp: orders_2g, prim_array_10m, wide_1m, deep_100k, Unicode, frame times; docs (PERF, DECISIONS, CHANGELOG)

**Acceptance criteria** (copied from the step file)
- [x] `orders_2g.json`: expanding the root array (millions of elements, grouped) takes **< 200 ms**. (expand < 0.01 ms in the app overlay: 2,450,341 children → 3 groups of 1,000,000 shown at once; bench `tree_10m/expand_root` 56–68 ns. Opening a group deep inside needs the root's checkpoint table: ~110 ms warm on a worker, up to ~1.2 s with evicted pages, shown as "loading…" rows; now prefetched in the background)
- [x] `prim_array_10m.json` and `wide_1m.json`: expand in < 200 ms; scrolling to the end is smooth. (both < 0.01 ms; End / scrollbar drag reach `[9,999,999]` and `key_999999` with frames ≤ 1.2 ms; the last group of `wide_1m` waited for a 46 ms background prepare)
- [x] Scrolling with the mouse wheel and dragging the scrollbar from top to bottom of a fully expanded big array: frames stay < 16 ms (log frame times in debug overlay `Ctrl+Shift+D`). (`prim_array_10m` fully expanded = 10,010,011 rows; overlay after Reset, opt-level 3 build driven by egui-mcp: 300 wheel events CPU frame max 0.53 ms; scrollbar drag top → bottom in 300 steps max 1.15 ms, p99 0.94 ms)
- [x] `deep_100k.json`: expanding 100,000 levels by keyboard works without stack overflow. (kittest `deep_100k_by_keyboard`: 200,000 × "→" through the app's key handler on a 100,000-level file, then real ←/Home/← presses; core test the same in 26 ms; real app: all 100k levels of `deep_100k.json` expanded (199,999 rows), End/← at depth 99,999, frames ≤ 1.1 ms, indent capped with a "depth N" badge)
- [x] Preview mode shows content within 1 s of opening the 2 GB file. (overlay "First rows after open": 134 ms and 163 ms on `orders_2g.json` with a stale cache; root shows `[4,815+]` from the first 4 MB, then switches to the full index keeping state (kittest `preview_switches_to_the_full_index_keeping_state`). Process launch → window ready is ~1.6 s in the debug-info build and isn't part of this)
- [x] Unicode keys and values render correctly (Armenian, Cyrillic, CJK, emoji); invalid UTF-8 inside strings renders with � and never panics. (screenshots in light and dark theme: Armenian, Cyrillic, Chinese, Japanese, Korean keys and values, colour emoji; `ok\xFF\xFEend` → `ok��end`; unit test `previews`)
- [x] All keyboard shortcuts work; context menu actions work. (kittest: click / triangle / double click, ↑↓←→ Home End PgUp PgDn Enter `*`, context menu Copy key / Copy value / Expand all below / Collapse, disabled hooks; egui-mcp in the real app: →, ↓, End, Enter, Ctrl+Shift+D)

**Progress notes**
- 7a done: `nav::NavCache` (shared checkpoint LRU + per-container read cursor), `children_if_ready` / `child_count_if_ready` / `is_ready` / `prepare(cancel)`; galloping node lookup while walking children: `orders_2g` root (2.45 M children) checkpoint build 295 ms → ~110 ms warm (first touch after a cache load: ~3 s of page faults, hence a worker + loading rows). `tree::TreeModel` (segments, nested groups, expand / collapse / toggle / parent / address / reveal / expansions + restore / expand_all) and `tree::preview::Preview`. Oracle proptests (rows, addresses, parents, ranges, restore, other group size, reveal, preview → full) pass with 5,000 cases. Bench: `rows_for_viewport_50` on 10 M children 4.9 µs, expand root 68 ns; deep 100k by "→" 26 ms in total.

**Quality gates (last run)**
- [x] fmt · [x] clippy (workspace, all targets) · [x] tests (174 passed, 5 ignored big-file tests; new: core 8 nav/cache unit + 6 tree + 2 doctests, app 2 unit + 6 tree kittest; shell tests updated for the preview) · [x] cargo deny · [x] benches (`--quick`, all within 10 % of step 05/06; new `tree_10m/*`). Run 2026-11-27 on M3 Pro.

**What was done**
- Core: `nav::NavCache` + non-blocking reads + `prepare`, galloping child walk, read cursor; `tree::{TreeModel, Rows, Row, RowItem, ValueRow, Expand, ExpandAll, Expansions, Reveal}`, `tree::preview::Preview`. Bench `benches/tree.rs`.
- App: `tree_view.rs` (per-tab tree state, prepare jobs, selection, keys, preview → full switch, group-size change), `ui/tree.rs` (virtual list with f64 scroll, own scrollbar, context menu), `ui/tree_row.rs` (row text, colours, lossy previews), `ui/debug_overlay.rs`; `app.rs` split into `app/{mod,actions}.rs` (under 500 lines). Status bar: the index summary is the hover text of "Index: …".
- Release binary 8.5 MB (was 7.4 MB).

**Known limitations / follow-ups**
- Collapsing a row forgets the expansion state below it (ADR 018).
- Adaptive mode: listing a record that contains a big unstored container scans that container on the UI thread; counts of unstored containers over 64 KiB show `{…}` until expanded (I-009).
- Two test flakes in ~50 runs (I-010), not reproduced in 40 loaded runs of the app suites.
- No tracing subscriber yet, so `tracing::info!` lines (first rows, expand everything) only appear in the overlay; logging setup can come with step 12.

**Decisions** (also in docs/DECISIONS.md)
- 018: segment row model, nested groups, address-based restore.
- 019: shared `NavCache`, `*_if_ready` on the UI thread, `prepare` on workers, galloping lookup, root prefetch.
- 020: own virtual list (f64 scroll), non-focusable rows, keys, deep-row indent cap, system fonts, copy limit, debug overlay.

### Step 08 — Raw text view
**Status:** ✅ Done · **Started:** 2026-11-27 · **Finished:** 2026-11-27

**Plan**
- [x] 8a core `index::lines`: fast `line_start` (via checkpoints, no scan back over a 2 GB line), `line_offset(line)`, bounded `position` (char column, byte column beyond a limit); status bar uses it
- [x] 8a core `pretty`: `PrettyPos` (offset, depth, inside-a-long-token start), `PrettyLine { depth, tokens, source_span }`, resumable generator `pretty_lines(bytes, start, max_lines, opts)`; long tokens wrap at fixed byte steps from the token start (UTF-8 / escape safe), so stepping up/down inside a 200 MB string is O(1)
- [x] 8a core `pretty::anchor`: line start at or before an offset from stored nodes + ready checkpoints; budgeted `seek` (resumable on a worker) and `lines_before`
- [x] 8a core original mode: soft-wrapped rows of file lines (`orig` rows forward/backward, row containing an offset), highlight tokenizer for a byte slice
- [x] 8a core `nav::address_of` (child-index path for the tree) and `goto` (parse `line`, `line:col`, `@offset` with k/m/g, `$.a.b[3]`; resolve)
- [x] 8a tests: unit (indentation, empties, escapes, UTF-8 split safety at slice edges, wrap), proptest round trip (pretty output re-parsed == source), backward == forward, seek == naive; fuzz target `pretty_range`; bench
- [x] 8b app: `raw_view.rs` state (mode, anchor, row window, seek jobs, selection, flash), `ui/raw.rs` (virtualized rows, gutter, highlight, own scrollbar mapped to bytes, wheel), tree | raw split with focus routing
- [x] 8b app: sync tree → raw (scroll + flash) and raw → tree (click → `node_at_offset` → reveal, on a worker when big), text selection + Ctrl/Cmd+C of source bytes (50 MB cap), Go to dialog (Ctrl/Cmd+L)
- [x] 8c kittest: toggle mode, go to line, tree click scrolls raw, raw click selects in tree, copy span
- [x] 8c manual / egui-mcp: orders_min_2g (Pretty, scroll + thumb drag frame times), orders_2g (Original, go to line 12,345,678), huge_string, sync with deep/grouped nodes; docs

**Acceptance criteria** (copied from the step file)
- [x] `orders_min_2g.json` (single line): opens in Pretty mode; scrolling and thumb dragging stay at 60 fps. — *Opens in Pretty (screenshot, avg line > 2,000 B). Optimized build + egui-mcp, overlay after Reset: thumb drag end to end and back (600 steps, warm) CPU frame p99 7.23 / max 15.4 ms; first cold pass had one 36 ms frame (page faults / first glyphs). Keyboard paging (300 keys, same `scroll_by` path as the wheel) CPU frame p99 8.8 / max 13.1 ms. egui-mcp's wheel events at a raw position didn't reach either pane this session, so wheel scrolling is covered by kittests (`mouse_wheel_*`).*
- [x] Pretty output re-parsed equals the source value (test on fixtures ≤ 10 MB, comparing serde values). (`tests/pretty.rs`: golden fixtures × wraps 16/61/400 × full/adaptive index; 400 generated docs per run (40,000 in a stress run); ignored `fixtures_10m` on `orders_10m` + `orders_min_10m` (full and adaptive index). Also compares the whitespace-free bytes exactly, which covers numbers serde can't parse such as `0.1E310`)
- [x] Original mode on the pretty 2 GB fixture shows correct line numbers; "Go to line 12,345,678" lands correctly. — *`orders_2g.json` opens in Original; Cmd+L → `12,345,678` shows lines 12,345,676 … with line 12,345,678 (`  },`, byte 275,184,583, cross-checked with a Python scan) as the third row; tree selects `[314,585]`. kittest `go_to_line_offset_and_path` on a 3,000-line file.*
- [x] Tree ↔ raw sync works both ways, including deep and grouped nodes. — *Real app: tree click → raw scroll + flash (orders_16k); raw click → tree on `orders_16k` (`qty : 3`), `prim_array_10m` at 63.9 % (`[6,385,183]` inside nested groups), `orders_min_2g` (a record field in adaptive mode), `deep_100k` Go to `@450000` → depth 56,250. kittests `raw_click_selects_the_value_in_the_tree`, `tree_selection_scrolls_the_raw_view_and_flashes` (deep path + real tree click + arrow key).*
- [x] `huge_string.json` scrolls through the string without freezing. — *200 MiB fixture (`gen-fixtures string --size 200m`): thumb to 50 %, 250 PageUp/↑/PageDown: UI p99 8.0 / max 10.2 ms, CPU frame p99 10.7 ms (one 39.8 ms frame, new glyphs). Found and fixed on the way: the selected 200 MB string was re-read every frame (700 ms frames) through `selected_spans()`.*
- [x] Copy of a selected span gives the exact source bytes. — *kittest `drag_selection_copies_the_exact_source_bytes` (drag across rows with Armenian/CJK, compares with the file bytes; Shift+click extends); `copy_over_the_limit_shows_a_message` (51 MB).*

**Progress notes**
- 8a done: `LineTable::{line_start, line_offset, position}` (no backward scan over a long line); `pretty::{pretty_lines, Generator, anchor_before, seek, prev_line, SeekState}`, `pretty::original::{row_at, next_row, prev_row}`, `pretty::lex::{tokenize, state_in_row}`, `nav::Navigator::address_of`, `goto::{parse, resolve_line, resolve_path}`. Fuzz `pretty_range` 502,697 runs clean. Bench (100 MB): viewport 2.6 µs, jump to any offset 2.2 µs, a viewport back 1.4 ms, original jump + viewport 1.7 µs.
- Found on the way: `cancel_returns_quickly` flakes (≈ 1 in 4 runs, 57–60 ms vs 50 ms) when it runs next to the 10,000-case proptests in the same binary; reproduced on the step 07 commit, so not caused by this step (I-011).

- 8b/8c done: raw view (state `raw_view/`, UI `ui/raw.rs` + `ui/raw_input.rs`), `panes.rs` (focus, sync, locate jobs), Go to dialog, split layout, kittests, manual checks above. Fixes found while driving the real app: a click resolved to the next character (caret rounding → now the glyph under the pointer); the selected spans were recomputed every frame (200 MB string → 700 ms frames; now cached per row + generation); scrolling up kept every passed row in the layout cache (4,483 rows laid out per frame; now trimmed); off-screen rows registered inverted accessibility rects.

**Quality gates (last run)**
- [x] fmt · [x] clippy (workspace, all targets) · [x] tests (207 passed, 0 failed, 6 ignored: 5 big-file tests pass with `--ignored --test-threads=1` (the RSS test is disturbed when they run in parallel, as before), `pretty::fixtures_10m` passes with `--ignored`; two full runs; new: core 2 lines + 8 pretty + 2 wrap + 2 goto unit, 4 doctests, `tests/pretty.rs` 5, nav proptest checks `address_of`; app 2 unit, `tests/raw.rs` 9) · [x] cargo deny · [x] benches (`--quick`, all within 10 % of step 07; new `pretty` bench) · fuzz `pretty_range` 502,697 runs clean. Run 2026-11-27 on M3 Pro.

**What was done**
- Core: `pretty::{pretty_lines, Generator, PrettyPos, PrettyLine, Token, TokenKind, anchor_before, seek, SeekState, prev_line}`, `pretty::original::{row_at, next_row, prev_row, OrigRow}`, `pretty::lex::{tokenize, state_in_row, LexState}`, `goto::{parse, resolve_line, resolve_path}`, `Navigator::address_of`, `LineTable::{line_start, line_offset, position, TextPos}`; fuzz target `pretty_range`; bench `pretty`.
- App: raw view (Pretty / Original, gutter, highlight, byte-mapped scrollbar, wheel/keys, selection + copy, flash), tree | raw resizable split with keyboard focus per pane, sync both ways, "Show in raw view", Go to dialog (Ctrl/Cmd+L), shortcuts dialog entries, status bar column bounded on long lines. `tree_view.rs` → `tree_view/{mod,keys}.rs`. All files < 500 lines.

**Known limitations / follow-ups**
- Rows wrap every 400 **bytes** (not characters) of a long line or token; same for ASCII (ADR 021).
- Revealing a value ~50k levels deep (Go to / raw click in `deep_100k`) costs one 20–60 ms frame in the tree model (I-012).
- Hiding the tree or the raw pane (architecture §7) isn't there yet; the split is resizable (I-013).
- Original mode: after a far jump into the middle of a very long line, colours may be wrong for a moment until the worker finds the string state (only when that needs > 1 MiB of scanning).
- No horizontal scrollbar: long rows scroll sideways with Shift+wheel / the trackpad.
- Search match highlighting in the raw view comes with step 10.

**Decisions** (also in docs/DECISIONS.md)
- 021: pretty layout as a pure function of the bytes, long tokens split at byte steps from the token start, anchors from the index, budgeted resumable seek.
- 022: raw view UI (byte-anchored scrolling, shared `NavCache`, gutter offsets, sync, focus, copy, Go to on a worker).

### Step 09 — Search engine (core)
**Status:** ✅ Done · **Started:** 2026-11-27 · **Finished:** 2026-11-27

**Plan**
- [x] Deps via `cargo add`: `aho-corasick`, `regex`
- [x] `search/engine.rs`: pick memmem (case-sensitive) / Aho-Corasick ASCII-ci / `regex::bytes` Unicode-ci (+ Armenian `և` ↔ `եվ`/`եւ`), whole-word filter, max match length
- [x] `search/chain.rs`: leftmost-first non-overlapping chain over a chunk window (matches *starting* in the chunk), cancel checks, stitching across chunk borders with resync
- [x] `search/run.rs`: workers on the rayon pool, per-chunk slots, ordered publisher, store with cap + memory budget, per-chunk chain starts for on-demand `next_after` / `prev_before` beyond the cap
- [x] `search/mod.rs`: `SearchQuery`, `Scope`, `SearchOptions`, `Match`, `start_search`, `SearchHandle` (count/progress/stored/get/index_of_offset/next/prev/cancel on drop/wait)
- [x] Tests: unit, proptest oracle (naive non-overlapping count) with chunk sizes 1–8 MiB, JSON inputs, Unicode regex oracle, fixtures, cap = 10 next/prev, cancel, drop, BOM document, ignored 100 MB set and 2 GB manifest + timing test
- [x] Fuzz target `search_any`; criterion bench `search` (`search_cs_100m`, `search_ci_100m`)
- [x] Docs: PERF.md, DECISIONS.md (ADR 023), CHANGELOG, THIRD_PARTY_LICENSES, fuzz README, PROGRESS

**Acceptance criteria** (copied from the step file)
- [x] Count equals a naive count on all fixtures ≤ 100 MB and on proptest inputs, with chunk sizes 64, 65, 4096 and 8 MiB. (every file of `gen-fixtures all --max-size 100m`, incl. invalid/encoding/diff, × 6 queries × chunk sizes 64/65/127/4096/8 MiB: `counts_match_on_fixtures_up_to_100m`, release, 75 s; 10 MB of each kind in the normal suite; proptests also check stored order and full next/prev walks)
- [x] Planted needles in `orders_2g.json` give exactly the counts in its manifest. (and `orders_min_2g.json`, case-sensitive and case-insensitive)
- [x] 2 GB warm: case-sensitive count **< 1.5 s**; case-insensitive **< 3 s** (record in `docs/PERF.md`). (19–20 ms / 19–82 ms; 72–93 ms for the first search on a freshly mapped warm file)
- [x] First result available in < 200 ms for a common word. (`email`: 4–13 ms)
- [x] Cancel stops all workers within 50 ms. (0.05–8 ms on 2 GB; test `cancel_stops_all_workers_quickly` on 256 MiB in debug)
- [x] `next_after` / `prev_before` are correct across the stored cap (test with cap = 10). (`next_prev_across_a_cap_of_ten` for every chunk size and arbitrary offsets; proptest/fuzz with caps 0, 1, 3/5, 10)
- [x] Unicode case-insensitive: `ЁЛКА` finds `ёлка`; `ԱՐԵՎ` finds `արև`. (`արև` is spelled with the ligature `և`; handled by an explicit rule, ADR 023)

**Quality gates (last run)**
- [x] fmt · [x] clippy · [x] tests (234 passed, 0 failed, 8 ignored; the ignored big-file tests pass with `--ignored`) · [x] cargo deny · [x] benches (all within 10 % of step 08) · fuzz `search_any` 826,872 runs clean. Run 2026-11-27 on M3 Pro, macOS.

**What was done**
- `braceray_core::search::{start_search, SearchHandle, SearchQuery, SearchOptions, Scope, Match}`; `Error::{SearchQuery, SearchUnsupported}`; `Document: AsRef<[u8]>`.
- Non-overlapping leftmost-first semantics, kept exact across chunk borders by stitching (a straddling match moves the chain's entry into the next chunk; the publisher rescans until the chains meet). Mutation check: removing the carry makes the proptest fail at once.
- Results are stored up to 5 M (12 B each), with bounded memory for chunks still waiting to be published; beyond the cap the count stays exact and next/prev scan only the chunks around the offset.
- Dev profile: `memchr`, `aho-corasick`, `regex-automata` at opt-level 3 so the search tests stay fast.

**Known limitations / follow-ups**
- API differs from the step file: no `Arc<Index>` argument and a `Result` return (ADR 023).
- Regex and keys/values scope return `SearchUnsupported` until step 13.
- `next_after` / `prev_before` only answer within the published part of the file, and beyond the cap they scan (≤ one 8 MiB chunk per call for normal data, more when no match is near), so the UI should call them on a worker when `is_capped()` (step 10).
- Workers share the global rayon pool: a search started during an index build may wait for pool threads (I-014).
- ASCII case-insensitive search doesn't fold the Kelvin sign `K` or the long s `ſ` to `k` / `s` (Aho-Corasick ASCII folding); non-ASCII queries use full Unicode simple folding.

**Decisions** (also in docs/DECISIONS.md)
- 023: engines, Armenian ligature rule, whole-word rule, chain stitching, capped store with memory budget, chain starts for navigation beyond the cap, API differences.

### Step 10 — Search UI
**Status:** ⚠️ Done with known issues · **Started:** 2026-11-27 · **Finished:** 2026-11-27

**Plan**
- [x] Core: dedicated search thread pool (I-014); `SearchHandle::matches_in` (highlights) and a cloneable `SearchNav` for next/prev on a worker; `path::jsonpath` formatting of `PathSegment`s
- [x] Core test: paths of the first 1000 matches equal a serde walk (small fixtures)
- [x] App `search_view`: per-tab search state (handle, current match, next/prev from the selection with wrap, beyond-cap navigation on a worker, pending jump while indexing, row info LRU + path/line job, copy-all-paths job)
- [x] App `ui/search.rs`: search bar (debounce 150 ms, Enter/Shift+Enter, Esc, `Aa`, `ab|`, disabled `.*` + scope, count text, history dropdown (20, persisted))
- [x] App `ui/results.rs`: bottom panel (resizable, collapsible, Cmd/Ctrl+Shift+F), virtual list with own f64 scroll, ↑/↓/Enter, header (count, cap note, Copy all paths → clipboard or temp file + Open)
- [x] Highlights: raw view soft background for visible matches + accent for current; tree markers for rows containing stored matches; status bar count/index; "Wrapped to top" toast
- [x] Shortcuts: Ctrl/Cmd+F, F3/Shift+F3, Cmd+G/Cmd+Shift+G (macOS), shortcuts dialog entries
- [x] kittests: count label, Enter/F3 move the selection, result click selects the node, toggles re-run, Esc clears, (+ wrap, history, panel toggle, cap, while indexing, copy all, Windows/macOS shortcuts)
- [x] Check in the optimized app with egui-mcp on `orders_2g.json`: first results, list scrolling, first/middle/last/beyond-cap jumps; search while indexing in the 60 fps `search_big` harness
- [x] Docs: DECISIONS (ADR 024), PERF, CHANGELOG, PROGRESS

**Acceptance criteria** (copied from the step file)
- [x] Typing "needle_common" in the 2 GB file: the first results appear in < 200 ms; the final count matches the manifest. (24,503 = manifest. Search start → first result row on screen 17–35 ms in a 60 fps harness, also while indexing; 12 ms in the app's overlay. From the keystroke it is 187–222 ms because the same step asks for a 150 ms debounce: see the question below and ADR 024)
- [x] The results list scrolls smoothly through millions of rows. (`email`, 2,450,341 rows: CPU frame p99 4.5 ms, max 6.0 ms)
- [x] Clicking any result (first, middle, last, beyond the stored cap via F3) lands on the right node in the tree and the right bytes in the raw view. (app + screenshots: #1, #3, #12,253, #24,503; `e` with 73 M matches: F3 past #5,000,000; kittests with a cap of 1,000)
- [x] Changing a toggle re-runs the search instantly; typing cancels the previous search (no stale results). (kittest `toggles_rerun_the_search_and_typing_replaces_it`; a new query drops the old handle, which cancels it)
- [x] Search works while indexing is still running; paths fill in when the index is ready. (kittest with indexing held; `search_big` on 2 GB during the build)
- [ ] All keyboard shortcuts work on Windows and macOS. (macOS in the app; kittests with egui set to Windows (Ctrl) and macOS (Cmd, ⌘G) pass. Not run on a real Windows machine yet: I-015, step 12)

**Quality gates (last run)**
- [x] fmt · [x] clippy · [x] tests (256 passed, 0 failed, 9 ignored; ignored `search_big` (app and core) pass with `--ignored`) · [x] cargo deny · [x] benches (search faster; others within noise of step 09, see PERF.md). Run 2026-11-27 on M3 Pro, macOS.

**What was done**
- Core: search workers on their own rayon pool (I-014); `SearchHandle::matches_in` / `nav()` + `SearchNav`; `path::{jsonpath, decode_key}`; tests `search_paths` (serde-walk oracle for the first 1000 matches on generated orders, pretty and minified; `matches_in` vs the store with caps 0–5 M; `SearchNav` from another thread; a search while every global pool thread is busy).
- App: `search_view` (per-tab state, row info LRU + job, copy-all job), `tab_search` (jump, next/prev from the selection, retry while searching, pending jump past the preview), `ui::search` (bar), `ui::results` (panel), `ui::vlist` (scrollbar shared with the tree), raw highlights + horizontal reveal, tree markers, status bar, shortcuts, history in settings, search timings in the debug overlay.
- Found while checking in the app: a match far into a long line was off screen (added the horizontal reveal); pretty-file previews showed runs of indentation (collapsed); `✕` isn't in the system fonts (now `×`).

**Known limitations / follow-ups**
- Shortcuts not tried on real Windows (I-015).
- Keystroke → first result is 150 ms debounce + 1–2 frames (≈ 170–220 ms); see the question for Martin.
- Tree markers come from stored matches only (beyond a 5 M cap no markers past the last stored match); raw highlights beyond the cap scan only the bytes on screen.
- The results panel's preview is cut by bytes (±32 / 48) and the path column by width; very long paths are clipped (full path in the tooltip later, with step 11's copy path).

**Decisions** (also in docs/DECISIONS.md)
- 024: one bar for all tabs, disabled v1 controls, cursor from the selection, retry while searching, lazy row info, highlight sources, history rule, copy-all to file above 10k, own search pool, how "first result < 200 ms" is measured.

### Step 11 — Breadcrumbs & copy
**Status:** ✅ Done · **Started:** 2026-11-28 · **Finished:** 2026-11-28

**Plan**
- [x] Core `path/`: `PathFormat` (JSONPath RFC 9535, jq, JavaScript with root name, JSON Pointer), quoting/escaping per format, reserved words; `decode_string` (shared with `decode_key`); split into `path/{mod,decode,tests}.rs`
- [x] Core tests: table tests (spaces, quotes, dots, unicode, empty, numeric-looking, reserved words, control chars) for every format; proptest random keys → path → resolve with a reference (`serde_json_path` for JSONPath, `Value::pointer`) → same value; optional `jq` / `node` check when installed
- [x] Core `copy`: streaming re-formatter for a byte range (raw, pretty with indent, minified, string contents) with cancel + progress, no DOM; tests vs serde (parsed values equal), bench 10 MB < 300 ms
- [x] App: `TreeModel` ancestor rows + `TreeView` cached breadcrumb chain and path segments of the selection
- [x] App `ui/breadcrumb.rs`: bar under the toolbar, clickable segments, collapse in the middle with `…` to expand, sibling dropdown on hover (first 200 + "…", virtualized), copy-path button + format dropdown (last choice in settings)
- [x] App copy: context menu (Copy path ▸ formats, Copy value / formatted / minified / key / string contents), copy job with progress for big values, size guard dialog (> 50 MB setting: Export… / Copy anyway / Cancel), export raw range via save dialog on a worker, toasts ("Copied JSONPath", "Copied 12.4 MB")
- [x] Shortcuts: Ctrl/Cmd+Shift+C copy path (last format), Ctrl/Cmd+C copy value when the tree has focus; shortcuts dialog
- [x] Settings: path format, JS root name, copy size limit (settings window)
- [x] kittests: breadcrumb click selects the ancestor, sibling dropdown jumps, copy actions produce the expected clipboard text, size guard appears above the limit, 1M-child array navigation
- [x] Check in the app with egui-mcp (1M-child array, deep file, big value copy) — also on `orders_2g.json`; fixes found: segment of a not-yet-prepared row vanished (now a spinner + prepare), "all levels" took half the window (now a ~4-line scroll area)
- [x] Docs: DECISIONS, PERF, CHANGELOG, PROGRESS

**Acceptance criteria** (copied from the step file)
- [x] Paths for keys with spaces, quotes, dots, unicode, empty string, numeric-looking keys and reserved words are correct in every format (table-driven tests). (`path::tests::expected_text_per_format`, 30 tricky keys × 4 formats, top level and nested)
- [x] Each copied path, evaluated by a reference implementation over `serde_json::Value` (JSONPath / Pointer in tests; jq if available in CI, optional), returns the selected value. (`serde_json_path` (RFC 9535) + `Value::pointer`: table, proptest of random keys with/without `\u` escapes, and every value of random documents via `node_at_offset` + `path_of`; jq 1.7.1 and node 23 run the jq / JavaScript paths when installed)
- [x] Copying a 10 MB object takes < 300 ms; copy formatted equals the serde pretty output (whitespace style aside: compare parsed values). (criterion `copy_10m` on a 10 MB orders object: raw 1.1 ms, pretty 13.2 ms, minified 10.9 ms; in the app a 6.9 MB array → 9.89 MB pretty on the clipboard. Pretty output equals `to_string_pretty` byte for byte for compact serde input (proptest, indent 2 and 4) and parses to the same value for every value of random documents)
- [x] The size guard triggers above the limit. (kittest `size_guard_asks_above_the_limit`: under the limit copies at once, above it asks, Cancel / Copy anyway (worker job); app: 2.15 GB root → "This value is 2.15 GB…" with Export… / Copy anyway / Cancel; Export… opens the save panel)
- [x] Breadcrumb navigation and sibling dropdown work on a 1M-child array. (kittest `million_child_array_navigation_and_siblings`; app: `million.json` `[523456]` → hover → `[3]`, and `orders_2g` root (2,450,341) `[1234567]` → "… 2,450,141 more" → `[5]`)

**Quality gates (last run)**
- [x] fmt · [x] clippy (also `--target x86_64-apple-darwin` and core for `x86_64-pc-windows-msvc`) · [x] tests (288 passed, 0 failed, 9 ignored) · [x] cargo deny · [x] benches (`copy_10m`: raw 1.1 ms, pretty 13 ms, minified 11 ms; all other benches within 10 % of step 09/10 on a re-run, see PERF.md). Run 2026-11-28 on M3 Pro, macOS.

**What was done**
- Core: `path::{PathFormat, format_path}` (JSONPath RFC 9535 / jq / JavaScript / JSON Pointer, per-format quoting, decoded keys), `copy::{render, write_to}` (raw / pretty / minified / string contents, one pass, no DOM, cancel + progress per 4 MB of input and output, indentation capped at 64 levels, output limit), `TreeModel::ancestors`, `Error::{CopyTooLarge, Write}`. Tests: tables, proptests against `serde_json_path` / `Value::pointer`, jq and node when installed, `tests/paths.rs` (every value of random documents), bench `copy`.
- App: breadcrumb bar (`ui::breadcrumb`), size guard (`ui::copy_guard`), per-tab `copy_job::CopyState` (sync ≤ 1 MB, worker above, export job), copy actions in the context menu and shortcuts, settings (path format, JS root, copy limit) in the settings window, recording clipboard hook for tests; kittests `tests/copy.rs` (6) and `copy_job` unit tests (4).
- Security review fix: pretty copy of `[[[[…` could grow with depth² before the next cancel / flush check (2bb48fc).

**Known limitations / follow-ups**
- Copy path of a value tens of thousands of levels deep reads every level on the UI thread (`deep_100k` middle, 56,250 levels: one ~17 ms frame) — I-016.
- Not driven end to end in the app: Ctrl/Cmd+C (egui-mcp sends a key, not egui's copy event; the copy event is covered by kittests) and the Export… save panel (out-of-process macOS panel; `export_to` is unit tested). Please try both by hand.
- The sibling list shows `key` / `[i]` only (no value preview).

**Decisions** (also in docs/DECISIONS.md)
- 025: dot-form rules per format, reference checks, whitespace-only formatter with bounded output, guard on input size and sync/worker split, breadcrumb chain from the row model, hover dropdown, clipboard and test hook, remembered format.

---

### Step 12 — MVP hardening, benchmarks & release v0.1.0
**Status:** 🟦 In progress · **Started:** 2026-11-28 · **Finished:** —

**Plan**
- 12a performance
  - [x] `gen-fixtures bench-big`: generate the 2 GB fixtures if missing; open → first content, full index (warm, cold with `--cold`), index memory, re-open from cache, search cs / ci (rare + common), first result, expand root, peak RSS; Markdown rows for PERF.md, ✅/❌ vs overview.md, non-zero exit on a miss
  - [x] Run it on M3P (both 2 GB files): every measured target met; cold run by Martin with `sudo`: index 2.78 / 3.48 s, first content 16 / 20 ms ✅
  - [x] "App memory excluding the mapped file < 300 MB + index": `alloc-stats` measurement build (ADR 027): 476 MB steady with a 471 MB index, 979 MB peak while building, 5 MB from cache ✅
  - [ ] Frame-time check of tree / raw / results on the 2 GB file with the overlay (`Ctrl+Shift+D`, exists since step 07: p50 / p99 / max over 600 frames) — re-check in the release build
- 12b robustness
  - [x] Fuzz all targets ≥ 10 min each (`index_any_bytes`, `search_any`, `pretty_range`; `jsonpath_parse` comes with step 14); fix crashes, add inputs to the corpus
  - [x] Panic hook: crash log in the config dir + dialog "Braceray hit a bug. Log saved to … [Open issue on GitHub]" (prefilled issue URL, nothing sent)
  - [x] Open every catalogue fixture (encoding, `invalid_*`, 5 GB sparse) through the UI via egui-mcp: no panics, clear messages (36 files: all 9 encoding cases, 13 `invalid_*` incl. the 1 GB "error at the end", 5 GiB sparse, deep / wide / prim / huge string, orders 10m / 2g pretty + minified, NDJSON, diff; "Open anyway" shows the valid prefix)
  - [x] Close a tab during indexing / search / copy: memory back to baseline, no panics; modify the file during indexing → reload banner. Heap back to 6.6 MB (`alloc-stats`); RSS can keep up to ~1 index of freed allocator pages for a while but doesn't grow over open/close cycles (I-017). Append / overwrite / delete → banner + Reload; **truncate crashed with SIGBUS → fixed with a SIGBUS guard (ADR 028)**
  - [x] CI runs `cargo test` again + generates the 100 MB fixture set (I-001, I-002); fix the known test flakes (I-011 ✅; I-010 not seen in CI so far). Fixed on the way: job repaint race, Windows `\` paths and 100 ns mtimes, search `wait_jump` returning before the tree reveal, cancel headroom on CI
  - [ ] Open backlog items planned for step 12 — Martin (2026-11-28): fix them now. Done: I-018 (early error), I-017 (investigated, won't fix), I-013 (pane toggles). To do: I-012/I-016 deep reveal / path, I-009 adaptive scan, I-007 macOS Open With
- 12c branding
  - [ ] Icon SVG (`{ }` + cyan scan beam on a dark rounded square) → PNG 1024…16, `.icns`, `.ico`; window icon via `ViewportBuilder::with_icon`
  - [ ] About dialog: name, version, tagline, MIT, GitHub link, third-party licenses viewer
- 12d packaging
  - [ ] `packaging/macos/Info.plist` (bundle id, `public.json` viewer, LSMinimumSystemVersion 11.0); Windows icon + version info via `winresource` in `build.rs`
  - [ ] `.github/workflows/release.yml` on tag `v*`: Windows x64 zip, macOS universal `.app` (lipo, ad-hoc codesign) in a dmg, Linux tar.gz, SHA256SUMS, `gh release create` with the changelog section
  - [ ] Dry run of the workflow (e.g. a pre-release tag, with Martin's OK) before `v0.1.0`
- 12e docs & release
  - [ ] README: screenshot/GIF, features, Install (SmartScreen / Gatekeeper steps), build from source, shortcuts, performance table, privacy, license
  - [ ] `CHANGELOG.md` `[0.1.0]`, version 0.1.0 in the workspace, release checklist (testing-strategy.md) in a release issue
  - [ ] Martin: tag `v0.1.0`, try the downloads on clean Windows + Apple Silicon

**Acceptance criteria** (copied from the step file)
- [ ] Every MVP target in `overview.md` is met on at least one reference machine, and the numbers are in `docs/PERF.md`.
- [x] Fuzzing finds no crashes in 10 min per target. (2026-11-28: 11 min each for `index_any_bytes`, `search_any`, `pretty_range`, 1.1–1.9 M runs each, 0 crashes; `jsonpath_parse` doesn't exist until step 14)
- [ ] `git tag v0.1.0 && git push --tags` produces a GitHub Release with Windows zip, macOS dmg, Linux tar.gz and checksums.
- [ ] Downloaded builds launch on a clean Windows machine and a Mac (Apple Silicon) following the README steps.
- [ ] The release checklist in `testing-strategy.md` is completed and pasted into the release PR or issue.
- [ ] `CHANGELOG.md` has a `[0.1.0]` section.

**Quality gates (last run)**
- [x] fmt · [x] clippy (also x86_64 macOS / Windows core) · [x] tests (301 passed, 0 failed, 9 ignored) · [x] cargo deny · [x] benches (`--quick`: all within 10 % except `cache_store`, the known noisy file-write bench: 11 ms, reruns 9–50 ms; the guard isn't on its path). Run 2026-11-28 on M3 Pro, macOS; Linux build of the guard only checked by CI.

**What was done**
- 12a: `gen-fixtures bench-big` (`tools/fixtures/src/bench_big/`); first run on M3P recorded in PERF.md (all measured targets ✅).
- 12b UI pass (egui-mcp, optimized debug build, scratch HOME): every catalogue fixture opened; tab closed at 0.3 / 1 / 2.5 s of indexing, during a 73 M-match search and during 900 MB raw / pretty copies (no panic, heap to baseline); file appended / overwritten / deleted / truncated during and after indexing. Found and fixed: SIGBUS on truncation (`mmap::guard`, ADR 028).
- 12b: `alloc_stats` measurement feature (ADR 027, Martin OK'd the `unsafe`), heap numbers in PERF.md. `crash` module (panic hook, crash log, dialog process, prefilled issue URL; ADR 026); unit tests + `tests/crash.rs` runs the binary with `--crash-test`; dialog checked on macOS.

---

## Session log
Newest first. One entry per Claude Code session (keep each entry 3–6 lines).

<!--
### YYYY-MM-DD HH:MM · Step NN
- Did: …
- Tests: N passed, 0 failed · CI: green/red
- Commits: abc1234, def5678
- Next: …
-->

### 2026-11-28 (3) · Step 12
- Did: 12b UI robustness through egui-mcp (Python stdio driver, opt-level 3 debug build, scratch HOME): all catalogue fixtures (36) give clear messages, no panics; close tab during index / search / copy (heap to baseline; RSS retention I-017); file modified during indexing. Truncation crashed the app with SIGBUS (the macOS "quit unexpectedly" dialog Martin saw) → SIGBUS guard in `mmap` (ADR 028), re-checked in the app. New backlog: I-017, I-018. A stray "Copied 2 B" notice appeared once in a scripted run and didn't reproduce in 3 reruns
- Security review: handler could pair one slot's start with another's length while slots churn → per-slot seqlock + churn test
- Tests: 301 passed, 0 failed, 9 ignored · fmt/clippy/deny clean
- Commits: 2b0234f + seqlock fix; not pushed
- Next: Martin: backlog items for step 12 (keep / move); then 12c branding

### 2026-11-28 (2) · Step 12
- Did: started step 12. `gen-fixtures bench-big` (all measured MVP targets ✅ on M3P, PERF.md); crash handler (log + dialog process + prefilled issue, ADR 026); fuzzed 3 targets 11 min each, 0 crashes; I-011 fixed (own test binary); `alloc-stats` heap counter (ADR 027, Martin OK'd the `unsafe` as measurement-only): 476 MB with a 471 MB index, 5 MB from cache; CI runs tests again + fixtures/bench job (I-001, I-002, I-006 resolved)
- CI: first runs red (cancel limit on slow runner, Windows paths / 100 ns mtimes, repaint race, search `wait_jump` too early); all fixed → run 46114991172 green on Windows, macOS, Linux
- Tests: 296 passed, 0 failed, 9 ignored · fmt/clippy/deny clean
- Commits: 589d0fc … 6530f2d; pushed (Martin's OK)
- Next: rest of 12b (UI robustness via egui-mcp), then 12c branding, 12d release workflow, 12e docs

### 2026-11-28 (1) · Step 11
- Did: step 11 end to end. Core path formats (JSONPath / jq / JavaScript / Pointer) checked against `serde_json_path`, `Value::pointer`, jq 1.7.1 and node; streaming copy formatter (exact serde pretty for compact input, 10 MB pretty in 13 ms); `TreeModel::ancestors`. App breadcrumb bar (collapse, all levels, hover sibling list), copy path button + format menu + Ctrl/Cmd+Shift+C, copy value styles, size guard with Export…, settings. Checked in the app on `orders_2g`, a 1M array and `deep_100k` via egui-mcp; fixed two things found that way (spinner for unprepared segments, scrolling "all levels" area)
- Security review: pretty copy of deeply nested input grew with depth² between checks → indentation cap, output-based checks, output limit (2bb48fc)
- Tests: 288 passed, 0 failed, 9 ignored · fmt/clippy (incl. x86_64 macOS) / deny clean · benches within 10 % on a re-run (first run under load)
- Commits: fd02f47, 2bb48fc, d6bf770; not pushed
- Next: Martin reviews step 11; then step 12

### 2026-11-27 (7) · Step 10 review
- Martin checked step 10 in the app ("all looks good"); the 150 ms debounce stays. Step stays ⚠️ only for the real-Windows shortcut check (I-015, step 12)
- Pushed steps 04–10. CI run 46036432255 failed on Windows and Linux: `clippy::large_enum_variant` (x86's `memmem::Finder` is ~288 bytes, small on aarch64). Boxed it (b70d2df); run 46037886367 green on all 3 OSes. Two security-review fixes for the "Copy all paths" temp file (O_EXCL, mode 0600)
- Lesson: before pushing, run `cargo clippy --target x86_64-apple-darwin` (and `-p braceray-core --lib --target x86_64-pc-windows-msvc`) to catch x86-only lints
- Next: step 11 (breadcrumbs & copy)

### 2026-11-27 (6) · Step 10
- Did: step 10 search UI end to end: core search pool (I-014), `matches_in`, `SearchNav`, JSONPath formatting + serde-walk oracle; app search bar, per-tab search state, results panel, raw highlights, tree markers, status, shortcuts, history, copy all paths. Checked in the optimized app on `orders_2g.json` through egui-mcp (counts, jumps first/middle/last/beyond the 5 M cap, 2.45 M-row list p99 4.5 ms) and with a 60 fps harness for first-result timing. Fixed three things found that way (horizontal reveal, preview whitespace, missing glyph).
- Tests: 256 passed, 0 failed, 9 ignored (2 GB tests pass with `--ignored`) · fmt/clippy/deny clean · benches: search faster, rest within noise
- Commits: de4c6e2 (10a core), e8bbdd6 (10b app + docs), plus a fix from the automated security review (temp file with O_EXCL); not pushed
- Next: Martin reviews step 10 and answers the debounce question; then step 11 (breadcrumbs & copy)

### 2026-11-27 (5) · Step 09
- Did: step 09 search engine end to end: engines (memmem / Aho-Corasick / Unicode regex + Armenian `և` rule), chunk chains stitched across borders, ordered publisher, capped store with memory budget, next/prev beyond the cap; proptest + fixture oracles, 2 GB test, fuzz target, bench. Found and fixed a test bug (an earlier search kept running and delayed the cancel check) and showed that the slow "first search after open" runs were page-cache evictions, not page faults.
- Tests: 234 passed, 0 failed, 8 ignored (100 MB set and 2 GB tests pass with `--ignored`) · fmt/clippy/deny clean · benches within 10 % · fuzz `search_any` 826,872 runs clean
- Commits: 2419a48; not pushed
- Next: Martin reviews step 09; then step 10 (search UI)


### 2026-11-27 (4) · Step 08
- Did: step 08 raw view end to end. Core `pretty` (position-independent row layout, anchors, budgeted seek, rows backwards, original-mode rows, lexer), `goto`, `address_of`, fast line-table lookups. App: raw view next to the tree with sync both ways, selection + copy, Go to, focus per pane. Checked every criterion in the optimized app through egui-mcp (orders_min_2g thumb drag p99 7.2 ms, Go to line 12,345,678 on orders_2g, 200 MB string, deep and grouped sync); fixed four bugs found that way (click caret rounding, per-frame re-read of a selected 200 MB string, unbounded row cache when scrolling up, off-screen row rects).
- Tests: 207 passed, 0 failed, 6 ignored · fmt/clippy/deny clean · benches within 10 % · fuzz `pretty_range` 502,697 runs clean. Pre-existing flake `cancel_returns_quickly` seen once (I-011, also on the step 07 commit)
- Commits: ebda464 (8a core), d4c6a23 (8b app + docs); not pushed
- Next: Martin reviews step 08; then step 09 (search engine)

### 2026-11-27 (3) · Step 07
- Did: step 07 tree view end to end. Core: shared `NavCache`, non-blocking child reads + `prepare` jobs, galloping lookup (2 GB root table 295 → 110 ms), `tree` row model (segments, nested groups, addresses, restore) and preview index. App: virtual tree list with own f64 scroll, colours, groups, keyboard, context menu, preview while indexing, debug overlay. Checked every criterion in the opt-level 3 app through egui-mcp (2 GB preview rows in 134 ms, 10 M-row scrolling ≤ 1.2 ms frames, deep 100k, Unicode light/dark).
- Tests: 174 passed, 0 failed, 5 ignored · fmt/clippy/deny clean · benches within 10 %
- Commits: fe313f8 (7a core), e202057 (7b app); not pushed
- Next: Martin reviews step 07; then step 08 (raw view)

### 2026-11-27 (2) · Step 06
- Did: drove the debug app with `egui-mcp` (Martin installed it) over stdio JSON-RPC: opened a fixture via Recent files and read the status bar (egui_mcp criterion ✅); resize-latency probe during 2 GB indexing (median 2.7 ms / p95 13.8 ms optimized); Cancel through the UI. Fixed recent files storing non-canonical paths (duplicates); README `egui-mcp` command.
- Tests: 145 passed, 0 failed, 5 ignored · fmt/clippy/deny clean
- Commits: `fix: store canonical paths in recent files` (see git log); not pushed
- Martin confirmed window drag, native dialog and System theme → step 06 ✅
- Next: start step 07 (tree view)

### 2026-11-27 · Step 06
- Did: step 06 app shell: core job plumbing (`JobHandle<P, T>`), settings + recent files, theme, tabs, open by dialog / drag & drop / CLI / recent, indexing progress with Cancel, error banner, status bar, settings window, shortcuts dialog, change watcher, debug inspection server. 10 kittest tests. Release app checked with the 2 GB fixture (3.2 s build in the app, 0.04 s from cache).
- Tests: 144 passed, 0 failed, 5 ignored · fmt/clippy/deny clean
- Blocked: `cargo install egui_mcp` denied by the permission policy (see Blockers)
- Commits: see git log (`feat: app shell (step 06)`); not pushed
- Next: Martin's manual checks (drag/resize during indexing, dialog, System theme, egui_mcp), then mark 06 ✅ and start step 07

### 2026-11-27 · Step 05
- Did: step 05 index cache end to end (`cache/` module, zero-copy `NodeStore`, tests, 2 GB re-open test, criterion bench). Dropped `directories` (MPL-2.0 dependency, question for Martin). Found that the `scalar_index` bench depends on code layout: the identical source runs at 139 ms or 280 ms depending on the build dir (I-005).
- Tests: 120 passed, 0 failed, 5 ignored (pass with `--ignored`) · fmt/clippy/deny clean · 2 GB re-open from cache 14 ms
- Commits: bbc18fb; not pushed (4 commits of step 04 also unpushed)
- Next: Martin reviews step 05; then step 06 (app shell)

### 2026-11-25 23:50 · Step 03 / 04
- Did: pushed step 03; step 04 done in four commits: 4a scalar reference, 4b SIMD classifier, 4c parallel builder, 4d navigation + benches + fuzz. Installed the `x86_64-apple-darwin` target (Rosetta testing), nightly and `cargo-fuzz`.
- Tests: 101 passed, 0 failed, 4 ignored big-file tests (pass with `--ignored`) · fmt/clippy/deny clean · core passes as x86_64 under Rosetta (SSE2/AVX2) · fuzz 546,754 runs clean · 2 GB warm index 1.18 s, 21.9 % memory
- Commits: eac9cbd (4a), 1c02d57 (4b), 16b7adc (4c), 4d commit (see git log); not pushed yet
- Next: Martin reviews step 04; then step 05 (index cache)

### 2026-11-25 21:30 · Step 01 / 03
- Did: CI run 45650452101 green on all 3 OSes, so step 01 ✅. Step 03 (`Document`, mmap, encoding detection, file identity, change detection) implemented end to end.
- Tests: 55 passed, 0 failed, 1 ignored (2 GB test passes with `--ignored`) · fmt/clippy/deny clean · 2 GB open < 6 ms, +2 MiB RSS · 5 GiB sparse file OK
- Commits: step 03 commit (see git log)
- Next: Martin reviews step 03; then step 04 (structural index & validation)

### 2026-11-25 20:30 · Step 01 / 02
- Did: added the MVP time estimate; CI run for step 01 checked (macOS job not picked up by GitHub, not a code failure); step 02 implemented end to end.
- Tests: 31 passed, 0 failed · fmt/clippy/deny clean · 2 GB orders in 1.3–1.6 s · `all --max-size 100m` 0.81 s, SHA-256 identical across runs
- Commits: step 02 commit (see git log)
- Next: push, confirm CI green on all 3 OSes → step 01 ✅; Martin reviews step 02; then step 03

### 2026-11-25 19:40 · Step 01
- Did: repo layout + `git init`; workspace, crates, config, deny, CI workflow, placeholder icon, docs.
- Tests: 3 passed, 0 failed · fmt/clippy/deny clean · release binary 7.4 MB · CI: not run yet (no remote)
- Commits: initial commit (see git log)
- Next: create GitHub repo + push (awaiting OK), confirm CI green, mark step 01 ✅

---

## Known issues backlog
Issues found during the build that are not fixed yet.

| ID | Found in step | Severity | Description | Planned fix (step) | Status |
|---|---|---|---|---|---|
| I-001 | 01 | Low | CI doesn't run `cargo test` (Martin's request); egui_kittest harness not proven in CI | 12 | Resolved (step 12: `cargo test --workspace --no-fail-fast` on all 3 OSes; green in run 46114991172 after fixing 3 test races and 2 platform assumptions) |
| I-002 | 02 | Low | CI doesn't generate the 100 MB fixture set (testing-strategy says it should); add with the test step | 12 | Resolved (step 12: Linux job `fixtures + benches (report)`, green) |
| I-003 | 04 | Low | Index build is limited by the sequential token consumer (~0.9 s of the 1.18 s for 2 GB). A per-chunk grammar check + parallel stitch could approach memory bandwidth. Targets are met, so only if later profiling needs it | 12 | Open |
| I-004 | 04 | Low | x86 speed only measured under Rosetta; measure `fast_index_2g` on a real Windows x64 machine (reference machine B) | 12 | Open |
| I-005 | 05 | Low | `index_100m/scalar_index` (the reference oracle only) runs at 139–147 ms or 270–300 ms for identical source depending on the build path: code-alignment sensitivity of its byte loop. Production builders are unaffected. Possible fix: `-C llvm-args=-align-loops=64` for benches, or restructure the loop if it ever matters | 12 | Open |
| I-007 | 06 | Low | macOS "Open With" / double-click: winit 0.30 doesn't forward `application:openURLs:`; needs an objc2 app delegate (unsafe) and a bundled `.app` (ADR 017) | 12 | Open |
| I-008 | 06 | Low | Status-bar memory shows the resident set, which includes mapped file and cache pages (2.24 GB right after building a 2 GB index). Show heap/private memory instead to match the "app memory excluding the mapped file" target | 12 | Resolved (step 12: heap measured with the `alloc-stats` build, ADR 027; normal builds keep the resident set with a tooltip that explains it) |
| I-009 | 07 | Low | Adaptive mode (e.g. `orders_min_2g`, depth ≤ 1 stored): reading a record whose children include a big unstored container finds its end with a SIMD scan on the UI thread, and counts of unstored containers > 64 KiB show `{…}` until expanded. Fine for typical records; a record holding a 100 MB+ nested array would stall a frame. Fix: do that scan in the prepare job, or store such containers in the index | 12 | Open |
| I-010 | 07 | Low | Rare app test flakes: in ~50 `cargo test` runs, once `open_shows_progress_then_done_and_cancel_works` hit the 20 s `step_until` timeout (which wait wasn't captured) and once `settings_round_trip_through_the_window` didn't find "Cache indexes on disk" (under an artificial all-cores load). Not reproduced in 40 further loaded runs of `shell` + `tree`. Check again when CI runs tests | 12 | Open |
| I-011 | 08 | Low | `index_build::cancel_returns_quickly` flakes (≈ 1 in 4 runs of the whole binary: 57–60 ms vs the 50 ms limit) because the binary's 10,000-case proptests load every core at the same time; passes alone. Reproduced on the step 07 commit. Fix: run it in its own test binary or under a lock with the proptests | 12 | Resolved (step 12: own binary `tests/index_cancel.rs`; 3/3 green while 3 fuzzers ran) |
| I-012 | 08 | Low | Revealing a value tens of thousands of levels deep (raw click / Go to in `deep_100k`, depth 56,250) costs one 20–60 ms frame: `TreeModel::reveal` expands level by level on the UI thread, O(depth × segments). Fix: insert the whole chain of segments in one pass, or reveal on a worker | 12 | Open |
| I-013 | 08 | Low | Tree and raw panes can't be hidden yet (architecture §7: "either can be hidden"); only the split is resizable. Add View toggles (toolbar + shortcuts) and persist them | 12 | Resolved (step 12: Tree / Raw toggles in the toolbar, Ctrl/Cmd+1 / Ctrl/Cmd+2, never both hidden, saved in the settings; kittest `tests/panes.rs`, checked in the app) |
| I-015 | 10 | Low | Search shortcuts (Ctrl+F, Ctrl+Shift+F, F3/Shift+F3, Enter/Shift+Enter, Esc) checked on macOS and in kittests with egui told it runs on Windows, but not on a real Windows machine | 12 | Open |
| I-014 | 09 | Medium | Search workers run on the global rayon pool, which the index build also uses. A search started while a 2 GB index builds may wait for pool threads (up to the build time) before its first result; seen in a test where one search's workers queued ~40 ms behind another's. Fix in step 10: a dedicated search pool (or a few dedicated threads), measured with "first result < 200 ms" during indexing | 10 | Resolved (step 10: own search pool; first result 17–35 ms during a 2 GB build) |
| I-016 | 11 | Low | Copy path (button, menu, Ctrl/Cmd+Shift+C) of a value tens of thousands of levels deep builds the path on the UI thread, O(depth) row reads: one ~17 ms frame at depth 56,250 (`deep_100k`). Fix: build the path from the address on a worker, together with I-012 | 12 | Open |
| I-017 | 12 | Low | After closing a tab, the heap returns to baseline but the resident set can stay higher for a while (466–960 MB seen once right after closing a freshly built 2 GB tab) | 12 | Won't fix (step 12, investigated): with the index cache off, RSS is back to 20–60 MB 1 s after every close; with it on, the background cache write keeps the index until the entry is written, and RSS is < 160 MB 8 s after most closes. `mi_collect(true)` on every pool thread after a close (tried with Martin's OK) didn't help (some cycles kept 240–360 MB), so it was not committed; no `unsafe` added. Repeated open/close doesn't grow memory |
| I-018 | 12 | Low | A file with an early error still gets fully scanned before the error shows: 5 GiB sparse file (`[]` + spaces + zeros, error at byte 8,194) takes 6 s and touches 3.8 GB of zero pages | 12 | Resolved (step 12: files > 4 MiB get a sequential look at their first 1 MiB; an error there finishes the index from that parser and only the line table scans the file: 5 GiB sparse 6.0 → 2.1 s; equal to the reference in `early_error_path_equals_reference`) |
| I-006 | 05 | Medium | Index cache behaviour on Windows (replace/delete of an entry mapped by another instance) is handled in code but untested; run `tests/cache.rs` on Windows once CI runs tests | 12 | Resolved (step 12: `tests/cache.rs` passes on the Windows runner) |
