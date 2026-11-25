# Step 14 — JSONPath queries

**Phase:** v1 · **Depends on:** 04, 10, 11

## Goal
Query mode in the search bar: type a JSONPath like `$.users[*].email`, `$..price` or `$.items[?@.price > 10]` and get matching nodes in the same results panel, streamed, fast and cancellable. No DOM.

## Supported syntax (RFC 9535 subset)
| Feature | Example |
|---|---|
| Root | `$` |
| Child by name | `$.store.book`, `$['store']['book']`, `$["weird key"]` |
| Wildcard | `$.store.*`, `$.items[*]` |
| Index, negative index | `$.items[0]`, `$.items[-1]` |
| Slice | `$.items[0:10]`, `$.items[::2]`, `$.items[-5:]` |
| Union | `$.items[0,3,7]`, `$['a','b']` |
| Descendants | `$..price`, `$..[0]`, `$..*` |
| Filter | `$.items[?@.price > 10]`, `[?@.email]` (exists), `[?@.tags[0] == 'vip']`, `&&`, `||`, `!`, parentheses |
| Comparisons | `== != < <= > >=` on numbers, strings, `true/false/null` |
| Functions (optional) | `length(@.tags) > 2`, `match(@.email, '.*@example\\.com')`, `search(...)`, `count(...)` |

## Design
- **Parser** (`jsonpath/parser.rs`): a hand-written recursive-descent parser over the **query string** (recursion is fine here; queries are tiny) → AST. Good error messages with a caret position. Fuzz target `jsonpath_parse` (from testing-strategy.md).
- **Evaluator** (`jsonpath/eval.rs`) over the index, streaming:
  - A segment pipeline producing `Location`s in document order where possible.
  - `.name` on an object: scan its children for the key (compare the raw key bytes; decode only when the key contains `\`).
  - `[*]` and slices on arrays: use element checkpoints to jump straight to ranges; negative indices via `child_count`.
  - **Descendant fast path:** `$..name` = run the step-13 Keys-scope literal search for `"name"` (exact key, quotes included) over the bytes in parallel, then take the value after `:`. Much faster than walking the whole tree. For general `..` segments, walk the node table in parallel by top-level subtree ranges.
  - Filters: evaluate on each candidate element by reading only the needed child values. Numbers are parsed with a fast float parser (`lexical-core` or `fast-float2`); strings are compared after unescaping only when needed.
  - Parallelism: split work by large subtrees (e.g. root array element ranges) across rayon; merge in order.
  - Results: the same `ResultsStore` as search (offset of the matched value + len), so the results panel, navigation, highlighting and export all work unchanged. Count is exact.
- **UI:** the search bar gets a mode switch `Text | JSONPath` (auto-switches when the input starts with `$`). There's a syntax hint popover with examples, a parse error inline with a caret, and a history.

## Acceptance criteria
- [ ] Every example in the table works and matches a reference evaluation over `serde_json::Value` (use the RFC 9535 compliance test suite cases where possible: vendor the JSON cases into `tests/fixtures/jsonpath-cts/` with their license noted; it's MIT-compatible, verify).
- [ ] `$..price` on the 2 GB fixture, warm: **< 5 s**.
- [ ] `$.orders[*].customer.email` streams its first results in < 300 ms.
- [ ] Filter queries on 2 GB complete with progress and are cancellable.
- [ ] Invalid queries show a helpful message with the position.

## Tests
Parser unit tests, evaluator oracle tests (proptest: random JSON + random simple queries from a generator → compare with the reference implementation), compliance cases, fuzz, benches `jsonpath_descendant_100m`, `jsonpath_filter_100m`.
