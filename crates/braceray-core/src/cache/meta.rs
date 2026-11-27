//! `index.json`: per-entry metadata (original path, size, last access) and LRU eviction.
//!
//! The metadata is advisory. Sizes always come from the directory listing, and an entry
//! without metadata uses its file's mtime as last access. Several app instances may update
//! it at once (temp file + rename): the worst case is a lost `last_access` update.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{ENTRY_EXT, TMP_EXT, tmp_path};

/// File name of the metadata file inside the cache dir.
pub(crate) const META_FILE: &str = "index.json";

/// Temp files older than this are left over from a crashed writer and get deleted.
const STALE_TMP: Duration = Duration::from_secs(3600);

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Meta {
    #[serde(default)]
    pub(crate) entries: BTreeMap<String, MetaEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MetaEntry {
    /// The indexed file (for display; lossy UTF-8).
    pub(crate) path: String,
    /// Size of the `.brx` file.
    pub(crate) size: u64,
    /// Milliseconds since the Unix epoch.
    pub(crate) last_access: u64,
}

/// Milliseconds since the Unix epoch.
pub(crate) fn now_millis() -> u64 {
    millis(SystemTime::now())
}

fn millis(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

impl Meta {
    /// Reads `index.json`; a missing or unreadable file gives empty metadata.
    pub(crate) fn load(dir: &Path) -> Meta {
        match fs::read(dir.join(META_FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!("index cache: ignoring unreadable {META_FILE}: {e}");
                Meta::default()
            }),
            Err(_) => Meta::default(),
        }
    }

    /// Writes `index.json` atomically (best effort: errors are logged).
    pub(crate) fn save(&self, dir: &Path) {
        let result = serde_json::to_vec_pretty(self)
            .map_err(std::io::Error::other)
            .and_then(|json| {
                let tmp = tmp_path(dir, "index-json");
                fs::write(&tmp, json)?;
                fs::rename(&tmp, dir.join(META_FILE)).inspect_err(|_| {
                    let _ = fs::remove_file(&tmp);
                })
            });
        if let Err(e) = result {
            tracing::warn!("index cache: can't write {META_FILE}: {e}");
        }
    }

    /// Records an access to `key` (and its size and source path when known).
    pub(crate) fn touch(dir: &Path, key: &str, path: Option<&Path>, size: Option<u64>) {
        let mut meta = Meta::load(dir);
        let now = now_millis();
        let entry = meta.entries.entry(key.to_owned()).or_insert(MetaEntry {
            path: String::new(),
            size: 0,
            last_access: now,
        });
        entry.last_access = now;
        if let Some(p) = path {
            entry.path = p.to_string_lossy().into_owned();
        }
        if let Some(s) = size {
            entry.size = s;
        }
        meta.save(dir);
    }
}

/// One `.brx` file found in the cache dir.
struct Found {
    key: String,
    size: u64,
    last_access: u64,
}

/// Deletes least-recently-used entries until the total size of the `.brx` files is at most
/// `max_bytes`; also deletes temp files left by crashed writers and drops metadata of
/// missing entries. Returns the bytes freed.
pub(crate) fn evict(dir: &Path, max_bytes: u64) -> u64 {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return 0;
    };
    let mut meta = Meta::load(dir);
    let mut found = Vec::new();
    let mut freed = 0;
    for entry in read_dir.flatten() {
        let path = entry.path();
        let Ok(md) = entry.metadata() else { continue };
        let ext = path.extension().and_then(|e| e.to_str());
        if ext == Some(TMP_EXT) {
            let old = md
                .modified()
                .ok()
                .and_then(|m| m.elapsed().ok())
                .is_some_and(|age| age > STALE_TMP);
            if old && fs::remove_file(&path).is_ok() {
                freed += md.len();
            }
        } else if ext == Some(ENTRY_EXT) {
            let Some(key) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let last_access = meta
                .entries
                .get(key)
                .map(|e| e.last_access)
                .or_else(|| md.modified().ok().map(millis))
                .unwrap_or(0);
            found.push(Found {
                key: key.to_owned(),
                size: md.len(),
                last_access,
            });
        }
    }

    let mut total: u64 = found.iter().map(|f| f.size).sum();
    found.sort_by(|a, b| (a.last_access, &a.key).cmp(&(b.last_access, &b.key)));
    let mut kept = BTreeMap::new();
    for f in found {
        if total > max_bytes {
            let path = dir.join(format!("{}.{ENTRY_EXT}", f.key));
            match fs::remove_file(&path) {
                Ok(()) => {
                    tracing::debug!("index cache: evicted {} ({} bytes)", f.key, f.size);
                    total -= f.size;
                    freed += f.size;
                    continue;
                }
                // Mapped by another process on Windows, for example: keep it for now.
                Err(e) => tracing::debug!("index cache: can't evict {}: {e}", f.key),
            }
        }
        let mut entry = meta.entries.remove(&f.key).unwrap_or(MetaEntry {
            path: String::new(),
            size: f.size,
            last_access: f.last_access,
        });
        entry.size = f.size;
        kept.insert(f.key, entry);
    }
    meta.entries = kept;
    meta.save(dir);
    freed
}
