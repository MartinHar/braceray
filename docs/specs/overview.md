# Braceray — Product overview

## One-liner
**Braceray — X-ray vision for giant JSON.** Open multi-gigabyte JSON files instantly, search them at disk speed, and jump straight to every match.

## Project constants
| Constant | Value |
|---|---|
| App name | Braceray |
| Binary name | `braceray` (`Braceray.exe` on Windows, `Braceray.app` on macOS) |
| License | MIT |
| Copyright holder | Martin Harutyunyan |
| GitHub repo | `https://github.com/MartinHar/braceray` |
| macOS bundle id | `io.github.martinhar.braceray` |
| Website (free) | `https://martinhar.github.io/braceray` (optional, GitHub Pages) |
| Budget | **$0**. No Apple Developer account and no code-signing certificate |

## Problem
Editors (VS Code, Notepad++), browsers and most JSON viewers parse the whole file into memory. A 2 GB file needs 10–20 GB of RAM and freezes or crashes them. QA engineers, backend developers and data people regularly get huge API dumps, logs and exports, and they need to **find things** in them quickly.

## Target users
- QA engineers inspecting large test data, API responses and logs
- Backend and data engineers with exports, dumps and NDJSON logs
- Anyone who has had "file too large to open"

## Principles
1. **Speed first.** Every feature must respect the performance targets below.
2. **Read-only.** We never modify the source file. (Editing 2 GB files is out of scope.)
3. **Never freeze.** Everything long shows progress and can be cancelled.
4. **Native and light.** One small binary, no runtime, works offline, no telemetry.
5. **Keyboard-friendly.** Every main action has a shortcut.

## Platforms
| OS | Architectures | Notes |
|---|---|---|
| Windows 10/11 | x64 (ARM64 optional) | Portable `.zip`, unsigned (SmartScreen: *More info → Run anyway*) |
| macOS 11+ | Apple Silicon + Intel (universal) | `.dmg` with ad-hoc signed `.app`; first launch: *System Settings → Privacy & Security → Open Anyway*, or `xattr -dr com.apple.quarantine /Applications/Braceray.app` |
| Linux | x64 | Not a goal, but keep it compiling (CI builds it). Free bonus |

## Features by phase

### MVP (v0.1.0)
- Open JSON files of any size: file dialog, drag & drop, recent files, command line `braceray <file>`
- Memory-mapped loading; top level visible in < 1 s
- Parallel structural index with **validation**: "Invalid JSON at line 1,204,331, column 17 (offset 88,120,551): expected ',' or '}'"
- **Index cache**, **enabled by default** (settings: on/off, max cache size, clear cache)
- **Tree view**: lazy expand/collapse, huge arrays grouped (`[0 … 999]`), type badges, value previews, child counts
- **Raw view**: virtualized, syntax-highlighted, on-the-fly pretty-print for minified files, synced with the tree
- **Search**: plain text, case-sensitive toggle, whole-word toggle; exact **match count**; results panel with JSON path + preview; Next/Previous (F3 / Shift+F3 / Enter); click to jump to the node
- **Breadcrumb bar** for the selected node; **Copy path** as JSONPath, jq or JavaScript
- **Copy value / subtree** to the clipboard (with a size guard)
- Dark / light / system theme
- Releases for Windows and macOS via GitHub Actions

### v1 (v1.0.0)
- Search: **regex**, **keys-only / values-only**
- **JSONPath queries** (RFC 9535 subset): `$.users[*].email`, `$..price`, `$.items[?@.price > 10]`
- **Bookmarks** on nodes (saved per file)
- **NDJSON / JSON Lines** support
- **Export**: matching nodes → new JSON file; array of objects → **CSV**

### v2 (v2.0.0)
- **Diff two big JSON files**: structural, whitespace-insensitive, optional key-order-insensitive, arrays by index or by key field (e.g. `id`)

### Explicitly out of scope
Editing and saving JSON, cloud features, accounts, telemetry, auto-update servers, paid tiers.

## Performance targets

Reference machines: **(A)** Apple M1/M2, 16 GB RAM, internal SSD; **(B)** Windows x64 with 8 cores, 16 GB RAM, NVMe SSD.
"Warm" = file already in the OS page cache. "Cold" = first read after reboot.

| Operation | File | Target |
|---|---|---|
| Time to first visible content | any size | **< 1 s** |
| Full index build (warm) | 2 GB | **< 5 s** |
| Full index build (cold, NVMe) | 2 GB | **< 10 s** |
| Re-open with index cache | 2 GB | **< 1 s** |
| Plain search, exact count (warm) | 2 GB | **< 1.5 s** |
| Case-insensitive search (warm) | 2 GB | **< 3 s** |
| Simple regex search (warm) | 2 GB | **< 5 s** |
| First search result shown | any | **< 200 ms** |
| `$..price` JSONPath (warm) | 2 GB | **< 5 s** |
| Expand a node with 1,000,000 children | — | **< 200 ms** |
| Scrolling tree / raw / results | any | **60 fps** (frame < 16 ms) |
| Index memory | typical data | **≤ 25 % of file size** |
| App memory excluding the mapped file | — | **< 300 MB** + index |
| Diff two 1 GB files (warm) | 2 × 1 GB | **< 30 s** |

These targets are checked by the benchmark harness in step 12 and recorded in `docs/PERF.md`.

## Known limitations (documented, by design)
- Search works on the **raw bytes** of the file. A string written with escapes (`"café"`) does not match a search for `café`. (Possible later: a "decode escapes" option.)
- Read-only. If the file changes on disk while it's open, Braceray detects it and offers to reload.
- UTF-8 only (with or without BOM). UTF-16/32 files are detected and rejected with a clear message.
