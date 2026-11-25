# Step 01 — Project setup & CI

**Phase:** MVP · **Depends on:** nothing

## Goal
An empty but correctly structured Rust workspace that builds a "Hello Braceray" egui window on Windows, macOS and Linux, with all quality gates running in GitHub Actions.

## Tasks
1. Create the workspace exactly as in `architecture.md` §2. Only skeleton files for now:
   - `crates/braceray-core` (lib) with `lib.rs` exposing `pub fn version() -> &'static str`.
   - `crates/braceray-app` (bin, binary name `braceray`) showing an eframe window titled "Braceray" with the text "Drop a JSON file here" centred.
   - `tools/fixtures` (bin `gen-fixtures`) printing "todo" (filled in step 02).
2. Root `Cargo.toml`:
   - `[workspace]` with `resolver = "3"` (or the latest), shared `[workspace.package]` (edition, license = "MIT", repository, authors).
   - `[workspace.dependencies]` for shared crates; add them with `cargo add` (latest versions).
   - Release profile: `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = "symbols"`.
   - A `[profile.bench]` that inherits release but keeps debug symbols.
3. `braceray-app/src/main.rs`:
   - `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` (no console window on Windows release builds).
   - `#[global_allocator] static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;`
   - Window: min size 900×600, default 1400×900, app id "braceray".
4. Files: `LICENSE` (MIT, `Copyright (c) 2026 Martin Harutyunyan`), `README.md` (name, tagline, features planned, build instructions, "Install on macOS/Windows" placeholder section), `CHANGELOG.md`, `docs/PROGRESS.md` (**use the provided template at `docs/specs/PROGRESS.md`**: move it to `docs/PROGRESS.md` if it isn't there already, and don't redesign it), `docs/PERF.md` (empty table), `docs/DECISIONS.md`, `.gitignore` (target, fixtures output `/fixtures-data/`, `.DS_Store`), `rustfmt.toml` (defaults + `max_width = 100`), `rust-toolchain.toml` (channel = "stable", components rustfmt and clippy).
5. `deny.toml`: license allow-list from `CLAUDE.md`, deny unknown registries, warn on duplicate versions, fail on security advisories.
6. `.github/workflows/ci.yml`:
   - Triggers: push to `main`, pull requests.
   - Matrix: `windows-latest`, `macos-latest` (Apple Silicon), `ubuntu-latest`.
   - Steps: checkout → `dtolnay/rust-toolchain@stable` → `Swatinem/rust-cache` → `cargo fmt --check` (Linux only) → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace` → `cargo build --release -p braceray-app`.
   - Linux needs the egui system deps (`libxkbcommon-dev libgtk-3-dev` etc.; check the eframe README for the current list).
   - A separate `deny` job using `EmbarkStudios/cargo-deny-action`.
7. App icon placeholder: a simple generated `{ }` icon with a scan line (SVG source + PNG 1024×1024) in `crates/braceray-app/assets/`. The final icon comes in step 12.

## Acceptance criteria
- [ ] `cargo run -p braceray-app` opens a window titled "Braceray".
- [ ] `cargo build --release` succeeds; the release binary is < 15 MB.
- [ ] All four quality gates pass locally.
- [ ] CI is green on all three OSes.
- [ ] `cargo deny check` passes (fonts bundled by egui are allowed explicitly with a comment).
- [ ] `docs/PROGRESS.md` exists (from the template), step 01 is ✅ with its section, quality gates line and a session log entry filled in, and *Current status* points to step 02.

## Tests
- One core unit test (`version()` is not empty) and one `egui_kittest` test that the window shows "Drop a JSON file here". This proves the UI test harness works in CI.
