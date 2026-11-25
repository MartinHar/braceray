# Step 05 — Index cache (enabled by default)

**Phase:** MVP · **Depends on:** 04

## Goal
The second time a file is opened, its index loads from disk in well under a second instead of being rebuilt. The cache is **on by default**, bounded in size, safe against stale or corrupted data, and easy to clear.

## Implementation (see architecture.md §6)
- Location: `directories::ProjectDirs::from("io.github", "<username>", "braceray").cache_dir()/index/`.
- Cache key: hex of xxh3_128 over `FileIdentity` (canonical path, size, mtime nanos, head/tail hash).
- File format (`.brx`):
  ```
  magic "BRXIDX\0\0" | format_version u32 | app_version str(16) | flags u32
  file_size u64 | file_mtime i128 | head_tail_hash u128
  node_count u64 | line_entries u64 | validation block (ok / error offset,line,col,expected,found)
  adaptive_depth u32 | padding to 64
  [Node; node_count] (raw, little-endian, #[repr(C)], bytemuck::Pod)
  [LineEntry; line_entries]
  xxh3_64 checksum of everything above (for files < 256 MB; for larger ones, checksum of header + sampled pages)
  ```
- Write: after a successful (or invalid-but-complete) build, on a background thread, to `<key>.tmp` then atomic rename. Never block the UI. Skip files smaller than 20 MB (rebuild is faster than I/O). The threshold is a setting.
- Read: on open, if the key file exists, mmap it, validate magic, version, size and checksum, and cast with `bytemuck::try_cast_slice` (alignment-checked). Any problem → delete the entry and rebuild silently (log with `tracing`).
- Child offset tables and element checkpoints are **not** cached in v1 (built lazily, cheap).
- Eviction: keep `index.json` metadata (key → original path, size, last_access). After each write, if total > `max_cache_size` (default 4 GB), delete least-recently-used entries.
- Settings (wired to the UI in step 06; expose them in core now):
  - `index_cache_enabled: bool = true`
  - `index_cache_max_bytes: u64 = 4 GiB`
  - `index_cache_min_file_bytes: u64 = 20 MiB`
  - `clear_index_cache()` → returns bytes freed
- Status bar text (step 06): "Index: from cache (0.21 s)" vs "Index: built (3.8 s)".

## Acceptance criteria
- [ ] Re-open the 2 GB fixture: index ready in **< 1 s** (warm disk).
- [ ] Modifying the source file (append a byte, touch mtime) → the cache is ignored and the index rebuilt.
- [ ] A corrupted cache file (truncate or flip bytes) → detected, deleted, rebuilt; no crash.
- [ ] Cache from an older `format_version` → ignored and rebuilt.
- [ ] With the cache disabled, nothing is written to the cache dir.
- [ ] Eviction keeps the total under the limit (test with a tiny limit).
- [ ] Two app instances opening the same file at once don't corrupt the cache (temp + rename).

## Tests
Unit tests per bullet using `tempfile` dirs (cache dir injectable for tests). A bench of load time for a 100 MB fixture's cache.
