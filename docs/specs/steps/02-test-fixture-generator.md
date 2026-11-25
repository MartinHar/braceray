# Step 02 — Test fixture generator

**Phase:** MVP · **Depends on:** 01

## Goal
A deterministic generator that creates every test file listed in `testing-strategy.md` → *Fixture catalogue*, from 10 MB up to 3 GB, fast. Big files are never committed; anyone can recreate them.

## Tasks
1. `tools/fixtures` binary `gen-fixtures` with a CLI (use `clap` with derive):
   ```
   gen-fixtures all      --out fixtures-data/ [--max-size 3g]
   gen-fixtures orders   --size 2g --out fixtures-data/orders_2g.json [--minified] [--seed 42]
   gen-fixtures ndjson   --size 1g --out …
   gen-fixtures deep     --depth 100000 --out …
   gen-fixtures wide     --keys 1000000 --out …
   gen-fixtures prim     --count 10000000 --out …
   gen-fixtures string   --size 200m --out …
   gen-fixtures invalid  --out fixtures-data/invalid/      # writes invalid_manifest.json too
   gen-fixtures encoding --out fixtures-data/encoding/
   gen-fixtures diff     --size 1g --changes 1000 --out fixtures-data/diff/   # writes diff_manifest.json
   ```
2. Deterministic: same seed ⇒ byte-identical output. Use a small fast PRNG (e.g. `rand_xoshiro` or `fastrand` seeded).
3. Fast: stream through a `BufWriter` (8 MiB buffer), no building of trees in memory. Target: ≥ 500 MB/s on reference machines. Print progress and elapsed time.
4. Realistic `orders` records. Each element ≈ 400–1200 bytes:
   ```json
   {"id":123456,"uuid":"…","customer":{"name":"Anna Petrosyan","email":"anna.p@example.com","country":"AM","tags":["vip","b2b"]},
    "items":[{"sku":"SKU-0001","title":"Café crème \"special\"","qty":2,"price":12.5e0,"discount":null}],
    "total":25.0,"paid":true,"notes":"line1\nline2 \\ backslash é 😀","created_at":"2026-01-02T03:04:05Z"}
   ```
   Include: Unicode (Armenian, Cyrillic, CJK, emoji), every escape type, negative and exponent numbers, `null`, booleans, empty `{}` and `[]`, and strings containing `{ } [ ] : ,`. Plant known needles at known frequencies (e.g. the word `NEEDLE_RARE` exactly 17 times, and `needle_common` in 1 % of records) and write their counts to `<file>.manifest.json`.
5. `--minified` vs pretty (2-space indent) output.
6. Invalid fixtures + `invalid_manifest.json`: `[{file, error_offset, line, column, description}]`.
7. Add `fixtures-data/` to `.gitignore`. Commit small golden fixtures (< 50 KB each) to `crates/braceray-core/tests/fixtures/` (hand-crafted and edge cases).
8. Document usage in `tools/fixtures/README.md`.

## Acceptance criteria
- [ ] `gen-fixtures all --max-size 100m` finishes in < 10 s and creates every fixture type.
- [ ] Two runs with the same seed produce identical SHA-256 hashes.
- [ ] Every generated valid file parses with `serde_json` (verify for sizes ≤ 100 MB in a test).
- [ ] Every invalid file fails at exactly the manifest's offset (verify with serde's line/column).
- [ ] Needle counts in the manifest match a naive count (test on the 10 MB file).
- [ ] 2 GB orders file generates in < 10 s on an NVMe machine (report the time in `docs/PERF.md`).

## Tests
- Determinism test (generate 1 MB twice, compare bytes).
- Validity test (generate 10 MB of each valid kind, parse with serde_json).
- Manifest correctness tests for invalid and needle manifests.
