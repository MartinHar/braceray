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
