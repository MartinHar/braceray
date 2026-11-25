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
