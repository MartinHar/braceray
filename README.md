# Braceray

**X-ray vision for giant JSON.**

Braceray is a free, open-source (MIT), read-only desktop viewer for Windows and macOS that opens
2 GB+ JSON files instantly and searches them at disk speed. Files are memory-mapped and never
parsed into memory as a whole, so a 2 GB file does not need 20 GB of RAM.

> Status: early development. Nothing is usable yet. See [docs/PROGRESS.md](docs/PROGRESS.md).

## Planned features

**MVP (v0.1.0)**
- Open JSON of any size: file dialog, drag & drop, recent files, `braceray <file>`
- Parallel structural index with validation and exact error line/column
- Index cache (on by default) so the second open is instant
- Lazy tree view, syntax-highlighted raw view with on-the-fly pretty-printing
- Fast plain-text search with an exact match count, a results list and F3 navigation
- Breadcrumbs and copy path (JSONPath, jq, JavaScript), copy value or subtree
- Dark, light and system themes

**v1 (v1.0.0)**: regex and keys-only/values-only search, JSONPath queries, bookmarks,
NDJSON / JSON Lines, export to JSON and CSV.

**v2 (v2.0.0)**: structural diff of two big JSON files.

Out of scope: editing, cloud features, accounts, telemetry.

## Install

### macOS
_Placeholder. A `.dmg` will be published on GitHub Releases from v0.1.0._

### Windows
_Placeholder. A portable `.zip` will be published on GitHub Releases from v0.1.0._

## Build from source

Requirements: the stable Rust toolchain ([rustup.rs](https://rustup.rs)); on macOS the Xcode
Command Line Tools; on Windows the Visual Studio Build Tools (C++ workload); on Linux the egui
system libraries (see `.github/workflows/ci.yml`).

```sh
cargo run -p braceray-app            # debug build
cargo build --release -p braceray-app # optimized binary at target/release/braceray
cargo test --workspace
```

## License

MIT. See [LICENSE](LICENSE). Third-party licenses are listed in `THIRD_PARTY_LICENSES.md`.
