# Braceray — build specs for Claude Code

> **Braceray** — X-ray vision for giant JSON.
> A free, open-source (MIT) desktop app for Windows and macOS that opens 2 GB+ JSON files instantly and searches them at disk speed.

This folder is the complete plan. You give it to Claude Code one step at a time; Claude Code builds the app.

---

## What's in this folder

| File | Purpose |
|---|---|
| `CLAUDE.md` | Permanent rules for Claude Code (stack, performance rules, quality gates, progress tracking). **Goes in the repo root.** |
| `PROGRESS.md` | Build tracker that Claude Code keeps updated: current step, checklists, perf numbers, session log, blockers. **Goes in `docs/`.** |
| `overview.md` | What we are building, features per phase, performance targets, project constants |
| `architecture.md` | Workspace layout, threading model, index design, search design, crate list |
| `testing-strategy.md` | How every layer is tested (oracle tests, fuzzing, UI tests, perf gates) |
| `steps/01…18-*.md` | The build steps, in order. Each has goal, implementation notes, acceptance criteria and tests |

## Build order

| # | Step | Phase | Ends with |
|---|---|---|---|
| 01 | Project setup & CI | MVP | Empty app builds on Win + Mac in CI |
| 02 | Test fixture generator | MVP | 10 MB → 3 GB test files on demand |
| 03 | File loading (mmap) | MVP | Files open without being copied to RAM |
| 04 | Structural index & validation | MVP | Parallel SIMD index, exact error line/column |
| 05 | Index cache (on by default) | MVP | Second open is instant |
| 06 | App shell | MVP | Window, theme, drag & drop, recent files, settings |
| 07 | Tree view (lazy) | MVP | Browse 2 GB at 60 fps |
| 08 | Raw text view | MVP | Highlighted text, pretty-prints minified JSON on the fly |
| 09 | Search engine (core) | MVP | Parallel search, exact count, streaming results |
| 10 | Search UI | MVP | Count, results list, F3 navigation, jump to node |
| 11 | Breadcrumbs & copy | MVP | Copy path (JSONPath / jq / JS), copy value / subtree |
| 12 | MVP hardening & release | MVP | **v0.1.0** on GitHub Releases |
| 13 | Advanced search | v1 | Regex, keys-only / values-only |
| 14 | JSONPath queries | v1 | `$.users[*].email`, `$..price`, filters |
| 15 | Bookmarks | v1 | Saved per file |
| 16 | NDJSON / JSON Lines | v1 | Record-per-line files |
| 17 | Export JSON / CSV | v1 | **v1.0.0** |
| 18 | Diff two big files | v2 | **v2.0.0** |

---

## How to start (one-time setup, all free)

1. **Install tools**
   - Rust (stable) via https://rustup.rs
   - Git
   - **macOS:** Xcode Command Line Tools: `xcode-select --install` (free, no Apple Developer account needed)
   - **Windows:** "Visual Studio Build Tools" with the *Desktop development with C++* workload (free; rustup offers it)
   - VS Code with the Claude Code extension (or the Claude Code CLI). Any account works: these files contain everything, and no other context is needed.
2. **Create the repo**: on GitHub create a **public** repo named `braceray` (public = unlimited free GitHub Actions minutes). Don't add a license there — step 01 creates it.
3. **Clone it** and copy these files in:
   ```
   braceray/
   ├── CLAUDE.md                 ← from this folder
   └── docs/
       ├── PROGRESS.md           ← from this folder (Claude Code updates it)
       └── specs/                ← everything else from this folder
       ├── README.md
       ├── overview.md
       ├── architecture.md
       ├── testing-strategy.md
       └── steps/01-…18-*.md
   ```
4. Open `docs/specs/overview.md` → section **Project constants** → fill in your GitHub username.
5. Open the `braceray` folder in VS Code and open the Claude Code panel (or run `claude` in the VS Code terminal at the repo root).

## Prompt to paste for each step

```
Read CLAUDE.md, docs/PROGRESS.md, docs/specs/overview.md and docs/specs/architecture.md,
then implement docs/specs/steps/NN-<name>.md.
Plan first, then implement. Keep docs/PROGRESS.md updated as you go (rules in CLAUDE.md).
Meet every acceptance criterion, run all quality gates, commit, and stop when the step is done.
```

Replace `NN-<name>` with the step file.

## Prompt to resume (new session, interrupted work, or "where are we?")

```
Read CLAUDE.md and docs/PROGRESS.md. Tell me in 3 lines where the build stands,
then continue the current step from its first unticked item.
```

Only want a status report? Open `docs/PROGRESS.md` yourself. The *Current status* table at the top always shows the current step, the next action and any blockers waiting for you.

## Tips

- **One step per session.** Run `/clear` between steps so each one starts with a clean context.
- **Review before the next step.** Read the diff, run `cargo run --release`, and try the feature yourself.
- **Too big?** If a step feels huge, ask Claude Code to split it into sub-steps and do them one by one.
- **Something broke?** Paste the error and say "fix this, add a regression test".
- **UI testing by Claude Code:** from step 06 on, Claude Code can drive the running app through the `egui_mcp` server (see step 06). Ask it to "open the 1 GB fixture and verify the tree scrolls smoothly".
- **Releases:** after step 12, `git tag v0.1.0 && git push --tags` builds and publishes the Windows and macOS downloads automatically.
