//! Deterministic test-fixture generator for Braceray.
//!
//! Every generator is a pure function of its options and seed: the same
//! inputs always give byte-identical files, whatever the thread count.
//! See `tools/fixtures/README.md` for usage.

pub mod all;
pub mod diff;
pub mod emit;
pub mod encoding;
pub mod invalid;
pub mod logs;
pub mod manifest;
pub mod orders;
pub mod records;
pub mod shapes;
pub mod size;
pub mod text;

/// Default seed for every generator.
pub const DEFAULT_SEED: u64 = 42;
