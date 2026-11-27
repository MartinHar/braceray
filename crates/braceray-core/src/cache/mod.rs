//! On-disk index cache (step 05, architecture.md §6), enabled by default.
//!
//! Each indexed file gets one `<key>.brx` entry in the cache dir, where `key` is the hex
//! xxh3-128 of its [`FileIdentity`]. Entries are written on a background thread to a
//! unique temp file and renamed into place, so readers (other app instances included) see
//! either a complete entry or none. Loading maps the entry and uses its node table without
//! copying it. Anything wrong with an entry (stale, old format, truncated, corrupted)
//! deletes it, and the caller rebuilds the index.

mod format;
mod meta;
mod open;

pub use open::{IndexSource, LoadedIndex, load_or_build};

use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use xxhash_rust::xxh3::Xxh3;

use crate::document::{Document, FileIdentity};
use crate::index::{Index, IndexOptions};
use crate::job::CancelToken;
use crate::mmap::MappedFile;

pub(crate) use format::FORMAT_VERSION;

/// Extension of cache entries.
pub(crate) const ENTRY_EXT: &str = "brx";
/// Extension of files being written.
pub(crate) const TMP_EXT: &str = "tmp";

/// Cache settings (shown in the app's settings from step 06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheSettings {
    /// Read and write the cache. Default `true`.
    pub index_cache_enabled: bool,
    /// Most bytes all entries may use together; least-recently-used entries are deleted
    /// beyond it. Default 4 GiB.
    pub index_cache_max_bytes: u64,
    /// Files smaller than this are not cached (rebuilding is faster than the I/O).
    /// Default 20 MiB.
    pub index_cache_min_file_bytes: u64,
}

impl Default for CacheSettings {
    fn default() -> Self {
        CacheSettings {
            index_cache_enabled: true,
            index_cache_max_bytes: 4 << 30,
            index_cache_min_file_bytes: 20 << 20,
        }
    }
}

/// Why an entry couldn't be used. Only logged: the caller just rebuilds.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CacheError {
    /// Reading, writing or deleting a cache file failed.
    #[error("index cache I/O error: {0}")]
    Io(#[from] io::Error),
    /// The entry was written by another format version.
    #[error("index cache entry has format version {0}, expected {FORMAT_VERSION}")]
    FormatVersion(u32),
    /// The entry was written by another app version.
    #[error("index cache entry was written by another Braceray version")]
    AppVersion,
    /// The source file or the index options changed since the entry was written.
    #[error("index cache entry is stale (file or options changed)")]
    Stale,
    /// Truncated, flipped bytes or impossible values.
    #[error("index cache entry is corrupted: {0}")]
    Corrupt(&'static str),
    /// The entry holds an error message this version doesn't know.
    #[error("index cache entry has an unknown error message")]
    UnknownMessage,
    /// Loading was cancelled.
    #[error("index cache load cancelled")]
    Cancelled,
}

/// What [`IndexCache::store`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOutcome {
    /// The entry was written (its size in bytes).
    Written(u64),
    /// Not written, and why.
    Skipped(SkipReason),
}

/// Why an index wasn't written to the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The cache is disabled.
    Disabled,
    /// The file is smaller than [`CacheSettings::index_cache_min_file_bytes`].
    TooSmall,
    /// The entry alone would exceed [`CacheSettings::index_cache_max_bytes`].
    TooLarge,
    /// The index was itself loaded from the cache.
    FromCache,
    /// Big-endian targets don't use the cache (the format stores raw little-endian nodes).
    Unsupported,
}

/// The on-disk index cache.
///
/// # Example
/// ```
/// use braceray_core::cache::{CacheSettings, IndexCache, IndexSource, load_or_build};
/// use braceray_core::index::IndexOptions;
/// use braceray_core::{CancelToken, Document, Progress};
///
/// let dir = tempfile::tempdir()?;
/// let path = dir.path().join("a.json");
/// std::fs::write(&path, br#"{"users": [{"id": 1}, {"id": 2}]}"#)?;
/// let settings = CacheSettings { index_cache_min_file_bytes: 0, ..CacheSettings::default() };
/// let cache = IndexCache::new(dir.path().join("cache"), settings);
/// let (opts, cancel, progress) = (IndexOptions::default(), CancelToken::new(), Progress::default());
///
/// let doc = Document::open(&path)?;
/// let first = load_or_build(&doc, &opts, Some(&cache), &cancel, &progress)?;
/// assert_eq!(first.source, IndexSource::Built);
/// first.pending_write.map(|w| w.join()); // the entry is written in the background
///
/// let again = load_or_build(&doc, &opts, Some(&cache), &cancel, &progress)?;
/// assert_eq!(again.source, IndexSource::Cache);
/// assert_eq!(again.index.nodes(), first.index.nodes());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct IndexCache {
    dir: PathBuf,
    settings: CacheSettings,
}

impl IndexCache {
    /// A cache in `dir` (created on the first write).
    pub fn new(dir: PathBuf, settings: CacheSettings) -> Self {
        IndexCache { dir, settings }
    }

    /// The OS cache dir: `~/Library/Caches/io.github.martinhar.braceray/index` on macOS,
    /// `%LOCALAPPDATA%\martinhar\braceray\cache\index` on Windows and
    /// `$XDG_CACHE_HOME/braceray/index` (or `~/.cache/braceray/index`) elsewhere: the same
    /// places as `directories::ProjectDirs::from("io.github", "martinhar", "braceray")`.
    /// `None` if the environment names no home directory.
    pub fn default_dir() -> Option<PathBuf> {
        let env = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let base = if cfg!(target_os = "macos") {
            env("HOME")?.join("Library/Caches/io.github.martinhar.braceray")
        } else if cfg!(windows) {
            env("LOCALAPPDATA")?
                .join("martinhar")
                .join("braceray")
                .join("cache")
        } else {
            env("XDG_CACHE_HOME")
                .filter(|p| p.is_absolute())
                .or_else(|| env("HOME").map(|h| h.join(".cache")))?
                .join("braceray")
        };
        Some(base.join("index"))
    }

    /// The cache dir.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Current settings.
    pub fn settings(&self) -> &CacheSettings {
        &self.settings
    }

    /// Replaces the settings (takes effect for the next load / store).
    pub fn set_settings(&mut self, settings: CacheSettings) {
        self.settings = settings;
    }

    /// The entry file of `identity`.
    pub fn entry_path(&self, identity: &FileIdentity) -> PathBuf {
        self.dir
            .join(format!("{}.{ENTRY_EXT}", cache_key(identity)))
    }

    pub(crate) fn skip_reason(&self, doc_size: u64) -> Option<SkipReason> {
        if cfg!(target_endian = "big") {
            Some(SkipReason::Unsupported)
        } else if !self.settings.index_cache_enabled {
            Some(SkipReason::Disabled)
        } else if doc_size < self.settings.index_cache_min_file_bytes {
            Some(SkipReason::TooSmall)
        } else {
            None
        }
    }

    /// Loads the cached index of `doc` built with `opts`, or `None` on a miss. A bad entry
    /// (stale, other version, corrupted) is deleted and gives `None`.
    pub fn load(&self, doc: &Document, opts: &IndexOptions, cancel: &CancelToken) -> Option<Index> {
        if self.skip_reason(doc.identity().size).is_some() {
            return None;
        }
        let key = cache_key(doc.identity());
        let path = self.dir.join(format!("{key}.{ENTRY_EXT}"));
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
            Err(e) => {
                tracing::warn!("index cache: can't open {}: {e}", path.display());
                return None;
            }
        };
        let result = match MappedFile::map(&file) {
            Ok(map) => format::read(Arc::new(map), &stamp(doc, opts), cancel),
            Err(e) => Err(CacheError::Io(e)),
        };
        drop(file);
        match result {
            Ok(index) => {
                meta::Meta::touch(&self.dir, &key, Some(&doc.identity().canonical_path), None);
                Some(index)
            }
            Err(CacheError::Cancelled) => None,
            Err(e) => {
                // The map is dropped with the error, so the file can be deleted on Windows.
                tracing::info!("index cache: discarding {}: {e}", path.display());
                if let Err(e) = fs::remove_file(&path) {
                    tracing::debug!("index cache: can't delete {}: {e}", path.display());
                }
                None
            }
        }
    }

    /// Writes `index` (of `doc`, built with `opts`) to the cache, then evicts entries
    /// beyond the size limit. Blocks for the I/O; see [`spawn_store`](Self::spawn_store).
    pub fn store(
        &self,
        doc: &Document,
        index: &Index,
        opts: &IndexOptions,
    ) -> Result<StoreOutcome, CacheError> {
        self.store_stamped(doc.identity(), stamp(doc, opts), index)
    }

    /// [`store`](Self::store) on a background thread.
    pub fn spawn_store(
        &self,
        doc: &Document,
        index: Arc<Index>,
        opts: &IndexOptions,
    ) -> io::Result<JoinHandle<Result<StoreOutcome, CacheError>>> {
        let cache = self.clone();
        let identity = doc.identity().clone();
        let stamp = stamp(doc, opts);
        std::thread::Builder::new()
            .name("braceray-cache-write".into())
            .spawn(move || cache.store_stamped(&identity, stamp, &index))
    }

    fn store_stamped(
        &self,
        identity: &FileIdentity,
        stamp: format::Stamp,
        index: &Index,
    ) -> Result<StoreOutcome, CacheError> {
        if let Some(reason) = self.skip_reason(identity.size) {
            return Ok(StoreOutcome::Skipped(reason));
        }
        if index.is_from_cache() {
            return Ok(StoreOutcome::Skipped(SkipReason::FromCache));
        }
        let size = format::encoded_len(index);
        if size > self.settings.index_cache_max_bytes {
            return Ok(StoreOutcome::Skipped(SkipReason::TooLarge));
        }
        fs::create_dir_all(&self.dir)?;
        let key = cache_key(identity);
        let path = self.dir.join(format!("{key}.{ENTRY_EXT}"));
        let tmp = tmp_path(&self.dir, &key);
        let written = File::create_new(&tmp).and_then(|f| {
            let mut out = BufWriter::with_capacity(8 << 20, f);
            format::write(&mut out, index, &stamp)
        });
        let renamed = written.and_then(|()| fs::rename(&tmp, &path));
        if let Err(e) = renamed {
            let _ = fs::remove_file(&tmp);
            // Another instance may hold the old entry open (Windows can't replace a mapped
            // file); if an entry exists, that instance wrote it.
            if path.exists() {
                tracing::debug!("index cache: kept the existing entry for {key}: {e}");
                return Ok(StoreOutcome::Written(size));
            }
            return Err(e.into());
        }
        meta::Meta::touch(&self.dir, &key, Some(&identity.canonical_path), Some(size));
        meta::evict(&self.dir, self.settings.index_cache_max_bytes);
        Ok(StoreOutcome::Written(size))
    }

    /// Deletes every entry (and leftover temp files and metadata). Returns the bytes freed.
    /// Entries another process has open may stay (Windows).
    pub fn clear(&self) -> Result<u64, CacheError> {
        let read_dir = match fs::read_dir(&self.dir) {
            Ok(r) => r,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        let mut freed = 0;
        for entry in read_dir.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str());
            let is_ours = ext == Some(ENTRY_EXT)
                || ext == Some(TMP_EXT)
                || path.file_name().is_some_and(|n| n == meta::META_FILE);
            if !is_ours {
                continue;
            }
            let len = entry.metadata().map_or(0, |m| m.len());
            match fs::remove_file(&path) {
                Ok(()) => freed += len,
                Err(e) => tracing::debug!("index cache: can't delete {}: {e}", path.display()),
            }
        }
        Ok(freed)
    }

    /// Total size of all entries in bytes.
    pub fn total_bytes(&self) -> u64 {
        fs::read_dir(&self.dir).map_or(0, |r| {
            r.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == ENTRY_EXT))
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
    }
}

/// Clears `cache` (see [`IndexCache::clear`]); returns the bytes freed.
pub fn clear_index_cache(cache: &IndexCache) -> Result<u64, CacheError> {
    cache.clear()
}

/// The cache key of a file: hex xxh3-128 of canonical path, size, mtime and content hash.
pub fn cache_key(identity: &FileIdentity) -> String {
    let mut h = Xxh3::new();
    let path = identity.canonical_path.as_os_str().as_encoded_bytes();
    h.update(&(path.len() as u64).to_le_bytes());
    h.update(path);
    h.update(&identity.size.to_le_bytes());
    h.update(&mtime_nanos(identity.mtime).to_le_bytes());
    h.update(&identity.content_hash.to_le_bytes());
    format!("{:032x}", h.digest128())
}

fn mtime_nanos(t: Option<SystemTime>) -> i128 {
    match t {
        None => i128::MIN,
        Some(t) => match t.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_nanos() as i128,
            Err(e) => -(e.duration().as_nanos() as i128),
        },
    }
}

fn stamp(doc: &Document, opts: &IndexOptions) -> format::Stamp {
    let id = doc.identity();
    format::Stamp {
        file_size: id.size,
        content_len: doc.len(),
        mtime_nanos: mtime_nanos(id.mtime),
        content_hash: id.content_hash,
        memory_budget: opts.budget_for(doc.len()),
    }
}

/// A unique temp file name in `dir` (unique per process and call).
pub(crate) fn tmp_path(dir: &Path, stem: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!("{stem}.{}.{n}.{TMP_EXT}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dir_is_an_absolute_index_dir() {
        if let Some(dir) = IndexCache::default_dir() {
            assert!(dir.is_absolute(), "{}", dir.display());
            assert!(dir.ends_with("index"));
            assert!(dir.to_string_lossy().contains("braceray"));
        }
    }

    #[test]
    fn mtime_encoding() {
        assert_eq!(mtime_nanos(None), i128::MIN);
        assert_eq!(
            mtime_nanos(Some(UNIX_EPOCH + std::time::Duration::from_nanos(5))),
            5
        );
        assert_eq!(
            mtime_nanos(Some(UNIX_EPOCH - std::time::Duration::from_nanos(5))),
            -5
        );
    }
}
