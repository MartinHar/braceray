# Braceray — Architecture

## 1. Big picture

```
            ┌──────────────────────────── braceray-app (egui) ────────────────────────────┐
            │  Toolbar · Breadcrumbs · Tree view · Raw view · Search bar · Results panel   │
            │  Settings · Bookmarks · Diff view          (UI thread: render + poll jobs)    │
            └───────────────▲───────────────────────────────────────────────┬──────────────┘
                            │ snapshots / progress                          │ commands
            ┌───────────────┴──────────────── braceray-core ────────────────▼──────────────┐
            │  Document ── mmap (&[u8]) ── StructuralIndex ── NodeRef / PathResolver        │
            │  Jobs: IndexJob · SearchJob · JsonPathJob · ExportJob · DiffJob              │
            │  IndexCache (disk)          CancelToken + Progress on every job               │
            └───────────────────────────────────────────────────────────────────────────────┘
                     rayon thread pool (all cores) — SIMD via memchr / std::arch
```

**Rule of thumb:** the file is never copied and never fully parsed. Everything works from
(a) the memory-mapped bytes and (b) a compact structural index.

## 2. Workspace layout

```
braceray/
├── Cargo.toml                  # [workspace], shared [profile.release]
├── CLAUDE.md
├── LICENSE                     # MIT
├── README.md  CHANGELOG.md  THIRD_PARTY_LICENSES.md
├── deny.toml                   # cargo-deny: licenses, advisories, bans
├── crates/
│   ├── braceray-core/          # NO UI deps. All logic. Library.
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── document.rs     # open, mmap, encoding check, file identity, change detection
│   │   │   ├── job.rs          # CancelToken, Progress, JobHandle<T>
│   │   │   ├── index/          # mod.rs, simd.rs (classifier), build.rs, node.rs, lines.rs, validate.rs, ndjson.rs
│   │   │   ├── cache.rs        # on-disk index cache
│   │   │   ├── nav.rs          # NodeRef, children iteration, offset → node, path building
│   │   │   ├── path.rs         # path formats: JSONPath / jq / JS
│   │   │   ├── search/         # plain.rs, regex.rs, scope.rs (keys/values), results.rs
│   │   │   ├── jsonpath/       # parser.rs, eval.rs
│   │   │   ├── export/         # json.rs, csv.rs
│   │   │   ├── diff/           # hash.rs, compare.rs
│   │   │   └── pretty.rs       # on-the-fly pretty printer for a byte range
│   │   └── benches/            # criterion: index, search, jsonpath
│   └── braceray-app/           # egui binary "braceray"
│       ├── src/
│       │   ├── main.rs         # eframe setup, mimalloc, CLI arg, windows_subsystem
│       │   ├── app.rs          # App state, job polling, layout
│       │   ├── settings.rs     # persisted settings
│       │   ├── theme.rs
│       │   └── ui/             # toolbar, tree, raw, search, results, breadcrumbs, bookmarks, diff, dialogs
│       ├── assets/             # icon.png, icon.icns, icon.ico
│       └── tests/              # egui_kittest UI tests
├── tools/
│   └── fixtures/               # binary "gen-fixtures": deterministic test-data generator
├── fuzz/                       # cargo-fuzz targets
├── packaging/
│   ├── macos/Info.plist
│   └── windows/                # resource script / icon embedding
├── .github/workflows/
│   ├── ci.yml                  # fmt, clippy, test, deny — Win, macOS, Linux
│   └── release.yml             # on tag v*: build, bundle, publish GitHub Release
└── docs/
    ├── specs/                  # these files
    ├── PROGRESS.md  PERF.md  DECISIONS.md
```

## 3. Structural index (the heart of speed)

### 3.1 Goals
- Build at memory/disk bandwidth using all cores.
- Small: **≤ 25 % of file size** for typical data.
- Answers fast: children of a node, parent of a node, node containing a byte offset, line/column of an offset.
- No recursion; arbitrary depth.

### 3.2 Build pipeline

**Phase 1: parallel classification (SIMD, per chunk).**
Split the mapped bytes into chunks (default 8 MiB, aligned to 64 bytes). For each chunk, in parallel, using 64-byte blocks (the simdjson technique):
- bitmasks for `"`, `\`, structural chars `{ } [ ] : ,`, and `\n`
- the escaped-character mask (odd-length backslash runs)
- the **in-string mask**: prefix-XOR of the unescaped-quote mask (carry-less multiply on x86 `pclmulqdq`, or the shift-XOR ladder on aarch64 / scalar fallback)
- computed **assuming the chunk starts outside a string with no pending escape**. Also record: quote parity at the end, trailing backslash run parity, newline count.

**Phase 2: fix chunk start state (sequential, tiny).**
A prefix pass over chunk summaries gives each chunk's true start state (in-string? escaped first byte?). If a chunk actually starts inside a string, its in-string mask is just inverted. An escape carry is fixed by re-classifying the first block only.

**Phase 3: emit events (parallel, per chunk).**
With correct masks, each chunk emits container events: `Open{offset, kind}` / `Close{offset}`. It also emits its relative depth delta, min depth and line-count checkpoints. Invalid-token checks run here (bad literals, control chars in strings, missing `:` or `,` are detected with a light per-chunk state machine over structural positions).

**Phase 4: stitch (sequential, O(#containers)).**
Match opens and closes with an explicit stack to build the **node table** in document order. Report the first structural error with its byte offset.

### 3.3 Data structures

```rust
/// One entry per container ({} or []). Stored in document order (pre-order).
#[repr(C)]
struct Node {
    start: u64,        // offset of '{' or '['
    end: u64,          // offset of matching '}' or ']'
    parent: u32,       // node id (u32::MAX for root)
    child_count: u32,  // direct children (all value types), filled lazily if unknown
    kind_depth: u32,   // bit 31: object/array, bits 0..30: depth
}                      // 28 bytes (+ padding → align as needed)
```
- **Primitive values are not nodes.** Children of a container are found by scanning its byte range at expand time (SIMD over a known-outside-string start). Results are cached in a per-node **child offset table** (LRU, memory-bounded).
- **Huge containers** (> 10 000 children) get a sparse **element checkpoint table**: the offset of every 1024th child, built on first expand. That makes `[0 … 999]` grouping and "go to element 5,000,000" O(1)-ish.
- **Line table:** every 64 KiB, `(byte offset, line number)`. Line/column of any offset = binary search + count newlines (memchr) in ≤ 64 KiB.
- **Adaptive mode:** if the projected node table would exceed the memory budget (25 % of file size, configurable), store only containers with depth ≤ D (D chosen automatically). Deeper containers are resolved lazily by scanning the parent's range. The UI never notices except as slightly slower expansion of very deep nodes.
- **Validation result:** `Ok` or `JsonError { offset, line, column, expected: &'static str, found: u8 }`. Indexing stops at the first error, and the valid prefix stays browsable.

### 3.4 Navigation API (`nav.rs`)
```rust
impl Index {
    fn root(&self) -> NodeRef;
    fn children(&self, n: NodeRef, range: Range<usize>) -> impl Iterator<Item = Child>; // key span, value span, kind
    fn child_count(&self, n: NodeRef) -> usize;
    fn parent(&self, n: NodeRef) -> Option<NodeRef>;
    fn node_at_offset(&self, off: u64) -> Location;   // innermost container + key/index + value span
    fn path_of(&self, loc: &Location) -> Vec<PathSegment>; // walk parents; key/index per level
    fn line_col(&self, off: u64) -> (u64, u64);
}
```
`node_at_offset` = binary search in the node table (sorted by `start`) for the innermost container with `start ≤ off ≤ end`, then a local scan inside it for the key/index.

## 4. Search design
- Input: query + options `{ case_sensitive, whole_word, regex, scope: All | Keys | Values }`.
- Haystack split into chunks (overlap = needle length − 1 for literals; for regex, chunk on structural boundaries such as `,` at a safe offset), processed with `rayon`.
- Engines:
  - literal, case-sensitive → `memchr::memmem::Finder` (built once, shared)
  - literal, ASCII case-insensitive → `aho-corasick` with `ascii_case_insensitive(true)`
  - non-ASCII case-insensitive or regex → `regex::bytes::Regex` (it uses literal prefilters internally)
- Whole word: post-filter matches on byte boundaries (`[A-Za-z0-9_]` and UTF-8 continuation bytes count as word chars).
- Scope Keys/Values: post-filter using a local in-string re-scan from the nearest node start (known outside-string point). A string followed by `:` is a key.
- **Results store:** count is always exact (`AtomicU64`). Offsets are stored in per-chunk sorted `Vec<u64>` merged in order, capped by `max_stored_results` (default 5,000,000). Beyond the cap, Next/Previous still works by searching on demand from the current offset.
- Streaming: chunks finish in any order, but the UI shows results in document order. It publishes the contiguous finished prefix, so the first results appear in milliseconds.
- Paths and previews are computed **only for visible rows** of the results panel (LRU cache).

## 5. Jobs and threading
```rust
pub struct CancelToken(Arc<AtomicBool>);
pub struct Progress { done: AtomicU64, total: u64 }   // bytes processed
pub struct JobHandle<T> { cancel: CancelToken, progress: Arc<Progress>, result: Receiver<JobEvent<T>> }
enum JobEvent<T> { Partial(T), Done(T), Failed(Error), Cancelled }
```
- One rayon global pool (all cores). The UI thread only renders and polls `try_recv`.
- Starting a new search cancels the previous one. Closing a file cancels all of its jobs.
- After posting events, jobs call `ctx.request_repaint()` (the app passes a repaint callback into core as a `Box<dyn Fn() + Send + Sync>` so core stays UI-free).

## 6. Index cache
- Location: OS cache dir via `directories` (`…/Caches/braceray/index/` on macOS, `%LOCALAPPDATA%\braceray\cache\index\` on Windows). Never next to the user's file.
- Key: xxh3 of (canonical path, size, mtime, xxh3 of the first and last 1 MiB). Header has a magic number, format version and app version; any mismatch means rebuild.
- Format: little-endian, header + raw `Node` table + line table, written to a temp file then atomically renamed. Load = mmap the cache file and zero-copy cast after validating size and alignment (or `bytemuck`).
- Settings: **enabled by default**, max total size (default 4 GB), "Clear cache" button. LRU eviction by last access.

## 7. UI architecture (egui)
- `App` holds `Vec<OpenDocument>` (tabs) and a selected tab. Each `OpenDocument` has: `Arc<Document>`, index state (building / ready / error), tree state (expanded set, selection), raw view scroll anchor, search state, bookmarks.
- Layout: top toolbar → breadcrumb bar → central split (tree | raw, resizable, either can be hidden) → bottom results panel (collapsible) → status bar (file size, index progress, line:col, match count, memory).
- All lists use `ScrollArea::show_rows` (fixed row height) so cost depends on visible rows only.
- Keyboard: Ctrl/Cmd+O open, Ctrl/Cmd+F search, F3 / Shift+F3 next/prev, Enter / Shift+Enter in search box, Ctrl/Cmd+L go to line, Ctrl/Cmd+Shift+C copy path, Ctrl/Cmd+B bookmark, Ctrl/Cmd+W close tab, ←/→ collapse/expand, ↑/↓ move.
- Debug builds enable egui's inspection plugin so Claude Code can drive the app through `egui_mcp`.

## 8. Error model
`braceray_core::Error` (thiserror): `Io`, `Mmap`, `Encoding(Utf16Le | Utf16Be | Utf32 | Binary)`, `Json(JsonError)`, `Cancelled`, `FileChanged`, `Cache(…)`, `JsonPath(ParseError)`, `Export(…)`.
Each error has a user-facing message plus an optional "details" text for a copy-to-clipboard button.

## 9. File-change safety
Memory-mapped files can change under us. Store (size, mtime) at open time and re-check on window focus and every 5 s. If something changed, show a banner: "File changed on disk. Reload?". On Unix, truncation can cause SIGBUS, so all mmap reads go through `Document` and we re-validate the length before long jobs; a SIGBUS guard turns reads past a truncated end into zero pages and marks the document as changed (ADR 028). On Windows the mapping prevents deletion; that's acceptable and documented.
