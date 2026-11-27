//! Preview mode: while the full index builds, the tree browses an index of the first few
//! MB. Offsets and child indices are the same as in the full index, so the expansion
//! state and the selection carry over (see [`TreeModel::expansions`](super::TreeModel::expansions)).

use crate::error::Error;
use crate::index::{Index, IndexOptions, build_bytes};
use crate::job::{CancelToken, Progress};
use crate::nav::{NavCache, Navigator};

/// Bytes indexed for the preview.
pub const PREVIEW_BYTES: usize = 4 << 20;

/// An index of a prefix of the document.
#[derive(Debug)]
pub struct Preview {
    len: usize,
    index: Index,
}

impl Preview {
    /// Indexes the first `limit` bytes of `bytes` (a few ms for 4 MB).
    ///
    /// The prefix usually ends inside a value, so the index reports "unexpected end"; the
    /// containers before the cut browse normally, and the ones cut off show the children
    /// found so far.
    ///
    /// # Errors
    /// [`Error::Cancelled`] when `cancel` fires.
    ///
    /// # Example
    /// ```
    /// use braceray_core::tree::preview::Preview;
    /// use braceray_core::nav::NavCache;
    /// use braceray_core::CancelToken;
    ///
    /// let bytes = br#"[{"id": 1}, {"id": 2}, {"id": 3}]"#;
    /// let preview = Preview::build(bytes, 20, &CancelToken::new())?;
    /// let cache = NavCache::default();
    /// let nav = preview.navigator(bytes, &cache);
    /// let root = nav.root_container().unwrap();
    /// // The prefix is `[{"id": 1}, {"id": 2`: the second object is cut off but listed.
    /// let children = nav.children(&root, 0..10);
    /// assert_eq!(children.len(), 2);
    /// assert_eq!(children[1].value.end as usize, preview.len());
    /// # Ok::<(), braceray_core::Error>(())
    /// ```
    pub fn build(bytes: &[u8], limit: usize, cancel: &CancelToken) -> Result<Self, Error> {
        let len = bytes.len().min(limit);
        let opts = IndexOptions {
            threads: Some(1),
            ..IndexOptions::default()
        };
        let index = build_bytes(&bytes[..len], &opts, cancel, &Progress::default())?;
        Ok(Preview { len, index })
    }

    /// Length of the indexed prefix.
    pub fn len(&self) -> usize {
        self.len
    }

    /// `true` for a preview of an empty document.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The prefix index.
    pub fn index(&self) -> &Index {
        &self.index
    }

    /// A navigator over the prefix of `bytes` (the same bytes the preview was built from).
    /// `cache` must be used for this preview only.
    pub fn navigator<'a>(&'a self, bytes: &'a [u8], cache: &'a NavCache) -> Navigator<'a> {
        Navigator::with_cache(&bytes[..self.len.min(bytes.len())], &self.index, cache)
    }
}
