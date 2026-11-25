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
