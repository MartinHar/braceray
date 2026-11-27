//! Braceray core engine: file access, structural index and search.
//!
//! This crate has no UI dependencies and is fully unit-testable.

pub mod cache;
pub mod document;
pub mod error;
pub mod goto;
pub mod index;
pub mod job;
mod mmap;
pub mod nav;
pub mod path;
pub mod pretty;
pub mod search;
pub mod tree;

pub use document::{Document, FileIdentity, Sniff};
pub use error::{Encoding, Error};
pub use index::{Index, JsonError, Validation};
pub use job::{CancelToken, JobEvent, JobHandle, Progress, Repaint};

/// Returns the version of the Braceray core crate.
///
/// # Example
/// ```
/// assert!(!braceray_core::version().is_empty());
/// ```
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_not_empty() {
        assert!(!version().is_empty());
    }
}
