//! An open, memory-mapped JSON file.

mod detect;

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use xxhash_rust::xxh3::Xxh3;

pub use detect::Sniff;
pub(crate) use detect::is_json_ws;

use crate::error::Error;
use crate::mmap::MappedFile;

/// Bytes hashed at each end of the file for [`FileIdentity::content_hash`].
const HASH_WINDOW: usize = 1024 * 1024;

/// Identifies a file version cheaply: used to detect changes and as the index cache key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIdentity {
    /// Absolute path with symlinks resolved.
    pub canonical_path: PathBuf,
    /// File size in bytes, including a UTF-8 BOM if there is one.
    pub size: u64,
    /// Last modification time, if the platform reports one.
    pub mtime: Option<SystemTime>,
    /// xxh3 of the first and the last 1 MiB of the file (the whole file if it is ≤ 2 MiB).
    /// Constant time, whatever the file size.
    pub content_hash: u64,
}

/// A read-only, memory-mapped JSON file.
///
/// Opening is O(1) in the file size: the file is mapped, not read, and only the first few
/// KiB plus 1 MiB at each end are touched. Bytes are paged in by the OS when they're used.
#[derive(Debug)]
pub struct Document {
    path: PathBuf,
    map: MappedFile,
    identity: FileIdentity,
    /// 3 if the file starts with a UTF-8 BOM, else 0.
    content_start: usize,
}

impl Document {
    /// Opens and maps `path`, and checks that it looks like UTF-8 JSON.
    ///
    /// Errors: [`Error::Io`] / [`Error::NotAFile`] if it can't be opened,
    /// [`Error::EmptyFile`] / [`Error::WhitespaceOnly`] if there's no content,
    /// [`Error::Encoding`] for UTF-16/32 or binary data, [`Error::NotJson`] if the first
    /// byte can't start a JSON value and [`Error::Mmap`] if the OS refuses to map it
    /// (for example on some network drives). The file is never read into RAM instead.
    ///
    /// # Example
    /// ```
    /// use braceray_core::Document;
    ///
    /// let dir = tempfile::tempdir()?;
    /// let path = dir.path().join("data.json");
    /// std::fs::write(&path, b"\xEF\xBB\xBF{\"a\": 1}")?;
    ///
    /// let doc = Document::open(&path)?;
    /// assert_eq!(doc.bytes(), b"{\"a\": 1}"); // the BOM is skipped
    /// assert_eq!(doc.len(), 8);
    /// assert!(!doc.has_changed_on_disk());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn open(path: &Path) -> Result<Arc<Document>, Error> {
        let io_err = |source| Error::Io {
            path: path.to_owned(),
            source,
        };
        let file = File::open(path).map_err(io_err)?;
        let meta = file.metadata().map_err(io_err)?;
        if !meta.is_file() {
            return Err(Error::NotAFile {
                path: path.to_owned(),
            });
        }
        if meta.len() == 0 {
            return Err(Error::EmptyFile);
        }
        let canonical_path = fs::canonicalize(path).map_err(io_err)?;
        let map = MappedFile::map(&file).map_err(|source| Error::Mmap { source })?;
        let bytes = map.bytes();

        let content_start = detect::check_bom(bytes).map_err(Error::Encoding)?;
        let content = &bytes[content_start..];
        let sample = &content[..content.len().min(detect::ENCODING_SAMPLE)];
        detect::check_sample(sample).map_err(Error::Encoding)?;
        detect::first_value_byte(content)?;

        let identity = FileIdentity {
            canonical_path,
            size: bytes.len() as u64,
            mtime: meta.modified().ok(),
            content_hash: head_tail_hash(bytes),
        };
        Ok(Arc::new(Document {
            path: path.to_owned(),
            map,
            identity,
            content_start,
        }))
    }

    /// The JSON content: the mapped file without its UTF-8 BOM (if any).
    ///
    /// All offsets used by the index and search are offsets into this slice.
    pub fn bytes(&self) -> &[u8] {
        &self.map.bytes()[self.content_start..]
    }

    /// Length of [`bytes`](Self::bytes) in bytes (the file size minus a BOM).
    pub fn len(&self) -> u64 {
        self.bytes().len() as u64
    }

    /// Always `false`: [`open`](Self::open) rejects files without content.
    pub fn is_empty(&self) -> bool {
        self.bytes().is_empty()
    }

    /// The path the file was opened with.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Size, mtime and content hash recorded at open time.
    pub fn identity(&self) -> &FileIdentity {
        &self.identity
    }

    /// `true` if the file's size or mtime differs from the values recorded at open time,
    /// or the file can't be found any more. Reads metadata only.
    pub fn has_changed_on_disk(&self) -> bool {
        match fs::metadata(&self.identity.canonical_path) {
            Ok(meta) => {
                meta.len() != self.identity.size || meta.modified().ok() != self.identity.mtime
            }
            Err(_) => true,
        }
    }

    /// Guesses whether the content is a single JSON document or NDJSON (first 64 KiB only).
    pub fn sniff(&self) -> Sniff {
        detect::sniff(self.bytes())
    }

    /// Tells the OS the file is about to be read front to back (before an index build).
    /// No-op on platforms without `madvise`.
    pub fn advise_sequential(&self) {
        self.map.advise_sequential();
    }

    /// Tells the OS the file will be read in random order (after the index build).
    /// No-op on platforms without `madvise`.
    pub fn advise_random(&self) {
        self.map.advise_random();
    }
}

/// xxh3 over the first and last [`HASH_WINDOW`] bytes (non-overlapping).
fn head_tail_hash(bytes: &[u8]) -> u64 {
    let head = &bytes[..bytes.len().min(HASH_WINDOW)];
    let tail_start = head.len().max(bytes.len().saturating_sub(HASH_WINDOW));
    let mut hasher = Xxh3::new();
    hasher.update(head);
    hasher.update(&bytes[tail_start..]);
    hasher.digest()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_covers_head_and_tail_only() {
        let mut data = vec![b' '; 5 * HASH_WINDOW];
        let base = head_tail_hash(&data);
        data[2 * HASH_WINDOW] = b'x'; // middle: not hashed
        assert_eq!(head_tail_hash(&data), base);
        data[10] = b'x';
        assert_ne!(head_tail_hash(&data), base);
        let base = head_tail_hash(&data);
        *data.last_mut().unwrap() = b'x';
        assert_ne!(head_tail_hash(&data), base);
    }

    #[test]
    fn hash_small_inputs_hash_everything_once() {
        assert_eq!(head_tail_hash(b"abc"), xxhash_rust::xxh3::xxh3_64(b"abc"));
        assert_eq!(head_tail_hash(b""), xxhash_rust::xxh3::xxh3_64(b""));
    }
}
