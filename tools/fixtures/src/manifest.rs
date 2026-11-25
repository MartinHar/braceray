//! Manifest types written next to fixtures (and read back by tests).

use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

/// Known needle and how often it occurs (byte-exact search counts).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NeedleCount {
    pub needle: String,
    /// Case-sensitive occurrences.
    pub count: u64,
    /// ASCII case-insensitive occurrences.
    pub count_case_insensitive: u64,
}

/// `<file>.manifest.json` for `orders` and `ndjson` fixtures.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordsManifest {
    pub file: String,
    pub kind: String,
    pub seed: u64,
    pub minified: bool,
    pub bytes: u64,
    pub records: u64,
    pub needles: Vec<NeedleCount>,
}

/// One entry of `invalid_manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvalidCase {
    pub file: String,
    /// Byte offset of the first offending byte (or the file length for an
    /// unexpected end of input).
    pub error_offset: u64,
    /// 1-based line of `error_offset`.
    pub line: u64,
    /// 1-based column (in bytes) of `error_offset`.
    pub column: u64,
    pub description: String,
}

/// One entry of `encoding_manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncodingCase {
    pub file: String,
    /// What the loader should conclude: `utf8`, `utf8-bom`, `utf16le`,
    /// `utf16be`, `utf32le`, `empty`, `whitespace-only`, `binary`.
    pub expect: String,
    pub description: String,
}

/// One known change between `diff_a` and `diff_b`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffChange {
    /// `value_changed`, `key_added` or `key_removed`.
    pub kind: String,
    /// JSONPath of the changed value, e.g. `$[123].total`.
    pub path: String,
    /// Value in A as JSON text (`None` if the key is absent in A).
    pub a: Option<String>,
    /// Value in B as JSON text (`None` if the key is absent in B).
    pub b: Option<String>,
}

/// `diff_manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffManifest {
    pub file_a: String,
    pub file_b: String,
    pub seed: u64,
    pub records: u64,
    pub changes: Vec<DiffChange>,
}

/// Writes any manifest as pretty JSON.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    text.push(b'\n');
    std::fs::write(path, text)
}

/// Path of the manifest that belongs to `file` (`<file>.manifest.json`).
pub fn manifest_path(file: &Path) -> std::path::PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".manifest.json");
    name.into()
}

/// File name as a string (for manifests).
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
