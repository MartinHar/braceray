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
