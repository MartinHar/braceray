# Performance measurements

Targets are in [specs/overview.md](specs/overview.md) → *Performance targets*. Summary per target is in
[PROGRESS.md](PROGRESS.md) → *Performance targets tracker*. Record every measurement here.

## Machines
| ID | CPU | RAM | Disk | OS |
|---|---|---|---|---|
| M3P | Apple M3 Pro, 12 cores | 18 GB | internal SSD (APFS, 85 % full) | macOS 27.2 |

## Measurements
| Date | Step | Commit | Benchmark | Fixture | Machine | Result | Notes |
|---|---|---|---|---|---|---|---|
| 2026-11-25 | 02 | (step 02) | `gen-fixtures orders --size 2g` (release) | orders_2g.json (pretty, 2,450,341 records) | M3P | 1.34–1.64 s wall (1.5–1.9 GiB/s) | 2 runs; target < 10 s ✅ |
| 2026-11-25 | 02 | (step 02) | `gen-fixtures orders --size 2g --minified` | orders_min_2g.json | M3P | 1.32 s wall | |
| 2026-11-25 | 02 | (step 02) | `gen-fixtures all --max-size 100m` | 41 files, 852 MiB | M3P | 0.81 s wall | target < 10 s ✅; two runs → identical SHA-256 for all 41 files |
| 2026-11-25 | 03 | (step 03) | `Document::open` (test `document_big`, release) | orders_2g.json (2,147,484,144 B) | M3P | first open 0.23–5.6 ms, best warm 0.14–0.35 ms | 2 runs; target < 50 ms ✅. Cold (after reboot) not measured |
| 2026-11-25 | 03 | (step 03) | RSS growth after `Document::open` | orders_2g.json | M3P | +2.03 MiB (2,129,920 B) | target < 5 MB ✅; that's the 2 × 1 MiB identity hash window |
| 2026-11-25 | 03 | (step 03) | `Document::open` of a 5 GiB sparse file (debug build) | temp file, `set_len` | M3P | 4.9 ms | `len()` correct |
| 2026-11-25 | 04a | (step 04a) | `index::scalar::build` (test `document_big::scalar_index_2g`, release) | orders_2g.json | M3P | 5.72 s (0.38 GB/s), 19,603,166 nodes, 470.7 MB index (21.9 % of file) | reference only; fast builder comes in 4b/4c |
| 2026-11-25 | 04b | (step 04b) | `simd::scan`, 1 thread (test `document_big::simd_scan_2g`, release) | orders_2g.json | M3P | neon 0.41 s (**5.25 GB/s**); scalar 5.66 s (0.38 GB/s) | classify + escape + in-string masks + structural count only; 169,690,851 structurals |
| 2026-11-25 | 04b | (step 04b) | same, x86_64 build under Rosetta 2 (`ROSETTA_ADVERTISE_AVX=1`) | orders_2g.json | M3P (emulated) | avx2+pclmul 1.29 GB/s, sse2+pclmul 0.58 GB/s, scalar 0.30 GB/s | emulated, not representative of real x86 |
| 2026-11-25 | 04c | (step 04c) | `build_index`, all cores (test `document_big::fast_index_2g`, release, warm = 2nd run of one file) | orders_2g.json | M3P | **1.18 s (1.81 GB/s)**, 19,603,166 nodes, index 21.9 % of file | target < 5 s ✅, ≤ 25 % ✅. Consumer ≈ 0.94 s of it |
| 2026-11-25 | 04c | (step 04c) | same, 1 thread | orders_2g.json | M3P | 2.82 s (0.76 GB/s) | tokenizer alone ≈ 1.5 GB/s, classifier 5.25 GB/s |
| 2026-11-25 | 04c | (step 04c) | same, all cores, warm | orders_min_2g.json | M3P | 1.94 s (1.11 GB/s), adaptive depth ≤ 1, 3,706,250 nodes, 4.2 % | |
| 2026-11-25 | 04c | (step 04c) | same, all cores, first run after reading the other 2 GB file (partly from SSD) | orders_2g / orders_min_2g | M3P | 3.00 s / 3.74 s | not a true cold (no reboot); cold target < 10 s |
| 2026-11-25 | 04d | (step 04d) | criterion `index_100m/scalar_index` (`--quick`) | orders_100m.json (generated, pretty) | M3P | 138 ms (725 MiB/s) | warm; the 2 GB scalar number above includes cold reads |
| 2026-11-25 | 04d | (step 04d) | criterion `index_100m/simd_index_1thread` | orders_100m.json | M3P | 137 ms (732 MiB/s) | same as scalar: the sequential consumer dominates (I-003) |
| 2026-11-25 | 04d | (step 04d) | criterion `index_100m/simd_index_parallel` | orders_100m.json | M3P | 63 ms (1.55 GiB/s) | |
| 2026-11-25 | 04d | (step 04d) | criterion `children_of_1m_array` (fresh navigator: checkpoints + 50 children at 500,000) | `[0, 1, …, 999999]` (6.9 MB) | M3P | 6.0 ms | step 07 target "expand 1M children < 200 ms" |
| 2026-11-25 | 04d | (step 04d) | `cargo +nightly fuzz run index_any_bytes` | libFuzzer, 120 s | M3P | 546,754 runs, 0 crashes, 0 mismatches | |
| 2026-11-27 | 05 | (step 05) | re-open with index cache: `Document::open` + `load_or_build` (test `document_big::cache_reopen_2g`, release, warm) | orders_2g.json (entry 471 MB) | M3P | **first 14.5 ms, best 13.8 ms** | target < 1 s ✅. Includes the full checksum of the entry (~45 GB/s on all cores) and node checks. Build 3.29 s that run (partly cold); cache write 147 ms in the background |
| 2026-11-27 | 05 | (step 05) | same | orders_min_2g.json (adaptive, entry 89 MB) | M3P | first 3.6 ms, best 3.3 ms | write 14 ms |
| 2026-11-27 | 05 | (step 05) | criterion `cache_100m/cache_load` (`--quick`) | orders_100m.json (entry ≈ 23 MB) | M3P | 1.89 ms | map + header + checksum + node checks |
| 2026-11-27 | 05 | (step 05) | criterion `cache_100m/cache_store` | orders_100m.json | M3P | 7.2 ms (noisy: 6–33 ms between runs) | temp file + rename + eviction scan |
| 2026-11-27 | 05 | (step 05) | criterion `index_100m/*` re-run (gate) | orders_100m.json | M3P | simd 1 thread 137–147 ms, parallel 65–72 ms, children_of_1m 6.0 ms | within noise of step 04 |
| 2026-11-27 | 05 | (step 05) | criterion `index_100m/scalar_index` | orders_100m.json | M3P | 270–300 ms in `target/`; **139–147 ms** for the identical source built in another target dir or worktree | Not a code regression: HEAD in `target/` 141 ms, this tree in a scratch target dir 139 ms, `-C llvm-args=-align-loops=64` 141 ms. The scalar reference loop is sensitive to code alignment, and the layout depends on the build path. Only the test oracle is affected (I-005) |
| 2026-11-27 | 06 | (step 06) | Release app, `braceray orders_2g.json`, cache empty (shown in the window) | orders_2g.json | M3P | "Indexed 2.15 GB in 3.2 s", live speed 1.45 GB/s at 87 % | Wall time in the app incl. `Document::open`; first run after a rebuild, file partly out of the page cache, plus the 471 MB cache write starting right after. Core-only warm build is 1.18 s (step 04) |
| 2026-11-27 | 06 | (step 06) | Release app, re-open `orders_2g.json` (status bar) | orders_2g.json | M3P | "Index: from cache (0.04 s)" | Includes the cache lookup on the job thread; target < 1 s ✅ |
| 2026-11-27 | 06 | (step 06) | Status-bar memory (resident set) | orders_2g.json | M3P | 96 MB after re-open from cache; 2.24 GB right after a fresh build | The RSS counts resident pages of the mapped file and the mapped cache entry, so it isn't the "app memory excluding the mapped file" of the target (I-008) |
| 2026-11-27 | 07 | (step 07) | checkpoint table of the root (`Navigator::prepare`), release | orders_2g.json (2,450,341 children) | M3P | 295 ms (binary search per child) → **~110 ms** with the galloping node lookup, warm; 1.2–3.3 s right after a cache load / with pages evicted | runs on a worker; the UI shows "loading…" rows meanwhile; now prefetched in the background |
| 2026-11-27 | 07 | (step 07) | same | orders_min_2g.json (3,706,249 children, adaptive) | M3P | 224 ms → ~105 ms warm | |
| 2026-11-27 | 07 | (step 07) | criterion `tree_10m/expand_root` (`--quick`) | `[0,…,9999999]` (79 MB) | M3P | 68 ns | 10 groups of 1,000,000; target "expand 1M children < 200 ms" ✅ |
| 2026-11-27 | 07 | (step 07) | criterion `tree_10m/rows_for_viewport_50` | same, prepared | M3P | **4.9 µs** | children 5,500,400..450, parsed from checkpoint 5,499,904 (≈ 500 children) |
| 2026-11-27 | 07 | (step 07) | criterion `tree_10m/prepare_root` | same | M3P | 73 ms | 10 M children |
| 2026-11-27 | 07 | (step 07) | test `tree::deep_100k_by_keyboard` (release) | `[[[…]]]` × 100,000 | M3P | 100k expands by "→" 26 ms total; capture + restore 30 ms | |
| 2026-11-27 | 07 | (step 07) | App (opt-level 3 + inspection), open → first tree rows (debug overlay), cache stale | orders_2g.json | M3P | **134 ms** (preview of the first 4 MB; root shows "[4,815+]"); a later run 163 ms | target "first visible content < 1 s" ✅. Process start → inspection server up is 1.6 s in this debug-info build (window + GPU), not counted |
| 2026-11-27 | 07 | (step 07) | App, expand the root / last expand (overlay) | orders_2g.json, prim_array_10m.json, wide_1m.json | M3P | < 0.01 ms each | groups need no child reads; expanding the last group of `wide_1m` waited for a 46 ms prepare job |
| 2026-11-27 | 07 | (step 07) | App, 300 mouse-wheel events over a fully expanded tree (overlay after Reset) | prim_array_10m.json, 10,010,011 rows, 20,011 segments | M3P | UI p99 0.24 / max 0.35 ms; **CPU frame p50 0.27 · p99 0.44 · max 0.53 ms** | target < 16 ms ✅ |
| 2026-11-27 | 07 | (step 07) | App, drag the scrollbar from top to bottom (300 steps) | same | M3P | UI max 0.56 ms; **CPU frame p50 0.45 · p99 0.94 · max 1.15 ms**; ends at `[9,999,999]` | ✅ |
| 2026-11-27 | 07 | (step 07) | App, "Expand everything" (debug button, UI thread) | prim_array_10m.json / deep_100k.json | M3P | 286 ms for 10,010,011 rows; 199,999 rows (depth 99,999) | test tool only; frame max 1.09 ms at depth 99,999 afterwards |
| 2026-11-27 | 07 | (step 07) | criterion gate re-run (`--quick`) | orders_100m.json / 1M / 10M arrays | M3P | simd 1 thread 143 ms, parallel 68 ms, children_of_1m 6.28 ms, cache_load 1.95 ms, cache_store 7.9–32 ms (noisy, as before), scalar_index 289 ms (I-005 layout artefact); tree_10m: expand_root 56 ns, rows_for_viewport_50 5.2 µs, prepare_root 74 ms | all within 10 % of step 05/06 |
