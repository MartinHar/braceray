# Braceray — Build progress

> **Claude Code maintains this file.** Read it at the start of every session and update it while you work (rules in `CLAUDE.md` → *Progress tracking*).
> Humans: this is the single place to see where the build stands.

## Legend
`⬜ Not started` · `🟦 In progress` · `✅ Done` · `⛔ Blocked` · `⚠️ Done with known issues`

---

## Current status
| Field | Value |
|---|---|
| **Current step** | 01 — Project setup & CI |
| **Status** | 🟦 In progress |
| **Current sub-task** | Push to GitHub and get CI green |
| **Next action** | Create public repo `MartinHar/braceray` (needs Martin's OK), push, check CI on 3 OSes, then mark step 01 ✅ |
| **Last updated** | 2026-11-25 |
| **Last session by** | Claude Code |
| **Latest version** | — (no release yet) |
| **CI on `main`** | — |

## Blockers / questions for Martin
- [x] 2026-11-25 · Step 01 · GitHub repo `MartinHar/braceray` doesn't exist yet · Resolved: Martin approved a public repo; asked that CI not run tests until near the end
<!-- Format: - [ ] YYYY-MM-DD · Step NN · question or blocker · what you need from me -->

---

## Phase overview
| # | Step | Phase | Status | Started | Finished | Commit / PR |
|---|---|---|---|---|---|---|
| 01 | Project setup & CI | MVP | 🟦 | 2026-11-25 | | |
| 02 | Test fixture generator | MVP | ⬜ | | | |
| 03 | File loading (mmap) | MVP | ⬜ | | | |
| 04 | Structural index & validation | MVP | ⬜ | | | |
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

**MVP:** 0 / 12 · **v1:** 0 / 5 · **v2:** 0 / 1

---

## Performance targets tracker
Fill in with real measurements (details and machine info in `docs/PERF.md`). Mark ✅ met / ❌ missed / — not measured yet.

| Target (from overview.md) | Goal | Measured | Machine | Step | Met? |
|---|---|---|---|---|---|
| First visible content (2 GB) | < 1 s | | | 07 | — |
| Full index, warm (2 GB) | < 5 s | | | 04 | — |
| Full index, cold NVMe (2 GB) | < 10 s | | | 04 | — |
| Re-open from cache (2 GB) | < 1 s | | | 05 | — |
| Plain search count, warm (2 GB) | < 1.5 s | | | 09 | — |
| Case-insensitive search (2 GB) | < 3 s | | | 09 | — |
| Simple regex (2 GB) | < 5 s | | | 13 | — |
| First search result | < 200 ms | | | 10 | — |
| `$..price` JSONPath (2 GB) | < 5 s | | | 14 | — |
| Expand 1M-child node | < 200 ms | | | 07 | — |
| Scrolling p99 frame time | < 16 ms | | | 07/08/10 | — |
| Index memory (% of file) | ≤ 25 % | | | 04 | — |
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
**Status:** 🟦 In progress · **Started:** 2026-11-25 · **Finished:** —

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
- [ ] Create GitHub repo, push, CI green (needs Martin's OK)

**Acceptance criteria** (copied from the step file)
- [x] `cargo run -p braceray-app` opens a window titled "Braceray".
- [x] `cargo build --release` succeeds; the release binary is < 15 MB.
- [x] All four quality gates pass locally.
- [ ] CI is green on all three OSes.
- [x] `cargo deny check` passes (fonts bundled by egui are allowed explicitly with a comment).
- [ ] `docs/PROGRESS.md` exists (from the template), step 01 is ✅ with its section, quality gates line and a session log entry filled in, and *Current status* points to step 02.

**Quality gates (last run)**
- [x] fmt · [x] clippy · [x] tests (3 passed: 1 core unit, 1 core doctest, 1 egui_kittest) · [x] cargo deny · benches n/a — run 2026-11-25 on macOS (Apple Silicon), Rust 1.100.0

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
