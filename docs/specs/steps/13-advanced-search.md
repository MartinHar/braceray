# Step 13 — Advanced search: regex, keys-only / values-only

**Phase:** v1 · **Depends on:** 09, 10

## Goal
Power-user search: regular expressions and restricting matches to object keys or to values, with the same speed, streaming and navigation as plain search.

## Regex
- Engine: `regex::bytes::RegexBuilder` with `size_limit` and `dfa_size_limit` set (e.g. 50 MB) so a pathological pattern fails with a message instead of using unbounded memory. Unicode mode on; case-insensitive via the toggle.
- **Chunk-boundary correctness:** regex matches can span any length, so a fixed overlap doesn't work. Rule: **a match may not span a newline or exceed `max_regex_match_len` (default 64 KiB)**. Each chunk searches `[start, end + 64 KiB)` and keeps matches starting in `[start, end)`. Document this limit in the UI tooltip. Leftmost-first, non-overlapping semantics as in step 09: the chunk at a boundary must continue from the previous chunk's last kept match end, so resolve boundary conflicts in the ordered-publish phase.
- Invalid pattern → inline error under the search box ("Unclosed group at position 7"); no search runs.
- Whole word + regex = wrap in `\b(?:…)\b`.
- Highlight the exact match spans as before.

## Scope: Keys / Values / All
- For each candidate match, classify the token that **contains the match start**:
  1. Find the innermost known-outside-string position before the match (the container start from `node_at_offset`, or a child-offset table entry).
  2. Scan forward with the SIMD classifier to the match to learn whether it is inside a string and where that string starts and ends.
  3. A string followed (after whitespace) by `:` is a **key**. Any other string, number or literal is a **value**. Matches on structural characters belong to neither (excluded unless All).
- Optimisations:
  - **Keys** scope with a literal query: search for the needle and verify. Classification cost is only paid per candidate.
  - Batch classification: candidates sorted by offset within a chunk share one forward scan per container.
- A match spanning a token boundary (e.g. `": "` across key and value) only counts in All.

## UI
Enable the `.*` toggle and the `All | Keys | Values` segmented control from step 10. Persist the last-used options (setting: "Remember search options").

## Acceptance criteria
- [ ] Regex counts equal `Regex::find_iter().count()` on fixtures ≤ 100 MB (with matches that don't cross newlines), with chunk sizes down to 64 B.
- [ ] Keys / Values counts equal a serde-tree walk count (proptest + golden fixtures), including keys containing `":"` and escaped quotes.
- [ ] 2 GB warm: simple regex (`\b[A-Z]{3}-\d{4}\b`) < 5 s; Keys-scope literal < 3 s.
- [ ] A catastrophic pattern (`(a+)+$` style) can't hang the app: the size limit or cancel works within 100 ms.
- [ ] Invalid regex shows an inline error.

## Tests
Oracle tests as above, a boundary matrix, fuzz extension of `search_any` with regex and scope flags, kittest for the toggles and error message.
