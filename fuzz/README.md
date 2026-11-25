# Fuzz targets

Needs a nightly toolchain and `cargo-fuzz` (not part of the main workspace):

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
cd fuzz && cargo +nightly fuzz run index_any_bytes -- -max_total_time=600
```

| Target | Checks |
|---|---|
| `index_any_bytes` | No panic on any input; the parallel builder (chunk sizes 64, 65, 4096, 8 MiB) equals the scalar reference; validity agrees with `serde_json` (`IgnoredAny`); navigation (`children`, `node_at_offset`, `path_of`) doesn't panic. |
