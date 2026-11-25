# Step 04 — Structural index & validation

**Phase:** MVP · **Depends on:** 03 · **Size:** large. Do it in the four sub-steps below, one commit (or session) each.

## Goal
Build the structural index described in `architecture.md` §3 at disk speed on all cores, validate the JSON and report the exact error location. This is the most important step for speed, so correctness and benchmarks come first.

---

## 4a — Scalar reference indexer (correctness first)
A simple, single-threaded, byte-by-byte state machine that produces the **same output** as the fast indexer will:
- `Vec<Node>` (architecture.md §3.3), line table (every 64 KiB), and `Result<(), JsonError>`
- full JSON validation per RFC 8259: literals `true/false/null`, number grammar, string escapes (`\" \\ \/ \b \f \n \r \t \uXXXX`), no raw control chars `< 0x20` in strings, commas and colons, a single top-level value, trailing whitespace only
- an explicit stack (no recursion)
- `JsonError { offset, line, column, expected: &'static str, found: Option<u8> }`, with column counted in **characters** (UTF-8 aware, computed only for the error line)

This is the **oracle** for 4b/4c and the fallback on CPUs without SIMD. Test it against `serde_json` with proptest (see testing-strategy.md).

## 4b — SIMD classifier (single chunk)
`index/simd.rs`, operating on 64-byte blocks:
- produce `u64` bitmasks: quote, backslash, structural (`{}[]:,`), whitespace, newline
- odd-backslash-sequence escape mask (simdjson algorithm)
- in-string mask = prefix XOR of unescaped quotes:
  - x86_64: `pclmulqdq` when `is_x86_feature_detected!("pclmul")`, AVX2 / SSE4.2 for byte compares
  - aarch64: NEON byte compares plus the shift-XOR ladder for prefix XOR (`x ^= x<<1; x ^= x<<2; … x<<32`)
  - portable scalar fallback for both
- structural positions outside strings → iterate set bits (`trailing_zeros`) to emit events
- Dispatch once at startup and store a function pointer.

Equivalence test: for random inputs and every implementation (scalar fallback, SSE, AVX2, NEON as available), masks are identical.

## 4c — Parallel build (chunks + stitch)
Implement phases 1–4 from architecture.md §3.2:
1. Chunks of `chunk_size` (default 8 MiB, **parameter** for tests) processed with `rayon::par_iter`.
2. A sequential prefix pass over chunk summaries fixes the in-string state and escape carry.
3. Per-chunk event emission into a per-chunk `Vec<Event>` (Open/Close offsets) + depth info + newline counts + light validation of the token sequence between structurals (literal/number/string checks run on the spans between structurals, in parallel).
4. Sequential stitch with a stack → node table in document order, `parent`, `depth`, `end`, `child_count` (count commas at the container's depth plus 1, or 0 when empty; collect in phase 3 as per-chunk partial counts keyed by depth).
5. Errors: each chunk reports its first local error; the stitch keeps the earliest global error. Structural errors (mismatched `}` vs `]`, unclosed containers at EOF, extra top-level value) are found in the stitch.
6. **Adaptive mode** (architecture.md §3.3): estimate the node count from phase-3 event counts. If the node table would exceed `memory_budget` (default 25 % of file size), keep only containers with depth ≤ D.
7. Progress: phase 1 + 3 report bytes done. Cancellation is checked between blocks of 1 MiB.

API:
```rust
pub struct IndexOptions { pub chunk_size: usize, pub memory_budget: u64, pub threads: Option<usize> }
pub fn build_index(doc: &Document, opts: &IndexOptions, cancel: &CancelToken, progress: &Progress)
    -> Result<Index, Error>;   // Index also contains Validation = Ok | Invalid(JsonError) (partial index kept)
```

## 4d — Navigation API & benchmarks
- Implement `nav.rs` from architecture.md §3.4: `root`, `children(range)`, `child_count`, `parent`, `node_at_offset`, `path_of`, `line_col`.
- Child iteration inside a container: SIMD scan from `node.start + 1` (known outside string) to `node.end`, yielding `Child { key: Option<Span>, value: Span, kind: Kind }`. Cache child offset tables in an LRU bounded by a byte budget (default 256 MB). For containers with > 10 000 children, build the element checkpoint table (every 1024th child).
- Criterion benches on the 100 MB fixture: `scalar_index`, `simd_index_1thread`, `simd_index_parallel`, `children_of_1m_array`. Record numbers in `docs/PERF.md`.
- A big-file harness (`#[ignore]` test or `tools/fixtures bench-index`) on 2 GB: print GB/s, total time, node count, index bytes.

## Acceptance criteria
- [ ] For every golden fixture and 10,000 proptest cases: the SIMD/parallel index == the scalar reference index (node table, line table, validation result).
- [ ] Run with chunk sizes 64, 65, 127, 4096 and 8 MiB: identical output.
- [ ] Every `invalid_*` fixture reports the manifest's offset, line and column exactly.
- [ ] `deep_100k.json` indexes without stack overflow.
- [ ] 2 GB orders, warm: **< 5 s** total on reference machines (single-thread SIMD ≥ 1.5 GB/s; parallel limited by memory bandwidth).
- [ ] Index memory ≤ 25 % of file size on `orders_2g.json`. Adaptive mode is triggered and correct on a file of tiny objects (e.g. `[{"a":1},{"a":1},…]`).
- [ ] Cancelling mid-build returns `Error::Cancelled` within 50 ms.
- [ ] `node_at_offset` + `path_of` give the same path as a serde walk for 10,000 random offsets.

## Tests
Unit, proptest oracle, chunk-boundary matrix, fuzz target `index_any_bytes` (create the fuzz crate now), criterion benches.
