//! Braceray core engine: file access, structural index and search.
//!
//! This crate has no UI dependencies and is fully unit-testable.

pub mod document;
pub mod error;
mod mmap;

pub use document::{Document, FileIdentity, Sniff};
pub use error::{Encoding, Error};

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
