# Step 09 — Search engine (core)

**Phase:** MVP · **Depends on:** 04

## Goal
`braceray-core::search`: a parallel search over the mapped bytes that gives an **exact match count**, streams matches in document order, supports case-sensitive / case-insensitive and whole-word, and is cancellable. Regex and key/value scope come in step 13, but design the API for them now.

## API
```rust
pub struct SearchQuery {
    pub text: String,
    pub case_sensitive: bool,   // default false
    pub whole_word: bool,       // default false
    pub regex: bool,            // step 13
    pub scope: Scope,           // All (MVP) | Keys | Values (step 13)
}
pub struct SearchOptions { pub chunk_size: usize, pub max_stored_results: usize /* default 5_000_000 */ }

pub fn start_search(doc: Arc<Document>, index: Arc<Index>, q: SearchQuery, o: SearchOptions,
                    repaint: RepaintFn) -> SearchHandle;

impl SearchHandle {
    pub fn count(&self) -> u64;               // live, exact when done
    pub fn is_done(&self) -> bool;
    pub fn progress(&self) -> f32;
    pub fn stored(&self) -> usize;            // contiguous, document-ordered results available now
    pub fn get(&self, i: usize) -> Option<Match>;          // Match { offset: u64, len: u32 }
    pub fn index_of_offset(&self, off: u64) -> Result<usize, usize>;  // binary search (for "current match")
    pub fn next_after(&self, off: u64) -> Option<Match>;   // works beyond the stored cap via an on-demand scan
    pub fn prev_before(&self, off: u64) -> Option<Match>;
    pub fn cancel(&self);
}
```

## Implementation notes (architecture.md §4)
- Engines: `memmem::Finder` (case-sensitive literal), `AhoCorasick` with `ascii_case_insensitive` (ASCII case-insensitive), `regex::bytes::Regex` with `(?i)` + `regex::escape` for non-ASCII case-insensitive. Pick automatically.
- Chunking: chunk size 8 MiB; each chunk searches `[start, end + needle_len - 1)` and only keeps matches **starting** in `[start, end)`, so there are no duplicates and nothing is missed.
- Whole word: check the byte before and after (`is_word_byte`: ASCII alnum, `_`, or ≥ 0x80).
- Overlapping matches: **non-overlapping**, leftmost-first (like editors). Document this; the count test uses the same semantics.
- Ordered streaming: chunk results land in a slot vector; a "published prefix" advances while consecutive chunks are complete. `stored()` = matches in the published prefix (≤ cap). `count` includes all completed chunks.
- Empty query → no search. Query longer than the file → 0.
- Memory: 5M stored results × 12 bytes ≈ 60 MB. Beyond the cap, `next_after` scans forward from the offset with the same engine (fast enough for interactive use).
- Starting a new search cancels the old one (the caller drops the handle → cancel on drop).
- Must work **before indexing finishes** (search only needs bytes). The paths for results need the index; the UI shows "path available after indexing".

## Acceptance criteria
- [ ] Count equals a naive count on all fixtures ≤ 100 MB and on proptest inputs, with chunk sizes 64, 65, 4096 and 8 MiB.
- [ ] Planted needles in `orders_2g.json` give exactly the counts in its manifest.
- [ ] 2 GB warm: case-sensitive count **< 1.5 s**; case-insensitive **< 3 s** (record in `docs/PERF.md`).
- [ ] First result available in < 200 ms for a common word.
- [ ] Cancel stops all workers within 50 ms.
- [ ] `next_after` / `prev_before` are correct across the stored cap (test with cap = 10).
- [ ] Unicode case-insensitive: `ЁЛКА` finds `ёлка`; `ԱՐԵՎ` finds `արև`.

## Tests
Unit + proptest oracle, chunk-boundary matrix, fuzz target `search_any`, criterion benches `search_cs_100m`, `search_ci_100m`.
