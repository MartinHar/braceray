# CLAUDE.md — Braceray project rules

You are building **Braceray**, a read-only desktop viewer for huge JSON files (2 GB+), for Windows and macOS.
Specs live in `docs/specs/`. Always read `docs/specs/overview.md` and `docs/specs/architecture.md` before working on a step.

## Stack (fixed — do not change without asking)
- Language: **Rust**, stable toolchain, edition 2024 (or the latest stable edition).
- UI: **egui / eframe** (latest stable). No webview, no Electron, no Tauri.
- Core crates: `memmap2`, `memchr`, `rayon`, `regex` (bytes API), `aho-corasick`, `sonic-rs` (only for small subtree parsing), `xxhash-rust` (xxh3), `mimalloc`, `rfd` (file dialogs), `arboard` (clipboard), `serde` + `serde_json` (settings only), `thiserror`, `anyhow` (app crate only), `tracing`.
- Test crates: `criterion`, `proptest`, `egui_kittest`, `cargo-fuzz` (fuzz/ dir), `insta` (optional snapshots).
- **App folders** (cache, settings): computed with std from `HOME` / `LOCALAPPDATA` / `XDG_*` (`IndexCache::default_dir`), not with `directories`: its dependency `option-ext` is MPL-2.0 (ADR 014, Martin's decision).
- **Versions:** when adding a dependency, use `cargo add <crate>` so you get the latest stable version. Never copy version numbers from memory.

## License rules
- Project license: **MIT** (`LICENSE`, copyright holder in `overview.md` → Project constants).
- Every dependency must be MIT-compatible. `cargo deny check licenses` must pass in CI.
- Allowed: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unlicense, BSL-1.0, Unicode-3.0, CC0-1.0, and the font licenses bundled by egui (OFL-1.1, UFL-1.0). **No GPL / LGPL / AGPL / SSPL.**
- Keep `THIRD_PARTY_LICENSES.md` current (generate it with `cargo about` or `cargo deny list`).

## Performance rules (non-negotiable)
1. **Never read the whole file into a `Vec`/`String`.** Files are memory-mapped (`memmap2`) and accessed as `&[u8]`.
2. **Never build a full DOM** of the file (no `serde_json::Value` for the whole document). Use the structural index (architecture.md §3).
3. **The UI thread never blocks.** Any work that may take > 4 ms runs on a worker (rayon pool or dedicated thread). The UI polls results each frame. Frame budget is 16 ms.
4. **Every long operation** (index, search, export, diff, JSONPath) reports progress (0–100 %) and is cancellable through a shared `CancelToken` (`Arc<AtomicBool>`) checked at least every few MB.
5. **Render only what is visible.** Tree, raw view and results list are virtualized; work is proportional to rows on screen, not to file size.
6. **No recursion over JSON depth.** Depth can be 100 000+. Use explicit stacks.
7. Hot loops work on bytes and use SIMD-backed crates (`memchr`, `memmem`). Avoid per-byte allocations, `format!` in loops, and UTF-8 validation of the whole file.
8. Release profile: `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `opt-level = 3`. Global allocator: `mimalloc`.
9. Don't use `-C target-cpu=native` for release builds (they must run on every machine). Rely on runtime CPU feature detection (`memchr` does this; for our own SIMD use `std::arch` + `is_x86_feature_detected!` / NEON on aarch64, with a scalar fallback).

## Code rules
- Workspace layout per `architecture.md` §2. `braceray-core` has **no UI dependencies** and is fully unit-testable.
- `unsafe` is allowed only in `braceray-core::{mmap, index::simd, search}`, plus `braceray-app::alloc_stats` behind the off-by-default `alloc-stats` measurement feature (ADR 027, Martin's OK 2026-11-28). Every `unsafe` block has a `// SAFETY:` comment and a test that covers it.
- Errors: `thiserror` enums in core, `anyhow` in the app. No `unwrap()`/`expect()` in non-test code except for true invariants (comment why).
- No panics on bad input. Invalid JSON, truncated files, binary garbage, empty files and UTF-16 files must give a clear message, never a crash.
- Public core APIs get rustdoc comments with an example.
- Keep files under ~500 lines; split modules early.

## Quality gates — run before every commit
```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```
For steps touching index/search/export/diff also run:
```
cargo bench -p braceray-core -- --quick
```
and compare with the numbers in `docs/PERF.md`. A slowdown > 10 % must be explained or fixed.

## Workflow for each step
1. Read `docs/PROGRESS.md` first, then the step file fully. Write a short plan (files, types, tests) before coding.
2. Implement in small commits. Conventional commit messages (`feat:`, `fix:`, `perf:`, `test:`, `docs:`, `ci:`).
3. Write the tests listed in the step **plus** any you find necessary.
4. Run all quality gates. Fix everything; never disable a lint or test to make CI green.
5. Update `docs/PROGRESS.md` (see *Progress tracking* below).
6. Update `CHANGELOG.md` (Keep a Changelog format) under `[Unreleased]`.
7. Stop and summarise. Don't start the next step on your own.

## Progress tracking — `docs/PROGRESS.md` (mandatory)
`docs/PROGRESS.md` is the single source of truth for build status. Keep it accurate so any session (or any person) can pick up exactly where the last one stopped.

**At session start**
- Read `docs/PROGRESS.md`. If a step is 🟦 In progress, **resume it** from its unticked plan items instead of starting over.
- Check *Blockers / questions*. If an open blocker affects this step, ask Martin before continuing.

**When a step starts**
- Set *Current status* (step, 🟦 In progress, next action, date).
- Set the step's row in *Phase overview* to 🟦 with the start date.
- Add a *Step NN* section from the template: write the **plan** as checkboxes and **copy the acceptance criteria** from the step file.

**During the step** (after each sub-task or commit, not only at the end)
- Tick finished plan items and acceptance criteria.
- Update *Current sub-task* and *Next action*.
- Write any open question or blocker to *Blockers / questions* (status ⛔ if work can't continue).
- Commit `PROGRESS.md` together with the code it describes.

**When a step finishes**
- Mark it ✅ only if **all** acceptance criteria are ticked and all quality gates pass. If something is consciously deferred, use ⚠️ and list it under *Known limitations* and in the *Known issues backlog* with a planned step.
- Fill in the *Quality gates (last run)* line with real numbers (tests passed, etc.).
- Update the *Phase overview* row (finish date, commit hash) and the MVP / v1 / v2 counters.
- Record any measured numbers in the *Performance targets tracker* (and the details in `docs/PERF.md`).
- Point *Current status* at the next step (⬜, "Next action: start step NN+1").

**At session end (always, even if interrupted mid-step)**
- Add a *Session log* entry at the top: what was done, test results, commits, next action.

**Never**
- Mark a criterion done that wasn't verified. Unsure → leave it unticked and add a note.
- Delete history from the session log or the issues backlog (mark items resolved instead).

## UI testing with egui_mcp
In debug builds the app enables egui's inspection plugin (see step 06). When a step changes the UI, launch the app and use the `egui` MCP server to click through the acceptance criteria. Also write `egui_kittest` tests for anything that must stay working.

## When unsure
Ask instead of guessing on product questions. On technical choices inside a step, pick the simplest option that meets the performance targets, and note it in `docs/DECISIONS.md` (one short ADR-style entry).
