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
