# gen-fixtures

Deterministic test-data generator for Braceray. Big fixtures are **never committed**: anyone can recreate them byte for byte.

```sh
cargo build --release -p gen-fixtures
alias gen-fixtures=target/release/gen-fixtures
```

## Commands

```sh
gen-fixtures all      --out fixtures-data/ [--max-size 3g]          # whole catalogue
gen-fixtures orders   --size 2g --out fixtures-data/orders_2g.json [--minified] [--seed 42]
gen-fixtures ndjson   --size 1g --out fixtures-data/logs_1g.ndjson
gen-fixtures deep     --depth 100000 --out fixtures-data/deep_100k.json
gen-fixtures wide     --keys 1000000 --out fixtures-data/wide_1m.json [--minified]
gen-fixtures prim     --count 10000000 --out fixtures-data/prim_array_10m.json [--minified]
gen-fixtures string   --size 200m --out fixtures-data/huge_string.json
gen-fixtures invalid  --out fixtures-data/invalid/ [--big-size 1g]  # + invalid_manifest.json
gen-fixtures encoding --out fixtures-data/encoding/                 # + encoding_manifest.json
gen-fixtures diff     --size 1g --changes 1000 --out fixtures-data/diff/   # + diff_manifest.json
```

### `bench-big` (performance targets, step 12)

```sh
cargo build --release -p gen-fixtures
./target/release/gen-fixtures bench-big --machine M3P [--files orders_2g.json,orders_min_2g.json] [--runs 3] [--cold]
```

Generates the listed `orders` fixtures in `--dir` (default `fixtures-data/`) when they are missing, then measures each one:
open → first content (preview index + first tree rows), full index (warm; cold with `--cold`), index memory, re-open from
the index cache, plain and case-insensitive search for `NEEDLE_RARE` / `needle_common` (counts checked against the
manifest), first search result, expand root, checkpoint table of the root, peak RSS. It prints Markdown rows for
`docs/PERF.md` (✅ / ❌ against `docs/specs/overview.md`) and exits non-zero if a target is missed. `--cold` drops the OS
page cache first, which needs root (`sudo purge` on macOS, `drop_caches` on Linux); without it the cold rows say
"not measured". Measure one 2 GB file per run on machines with < 32 GB RAM, so both don't compete for the page cache.

Sizes accept `500`, `10k`, `100m`, `2g` (binary units). `-q` hides progress. `fixtures-data/` is in `.gitignore`.

`all --max-size N` caps every fixture at `N`. Sizes that collapse onto the same cap are generated once, and every fixture type is still created. CI-sized set: `--max-size 100m` (~850 MB, under 1 s on an M3 Pro).

## Guarantees

- **Deterministic.** Same options and seed give identical bytes, whatever the thread count. Record `i` is a pure function of `(seed, i)`, and blocks of records are generated in parallel (rayon) but written in order.
- **Fast.** Output streams through an 8 MiB `BufWriter`; nothing is built in memory. A 2 GB `orders` file takes ~1.5 s on an M3 Pro (see `docs/PERF.md`).
- **Valid.** Every "valid" kind parses with `serde_json` (tests cover ≤ 10 MB of each kind).

## What's in the files

| Kind | Shape |
|---|---|
| `orders` | Top-level array of order objects (~850 B pretty / ~600 B minified). Covers Armenian, Cyrillic, CJK, emoji, every escape (`\" \\ \/ \b \f \n \r \t \uXXXX`, surrogate pair), negative numbers and exponents, `null`, booleans, empty `{}` / `[]`, and `{ } [ ] : ,` inside strings. Pretty uses a 2-space indent. |
| `ndjson` | One minified log record per line. |
| `deep` | `{"k":"v","d":[1,{"k":"v","d":[1,…"bottom"…]}]}`, depth = number of containers. Always minified. |
| `wide` | One object with keys `key_0 … key_N-1` and mixed primitive / empty-container values. |
| `prim` | Array of numbers: ints, negatives, two-decimal values, `X.5e±N`. |
| `string` | `{"data": "<one huge string>"}` with escapes and Unicode. |
| `invalid` | Truncated, trailing commas, missing `:` / `,`, bad literal, raw control char, unclosed string, mismatched bracket, trailing garbage, a multi-line case, a case after multi-byte UTF-8, and `invalid_end_<size>.json` (a big pretty file whose only error is a trailing comma before the final `]`). |
| `encoding` | Plain UTF-8, UTF-8 BOM, UTF-16 LE/BE (with BOM), UTF-16 LE without BOM, UTF-32 LE, empty, whitespace-only, binary. |
| `diff` | `diff_a_<size>.json` / `diff_b_<size>.json`: same records except N changed ones (`total` +1.00, `paid` flipped, `coupon` added, or `notes` removed). |

## Manifests

**`<file>.manifest.json`** (`orders`, `ndjson`) records the byte size, the record count and needle counts (case-sensitive and ASCII case-insensitive):
- `NEEDLE_RARE`: exactly 17 times, spread over the file
- `needle_common`: once in every record whose index ends in 50 (1 % of records)
- `needle`: substring of both (handy for case-insensitive tests)

**`invalid_manifest.json`** is `[{file, error_offset, line, column, description}]`:
- `error_offset` is the offending byte, or the file length for an unexpected end of input.
- `line` / `column` are 1-based, and the column counts **bytes**.
- They match `serde_json`'s position, with one quirk: for a raw control character serde reports the byte before it.

**`encoding_manifest.json`** is `[{file, expect, description}]`. `expect` is one of `utf8`, `utf8-bom`, `utf16le`, `utf16be`, `utf32le`, `empty`, `whitespace-only`, `binary`.

**`diff_manifest.json`** is `{file_a, file_b, seed, records, changes: [{kind, path, a, b}]}`:
- `path` is JSONPath (`$[123].total`).
- `a` / `b` are JSON text, or `null` when the key is absent on that side.

## Golden fixtures

Small committed copies (< 50 KB each) live in `crates/braceray-core/tests/fixtures/`:

- `valid/`: hand-written edge cases (top-level scalars, empties, every escape, Unicode, structural characters in strings, number edge cases, CRLF).
- `invalid/`, `encoding/`, `generated/`: generator output. A test fails if they go stale. Regenerate with:

```sh
G=crates/braceray-core/tests/fixtures
gen-fixtures -q invalid  --out $G/invalid --big-size 0
gen-fixtures -q encoding --out $G/encoding
gen-fixtures -q orders --size 16k --out $G/generated/orders_16k.json
gen-fixtures -q orders --size 16k --minified --out $G/generated/orders_min_16k.json
gen-fixtures -q ndjson --size 16k --out $G/generated/logs_16k.ndjson
gen-fixtures -q deep --depth 1000 --out $G/generated/deep_1000.json
```
