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
| `pretty_range` | No panic; pretty rows tile the input; for valid JSON, seeking from an anchor and stepping back give the rows of a full pass. |
| `search_any` | No panic; with chunk sizes 1–4096 B and caps 0 / 1 / 5 / ∞, the count, the stored prefix and full `next_after` / `prev_before` walks equal a naive non-overlapping scan (case-sensitive, ASCII-folded, whole word). |
