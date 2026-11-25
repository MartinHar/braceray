# Braceray — Build progress

> **Claude Code maintains this file.** Read it at the start of every session and update it while you work (rules in `CLAUDE.md` → *Progress tracking*).
> Humans: this is the single place to see where the build stands.

## Legend
`⬜ Not started` · `🟦 In progress` · `✅ Done` · `⛔ Blocked` · `⚠️ Done with known issues`

---

## Current status
| Field | Value |
|---|---|
| **Current step** | 04 — Structural index & validation |
| **Status** | 🟦 In progress |
| **Current sub-task** | 4d — navigation API, benches, fuzz |
| **Next action** | Implement `nav.rs` (children, node_at_offset, path_of, LRU, checkpoints), criterion benches, fuzz crate |
| **Last updated** | 2026-11-25 |
| **Last session by** | Claude Code |
| **Latest version** | — (no release yet) |
| **CI on `main`** | Run 45650452101 (commit a3a1b0a): Windows ✅ macOS ✅ Linux ✅ deny ✅ |

## Blockers / questions for Martin
- [x] 2026-11-25 · Step 01 · GitHub repo `MartinHar/braceray` doesn't exist yet · Resolved: Martin approved a public repo; asked that CI not run tests until near the end
<!-- Format: - [ ] YYYY-MM-DD · Step NN · question or blocker · what you need from me -->

---

## Phase overview
| # | Step | Phase | Status | Started | Finished | Commit / PR |
|---|---|---|---|---|---|---|
| 01 | Project setup & CI | MVP | ✅ | 2026-11-25 | 2026-11-25 | a3a1b0a (CI run 45650452101) |
| 02 | Test fixture generator | MVP | ✅ | 2026-11-25 | 2026-11-25 | (see git log) |
| 03 | File loading (mmap) | MVP | ✅ | 2026-11-25 | 2026-11-25 | (see git log) |
| 04 | Structural index & validation | MVP | 🟦 | 2026-11-25 | | |
| 05 | Index cache | MVP | ⬜ | | | |
| 06 | App shell | MVP | ⬜ | | | |
| 07 | Tree view | MVP | ⬜ | | | |
| 08 | Raw text view | MVP | ⬜ | | | |
| 09 | Search engine (core) | MVP | ⬜ | | | |
| 10 | Search UI | MVP | ⬜ | | | |
| 11 | Breadcrumbs & copy | MVP | ⬜ | | | |
| 12 | MVP hardening & release v0.1.0 | MVP | ⬜ | | | |
| 13 | Advanced search (regex, keys/values) | v1 | ⬜ | | | |
| 14 | JSONPath queries | v1 | ⬜ | | | |
| 15 | Bookmarks | v1 | ⬜ | | | |
| 16 | NDJSON / JSON Lines | v1 | ⬜ | | | |
| 17 | Export JSON / CSV · release v1.0.0 | v1 | ⬜ | | | |
| 18 | Diff two files · release v2.0.0 | v2 | ⬜ | | | |

**MVP:** 3 / 12 · **v1:** 0 / 5 · **v2:** 0 / 1

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
| First visible content (2 GB) | < 1 s | | | 07 | — |
| Full index, warm (2 GB) | < 5 s | 1.18 s (pretty) / 1.94 s (minified) | M3P | 04 | ✅ |
| Full index, cold NVMe (2 GB) | < 10 s | ~3.0 s / 3.7 s (page cache partly evicted, not a true reboot-cold) | M3P | 04 | — |
| Re-open from cache (2 GB) | < 1 s | | | 05 | — |
| Plain search count, warm (2 GB) | < 1.5 s | | | 09 | — |
| Case-insensitive search (2 GB) | < 3 s | | | 09 | — |
| Simple regex (2 GB) | < 5 s | | | 13 | — |
| First search result | < 200 ms | | | 10 | — |
| `$..price` JSONPath (2 GB) | < 5 s | | | 14 | — |
| Expand 1M-child node | < 200 ms | | | 07 | — |
| Scrolling p99 frame time | < 16 ms | | | 07/08/10 | — |
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
**Status:** 🟦 In progress · **Started:** 2026-11-25 · **Finished:** —

**Plan**
- [x] 4a: `job.rs` (`CancelToken`, `Progress`); `index/{mod,node,lines,scalar}.rs`: 24-byte `Node`, `Index`, `Validation`, `JsonError` (char column), line table, scalar reference indexer (explicit stack, full RFC 8259 validation, optional depth limit); oracle tests vs serde (proptest), invalid manifest, deep_100k
- [x] 4b: `index/simd/` classifier (scalar / SSE4.2 / AVX2 / NEON, pclmul or shift-XOR prefix XOR), runtime dispatch, equivalence tests
- [x] 4c: parallel build (chunks, prefix fix, per-chunk events + validation, stitch, adaptive mode, progress, cancel), `build_index` API; equivalence + chunk-size matrix
- [ ] 4d: `nav.rs`, child iteration + LRU + checkpoints; criterion benches; 2 GB harness; fuzz crate `index_any_bytes`
- [ ] Docs: PERF, DECISIONS, CHANGELOG, PROGRESS

**Acceptance criteria** (copied from the step file)
- [x] For every golden fixture and 10,000 proptest cases: the SIMD/parallel index == the scalar reference index (node table, line table, validation result). (`tests/index_build.rs`: 3 properties × 10,000 cases (generated, mutated, random bytes) × every classifier; 5 runs native + 1 under Rosetta with SSE2/AVX2)
- [x] Run with chunk sizes 64, 65, 127, 4096 and 8 MiB: identical output. (every equivalence check runs all five)
- [x] Every `invalid_*` fixture reports the manifest's offset, line and column exactly. (both builders; byte column checked via the offset, char column separately, ADR 009)
- [x] `deep_100k.json` indexes without stack overflow. (both builders, plus 1 M unclosed `[`)
- [x] 2 GB orders, warm: **< 5 s** total on reference machines (single-thread SIMD ≥ 1.5 GB/s; parallel limited by memory bandwidth). (1.18 s pretty / 1.94 s minified on M3 Pro; classifier 5.25 GB/s single-thread, tokenizer ≈ 1.5 GB/s; the parallel part is limited by the sequential consumer rather than memory bandwidth, I-003. Windows machine B not measured yet)
- [x] Index memory ≤ 25 % of file size on `orders_2g.json`. Adaptive mode is triggered and correct on a file of tiny objects (e.g. `[{"a":1},{"a":1},…]`). (21.9 %; `adaptive_mode_on_tiny_objects` equals the reference built with the same depth limit; minified 2 GB uses depth ≤ 1, 4.2 %)
- [x] Cancelling mid-build returns `Error::Cancelled` within 50 ms. (`cancel_returns_quickly`, 256 MB, cancel after 0/5/30 ms, passes in debug and release)
- [ ] `node_at_offset` + `path_of` give the same path as a serde walk for 10,000 random offsets.

**Quality gates (last run)**
- [x] fmt · [x] clippy (workspace native + x86_64-apple-darwin; core for x86_64-pc-windows-msvc) · [x] tests (95 passed, 4 ignored big-file tests: core 29 unit + 11 document + 10 index_scalar + 10 index_build + 6 doctests; gen-fixtures 14 + 9 + 4 + 1; app 1) · [x] cargo deny · benches n/a until 4d. Run 2026-11-25 after 4c.

**Progress notes**
- 4a done: scalar reference indexer. 2,000 generated docs per run match the generator's own node table exactly, and 2,000 mutated docs agree with serde on validity (8 release runs ≈ 32k cases). All 12 invalid fixtures hit the manifest offset/line/column. 2 GB orders: 0.38 GB/s, 21.9 % memory (baseline).
- 4b done: classifier with scalar / NEON / SSE2 / AVX2 (+pclmul) and runtime dispatch. All implementations equal on every byte value, 2,000 random blocks, 2,000 random multi-block inputs and an exhaustive backslash-run sweep. NEON scans 2 GB at 5.25 GB/s on one thread.
- 4c done: `build_index` / `IndexOptions`. Exactly equal to the reference (incl. errors) by construction: SIMD tokens + the shared `Parser` state machine, hand-off to the byte-level parser for anything doubtful (ADR 011). The proptest found one real bug on the way (`null0`: bytes after a hand-off had no token), fixed with `catch_up`. 2 GB warm 1.18 s.
- Decisions: ADR 008 (24-byte node), ADR 009 (validation semantics, char columns, partial index), ADR 010 (SSE2 instead of SSE4.2, dispatch, escape mask, Rosetta testing), ADR 011 (parallel builder design).

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

### 2026-11-25 23:50 · Step 03 / 04
- Did: pushed step 03; step 04a (scalar reference), 4b (SIMD classifier) and 4c (parallel builder) done, each in its own commit. 4d (navigation, benches, fuzz) is next.
- Tests: 95 passed, 0 failed, 4 ignored big-file tests (pass with `--ignored`) · fmt/clippy/deny clean · core also passes as x86_64 under Rosetta (SSE2/AVX2) · 2 GB warm index 1.18 s, 21.9 % memory
- Commits: eac9cbd (4a), 1c02d57 (4b), 4c commit (see git log)
- Next: step 4d

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
| I-001 | 01 | Low | CI doesn't run `cargo test` (Martin's request); egui_kittest harness not proven in CI | 12 | Open |
| I-002 | 02 | Low | CI doesn't generate the 100 MB fixture set (testing-strategy says it should); add with the test step | 12 | Open |
| I-003 | 04 | Low | Index build is limited by the sequential token consumer (~0.9 s of the 1.18 s for 2 GB). A per-chunk grammar check + parallel stitch could approach memory bandwidth. Targets are met, so only if later profiling needs it | 12 | Open |
| I-004 | 04 | Low | x86 speed only measured under Rosetta; measure `fast_index_2g` on a real Windows x64 machine (reference machine B) | 12 | Open |
