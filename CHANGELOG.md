# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Cargo workspace with `braceray-core`, `braceray-app` (binary `braceray`) and `tools/fixtures` (`gen-fixtures`).
- Empty "Braceray" window showing "Drop a JSON file here".
- CI on Windows, macOS and Linux (fmt, clippy, release build) plus a `cargo deny` job. Tests run locally only for now.
- MIT license, README, placeholder app icon.
- `gen-fixtures`: deterministic, parallel test-data generator (`all`, `orders`, `ndjson`, `deep`, `wide`, `prim`, `string`, `invalid`, `encoding`, `diff`) with needle, invalid-position, encoding and diff manifests. 2 GB in ~1.5 s.
- Golden test fixtures in `crates/braceray-core/tests/fixtures/` (valid edge cases, invalid, encoding, small generated samples).
- `braceray_core::Document`: opens files of any size (tested with 5 GiB) in O(1) via a read-only memory map; skips a UTF-8 BOM; rejects UTF-16/32 (with convert commands for macOS and Windows), binary, empty, whitespace-only and non-JSON files with clear messages; records a `FileIdentity` (canonical path, size, mtime, xxh3 of the first and last 1 MiB) and detects changes on disk; sniffs JSON vs NDJSON.
- Structural index (reference implementation): RFC 8259 validation with an explicit stack (any depth), exact error position ("Invalid JSON at line 1, column 17 (offset 20): expected ',' or '}', found '\"'"), 24-byte container nodes, line table, cancellation and progress.
- SIMD block classifier (NEON, SSE2, AVX2, `pclmulqdq`, scalar fallback) with runtime CPU detection: 5.25 GB/s single-threaded on Apple M3 Pro.
