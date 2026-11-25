# Braceray — Testing strategy

Goal: prove that Braceray is **correct** (never shows wrong structure, wrong counts or wrong paths), **robust** (never crashes on any input) and **fast** (meets `overview.md` targets on every release).

## Test pyramid

| Layer | Tool | Where | Runs in CI |
|---|---|---|---|
| Unit tests | `cargo test` | `braceray-core/src/**` (`#[cfg(test)]`) | every push |
| Oracle / property tests | `proptest` + `serde_json` as oracle | `braceray-core/tests/` | every push |
| Golden fixtures | small hand-written JSON + expected output | `braceray-core/tests/fixtures/` | every push |
| Fuzzing | `cargo-fuzz` (libFuzzer) | `fuzz/` | nightly job (or manual), 10 min per target |
| UI tests | `egui_kittest` | `braceray-app/tests/` | every push |
| Exploratory UI | Claude Code via `egui_mcp` + manual | — | per UI step |
| Benchmarks | `criterion` (100 MB fixture) | `braceray-core/benches/` | every push (quick mode, report only) |
| Big-file perf | `gen-fixtures` + `bench-big` harness (2 GB / 3 GB) | `tools/fixtures` | before each release, recorded in `docs/PERF.md` |
| Cross-platform smoke | release artifacts | Windows + macOS VMs / your machines | before each release (checklist below) |

## Oracles (how we know the answer is right)
- **Structure:** for any generated JSON ≤ 1 MB, walking our index (keys, kinds, child counts, value spans) must equal walking `serde_json::Value` of the same text.
- **Validity:** our validator says valid ⇔ `serde_json::from_slice::<IgnoredAny>` says valid. On invalid input, our error offset must be ≤ serde's reported position and point at the same line.
- **Search count:** equals a naive `haystack.windows(n).filter(..)` count (case-sensitive), and `regex::bytes` `find_iter().count()` for regex. Scope Keys/Values equals the count from walking the serde tree.
- **Paths:** for every match, resolving the path we display with a reference JSONPath walk over `serde_json::Value` returns a value containing the match.
- **Chunk boundaries:** every test also runs with tiny chunk sizes (64 B, 65 B, 127 B, 4 KiB) so strings, escapes, multi-byte UTF-8 and matches cross chunk borders. Chunk size is a parameter in core for exactly this reason.

## Property-test generators (`proptest`)
- Arbitrary JSON trees (depth ≤ 20, mixed kinds), serialized minified, pretty, and with random whitespace
- Strings containing `\"`, `\\`, `\\\"`, `\u0000`–`\u001f` escapes, emoji, Armenian/Cyrillic text, and `{}[],:` inside strings
- Mutations for invalid JSON: delete / insert / replace one byte

## Fuzz targets
1. `index_any_bytes`: arbitrary bytes → build index → must not panic; validity must agree with serde.
2. `search_any`: arbitrary bytes + needle + options → no panic; count = naive count.
3. `jsonpath_parse`: arbitrary string → parser must not panic.
4. `pretty_range`: arbitrary valid JSON + random range → no panic; output re-parses.

## Fixture catalogue (`gen-fixtures`, deterministic seed)
| Name | Size | Shape |
|---|---|---|
| `orders_{10m,100m,1g,2g,3g}.json` | as named | top-level array of realistic order objects (ids, names, emails, nested items arrays, unicode, escapes, numbers with exponents, nulls, bools) |
| `orders_min_*.json` | same | minified, single line |
| `deep_100k.json` | small | nesting depth 100,000 |
| `wide_1m.json` | ~50 MB | one object with 1,000,000 keys |
| `prim_array_10m.json` | ~100 MB | array of 10,000,000 numbers |
| `huge_string.json` | 200 MB | one 200 MB string value |
| `logs_*.ndjson` | 100 MB / 1 GB | one record per line |
| `invalid_*.json` | small + 1 GB | truncated, trailing comma, missing colon, bad literal, control char, unclosed string, error at the very end of a 1 GB file. Expected error position stored in `invalid_manifest.json` |
| `bom_utf8.json`, `utf16le.json`, `empty.json`, `whitespace.json`, `binary.bin` | tiny | encoding edge cases |
| `diff_a_1g.json` / `diff_b_1g.json` | 1 GB each | same data with N known changes (manifest lists them) |

Fixtures > 10 MB are **generated, never committed** (`.gitignore`). CI generates the 100 MB set.

## UI tests (`egui_kittest`)
Each UI step adds tests. Use `Harness` with a small fixture and assert through the accessibility tree (labels, roles):
- open file → root node visible, status bar shows the size
- expand / collapse with mouse and keyboard
- search "needle" → status shows "N matches"; F3 moves the selection; results row click selects the node
- breadcrumb shows the path; "Copy JSONPath" puts the expected string on the (mocked) clipboard
- theme switch, settings persistence round-trip
- invalid file → error banner with line/column

## Release checklist (manual, ~20 min)
- [ ] Download the Windows zip on a clean Windows 10/11 machine → SmartScreen → Run anyway → app starts
- [ ] Download the macOS dmg on Apple Silicon (and on Intel if available) → Open Anyway flow works → app starts
- [ ] Open the 2 GB fixture: first content < 1 s, index completes, status bar shows timing
- [ ] Search a common word: count appears, F3 cycles, results click jumps
- [ ] Re-open the same file: instant (cache hit shown in status bar)
- [ ] Open an invalid file: clear error with line/column; valid prefix browsable
- [ ] Drag & drop, recent files, dark/light theme
- [ ] Memory in Task Manager / Activity Monitor within targets
- [ ] `docs/PERF.md` updated with this release's numbers
