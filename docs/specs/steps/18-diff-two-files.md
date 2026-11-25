# Step 18 — Diff two big JSON files · release v2.0.0

**Phase:** v2 · **Depends on:** 04, 05, 07, 08, 16

## Goal
Compare two huge JSON (or NDJSON) files **structurally**: show what was added, removed and changed, navigate between differences, all without parsing either file into memory. Example: two 1 GB API dumps compared in < 30 s.

## Semantics
- Whitespace-insensitive always. Numbers compared by their text after normalisation (`1.0` vs `1` = different by default; option "Numeric equality" makes them equal).
- **Objects:** match children by key. Option **"Ignore key order"** (default on); if off, a key-order change is reported as "reordered".
- **Arrays:** mode per diff (dialog), with a per-path override from the context menu:
  - **By index** (default): element i vs element i; length difference = added/removed tail.
  - **By key field:** match elements by a field (e.g. `id`). Suggest likely key fields automatically (fields present in > 95 % of sampled elements with unique values).
  - **LCS** (only for arrays ≤ 100,000 elements, otherwise disabled with a tooltip): longest common subsequence of element hashes for inserted/removed elements in the middle.
- NDJSON: records compared by line number or by key field.

## Engine (`braceray-core::diff`)
1. Index both files (cache helps).
2. **Subtree hashing:** compute a canonical hash (xxh3-64) for every container node bottom-up in parallel, ignoring whitespace (hash the token stream). With "Ignore key order", an object's hash combines child `(key_hash, value_hash)` pairs with an order-independent combiner (sum of mixed hashes). Store hashes in a parallel `Vec<u64>` (8 bytes per node). Primitive values are hashed on the fly when compared. Hash computation is a job with progress (both files in parallel).
3. **Top-down comparison:** equal hash → identical subtree, skip it (this is what makes it fast). Different → descend: match children per the semantics above and recurse with an **explicit stack**. Emit `DiffEntry { kind: Added | Removed | Changed | TypeChanged | Reordered, left: Option<Location>, right: Option<Location>, path }` in document order of the left file (then right-only additions).
4. Stream entries into a results store; exact counts per kind. Cap the stored entries (default 1,000,000; beyond that, the summary per subtree shows "N more changes").
5. Hash collisions: on "equal" for subtrees < 64 KiB, optionally verify bytes (setting "Paranoid mode"; default off; the probability is negligible with 64-bit xxh3 for this use).

## UI
- File → "Compare with…" (or drop a second file while holding Alt/Option).
- **Diff view:** two synced tree panes (left = A, right = B) with colour coding (green added, red removed, amber changed), collapsed by default to only the paths containing changes ("Show unchanged" toggle).
- Summary bar: `+1,204 added · −88 removed · ~3,411 changed` and a progress bar while running.
- Differences list (bottom panel, like search results): path, kind, short before → after preview; click to focus both panes; `F7` / `Shift+F7` next/prev difference.
- Raw side-by-side for the selected changed value (pretty, with inline highlighting of the changed characters for strings < 1 MB).
- Export diff: as JSON (list of entries with paths and old/new values) or as a JSON Patch (RFC 6902) file.

## Acceptance criteria
- [ ] `diff_a_1g.json` vs `diff_b_1g.json`: every change in `diff_manifest.json` is reported, and nothing else, in all three array modes where applicable.
- [ ] Identical files (same content, different whitespace) → "No differences" quickly (root hashes equal).
- [ ] 2 × 1 GB warm: **< 30 s**, UI responsive, cancellable.
- [ ] Applying the exported JSON Patch to A with a reference implementation (`json-patch` crate on small fixtures) yields B.
- [ ] Version **2.0.0** released via the tag workflow; PERF.md updated.

## Tests
Oracle: diff small random JSON pairs (proptest: generate A, apply random mutations → B; expected diff = the mutations) for each array mode; hash invariance tests (whitespace, key order); manifest test on the 100 MB diff fixture in CI; kittest for the diff view navigation.
