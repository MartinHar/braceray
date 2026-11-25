//! Reference indexer: a single-threaded, byte-by-byte RFC 8259 validator that builds the
//! node table with an explicit stack (no recursion).
//!
//! It is the oracle for the SIMD / parallel builder, which must produce an equal [`Index`].
//! Like `serde_json`'s `IgnoredAny`, it doesn't validate UTF-8 inside strings or pair
//! `\u` surrogates; everything else in the grammar is checked.

use super::Index;
use super::node::MAX_OFFSET;
use crate::error::Error;
use crate::job::{CancelToken, Progress};

/// Builds the index of `bytes` byte by byte.
///
/// `max_stored_depth` is the adaptive-mode limit (`None` stores every container).
/// Returns `Err` only for cancellation or a document over 1 TiB; invalid JSON gives an
/// `Ok` index whose [`Index::validation`] is `Invalid`.
///
/// # Example
/// ```
/// use braceray_core::index::{scalar, ContainerKind};
/// use braceray_core::{CancelToken, Progress};
///
/// let index = scalar::build(br#"{"a": [1, 2], "b": {}}"#, None, &CancelToken::new(), &Progress::default())?;
/// assert!(index.is_valid());
/// let kinds: Vec<_> = index.nodes().iter().map(|n| (n.kind(), n.child_count())).collect();
/// assert_eq!(kinds, [(ContainerKind::Object, 2), (ContainerKind::Array, 2), (ContainerKind::Object, 0)]);
/// # Ok::<(), braceray_core::Error>(())
/// ```
pub fn build(
    bytes: &[u8],
    max_stored_depth: Option<u32>,
    cancel: &CancelToken,
    progress: &Progress,
) -> Result<Index, Error> {
    if bytes.len() as u64 > MAX_OFFSET {
        return Err(Error::FileTooLarge);
    }
    super::parser::build_reference(bytes, max_stored_depth, cancel, progress)
}
