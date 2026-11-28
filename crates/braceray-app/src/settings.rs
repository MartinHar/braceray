//! Persisted settings and recent files: one small JSON file in the OS config dir.
//!
//! Loading never fails: a missing or unreadable file gives the defaults, and each field
//! that is missing, of the wrong type or out of range is reset to its default on its own.
//! Saving runs on a background thread (writes go to a temp file that is renamed over the
//! old one), so the UI thread never waits for the disk.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};

use braceray_core::cache::CacheSettings;
use braceray_core::path::{DEFAULT_JS_ROOT, PathFormat};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Most recent files kept.
pub const MAX_RECENT: usize = 15;
/// Searches kept in the history.
pub const MAX_HISTORY: usize = 20;

/// Light, dark or follow the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    /// Follow the OS setting.
    #[default]
    System,
    /// Always dark.
    Dark,
    /// Always light.
    Light,
}

impl ThemeChoice {
    /// All choices, in the order the UI shows them.
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Dark, ThemeChoice::Light];

    /// Label in the UI.
    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "System",
            ThemeChoice::Dark => "Dark",
            ThemeChoice::Light => "Light",
        }
    }
}

/// Everything the app persists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Colour theme.
    pub theme: ThemeChoice,
    /// Read and write the index cache.
    pub index_cache_enabled: bool,
    /// Size limit of the index cache.
    pub index_cache_max_bytes: u64,
    /// Files smaller than this aren't cached.
    pub index_cache_min_file_bytes: u64,
    /// Search results kept in memory (the count is always exact). Used from step 09.
    pub max_stored_results: u64,
    /// Children per group when a container is huge (`[0 … 999]`). Used from step 07.
    pub tree_group_size: u64,
    /// Format of "Copy path" (the breadcrumb button and Ctrl/Cmd+Shift+C): the last one
    /// chosen.
    pub path_format: PathFormat,
    /// Root name of JavaScript paths (`data.orders[0]`).
    pub js_root: String,
    /// Copying a value bigger than this asks first (offers "Export…").
    pub copy_limit_bytes: u64,
    /// Recently opened files, newest first.
    pub recent_files: Vec<PathBuf>,
    /// Recent search texts, newest first.
    pub search_history: Vec<String>,
}

/// Allowed range of [`Settings::index_cache_max_bytes`]: 64 MiB ..= 1 TiB.
pub const CACHE_MAX_RANGE: (u64, u64) = (64 << 20, 1 << 40);
/// Allowed range of [`Settings::index_cache_min_file_bytes`]: 0 ..= 100 GiB.
pub const CACHE_MIN_FILE_RANGE: (u64, u64) = (0, 100 << 30);
/// Allowed range of [`Settings::max_stored_results`].
pub const MAX_RESULTS_RANGE: (u64, u64) = (1_000, 100_000_000);
/// Allowed range of [`Settings::tree_group_size`].
pub const GROUP_SIZE_RANGE: (u64, u64) = (10, 1_000_000);
/// Allowed range of [`Settings::copy_limit_bytes`]: 1 MiB ..= 64 GiB.
pub const COPY_LIMIT_RANGE: (u64, u64) = (1 << 20, 64 << 30);
/// Default of [`Settings::copy_limit_bytes`]: 50 MB.
pub const DEFAULT_COPY_LIMIT: u64 = 50 << 20;
/// Longest accepted [`Settings::js_root`].
pub const MAX_JS_ROOT: usize = 64;

impl Default for Settings {
    fn default() -> Self {
        let cache = CacheSettings::default();
        Settings {
            theme: ThemeChoice::System,
            index_cache_enabled: cache.index_cache_enabled,
            index_cache_max_bytes: cache.index_cache_max_bytes,
            index_cache_min_file_bytes: cache.index_cache_min_file_bytes,
            max_stored_results: 5_000_000,
            tree_group_size: 1000,
            path_format: PathFormat::JsonPath,
            js_root: DEFAULT_JS_ROOT.to_owned(),
            copy_limit_bytes: DEFAULT_COPY_LIMIT,
            recent_files: Vec::new(),
            search_history: Vec::new(),
        }
    }
}

impl Settings {
    /// Parses settings JSON. Bad or missing fields get their defaults (never fails).
    pub fn from_json(text: &str) -> Settings {
        let obj = match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(obj)) => obj,
            Ok(_) | Err(_) => {
                tracing::warn!("settings: not a JSON object, using defaults");
                Map::new()
            }
        };
        let d = Settings::default();
        let mut s = Settings {
            theme: field(&obj, "theme", d.theme),
            index_cache_enabled: field(&obj, "index_cache_enabled", d.index_cache_enabled),
            index_cache_max_bytes: field(&obj, "index_cache_max_bytes", d.index_cache_max_bytes),
            index_cache_min_file_bytes: field(
                &obj,
                "index_cache_min_file_bytes",
                d.index_cache_min_file_bytes,
            ),
            max_stored_results: field(&obj, "max_stored_results", d.max_stored_results),
            tree_group_size: field(&obj, "tree_group_size", d.tree_group_size),
            path_format: field(&obj, "path_format", d.path_format),
            js_root: field(&obj, "js_root", d.js_root),
            copy_limit_bytes: field(&obj, "copy_limit_bytes", d.copy_limit_bytes),
            recent_files: field(&obj, "recent_files", d.recent_files),
            search_history: field(&obj, "search_history", d.search_history),
        };
        s.validate();
        s
    }

    /// Serializes to pretty JSON.
    pub fn to_json(&self) -> String {
        // Serializing plain fields (bool, u64, strings, paths) can't fail; fall back to an
        // empty object, which loads as the defaults, just in case.
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Resets out-of-range values to their defaults and trims the recent list.
    pub fn validate(&mut self) {
        let d = Settings::default();
        clamp_or_default(
            &mut self.index_cache_max_bytes,
            CACHE_MAX_RANGE,
            d.index_cache_max_bytes,
        );
        clamp_or_default(
            &mut self.index_cache_min_file_bytes,
            CACHE_MIN_FILE_RANGE,
            d.index_cache_min_file_bytes,
        );
        clamp_or_default(
            &mut self.max_stored_results,
            MAX_RESULTS_RANGE,
            d.max_stored_results,
        );
        clamp_or_default(
            &mut self.tree_group_size,
            GROUP_SIZE_RANGE,
            d.tree_group_size,
        );
        clamp_or_default(
            &mut self.copy_limit_bytes,
            COPY_LIMIT_RANGE,
            d.copy_limit_bytes,
        );
        if !valid_js_root(&self.js_root) {
            tracing::warn!(
                "settings: bad js_root {:?}, using {:?}",
                self.js_root,
                d.js_root
            );
            self.js_root = d.js_root;
        }
        let mut seen = Vec::with_capacity(self.recent_files.len());
        self.recent_files.retain(|p| {
            let keep = !p.as_os_str().is_empty() && !seen.contains(p);
            if keep {
                seen.push(p.clone());
            }
            keep
        });
        self.recent_files.truncate(MAX_RECENT);
        let mut seen: Vec<String> = Vec::with_capacity(self.search_history.len());
        self.search_history.retain(|t| {
            let keep = !t.is_empty() && !seen.contains(t);
            if keep {
                seen.push(t.clone());
            }
            keep
        });
        self.search_history.truncate(MAX_HISTORY);
    }

    /// Moves `text` to the front of the search history. Returns `true` if it changed.
    pub fn push_history(&mut self, text: &str) -> bool {
        if text.is_empty() || self.search_history.first().is_some_and(|t| t == text) {
            return false;
        }
        self.search_history.retain(|t| t != text);
        self.search_history.insert(0, text.to_owned());
        self.search_history.truncate(MAX_HISTORY);
        true
    }

    /// The index cache part.
    pub fn cache_settings(&self) -> CacheSettings {
        CacheSettings {
            index_cache_enabled: self.index_cache_enabled,
            index_cache_max_bytes: self.index_cache_max_bytes,
            index_cache_min_file_bytes: self.index_cache_min_file_bytes,
        }
    }

    /// Moves `path` to the front of the recent list.
    pub fn push_recent(&mut self, path: &Path) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(MAX_RECENT);
    }

    /// Removes `path` from the recent list.
    pub fn remove_recent(&mut self, path: &Path) {
        self.recent_files.retain(|p| p != path);
    }
}

/// A JavaScript root name: an expression start such as `data`, `window.data` or
/// `$json` (identifier characters and dots, not starting with a digit or dot).
pub fn valid_js_root(root: &str) -> bool {
    let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '.');
    root.len() <= MAX_JS_ROOT
        && root
            .chars()
            .next()
            .is_some_and(|c| ok(c) && !c.is_ascii_digit() && c != '.')
        && root.chars().all(ok)
        && !root.ends_with('.')
        && !root.contains("..")
}

fn field<T: serde::de::DeserializeOwned>(obj: &Map<String, Value>, key: &str, default: T) -> T {
    match obj.get(key) {
        None => default,
        Some(v) => serde_json::from_value(v.clone()).unwrap_or_else(|e| {
            tracing::warn!("settings: bad value for {key} ({e}), using the default");
            default
        }),
    }
}

fn clamp_or_default(value: &mut u64, (lo, hi): (u64, u64), default: u64) {
    if !(lo..=hi).contains(value) {
        tracing::warn!("settings: value {value} out of range {lo}..={hi}, using {default}");
        *value = default;
    }
}

/// The settings file: `~/Library/Application Support/io.github.martinhar.braceray/` on
/// macOS, `%APPDATA%\martinhar\braceray\config\` on Windows, `$XDG_CONFIG_HOME/braceray/`
/// (or `~/.config/braceray/`) elsewhere, plus `settings.json`. Same places as
/// `directories::ProjectDirs::config_dir` (ADR 014). `None` without a home directory.
pub fn default_path() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let dir = if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support/io.github.martinhar.braceray")
    } else if cfg!(windows) {
        env("APPDATA")?
            .join("martinhar")
            .join("braceray")
            .join("config")
    } else {
        env("XDG_CONFIG_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| env("HOME").map(|h| h.join(".config")))?
            .join("braceray")
    };
    Some(dir.join("settings.json"))
}

enum SaveMsg {
    Write(String),
    Flush(Sender<()>),
}

/// Where settings are loaded from and saved to.
#[derive(Debug)]
pub struct SettingsStore {
    path: Option<PathBuf>,
    tx: Option<Sender<SaveMsg>>,
}

impl SettingsStore {
    /// A store at `path`; `None` keeps settings in memory only.
    pub fn new(path: Option<PathBuf>) -> Self {
        let tx = path.clone().and_then(|path| {
            let (tx, rx) = mpsc::channel::<SaveMsg>();
            let spawned = std::thread::Builder::new()
                .name("settings-saver".to_owned())
                .spawn(move || {
                    while let Ok(msg) = rx.recv() {
                        match msg {
                            SaveMsg::Write(text) => {
                                if let Err(e) = write_atomic(&path, &text) {
                                    tracing::warn!("settings: can't save {}: {e}", path.display());
                                }
                            }
                            SaveMsg::Flush(done) => {
                                let _ = done.send(());
                            }
                        }
                    }
                });
            match spawned {
                Ok(_) => Some(tx),
                Err(e) => {
                    tracing::warn!("settings: can't start the saver thread: {e}");
                    None
                }
            }
        });
        SettingsStore { path, tx }
    }

    /// The settings file, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Reads the settings (defaults when the file is missing or broken). Small file; called
    /// once at startup.
    pub fn load(&self) -> Settings {
        let Some(path) = &self.path else {
            return Settings::default();
        };
        match std::fs::read_to_string(path) {
            Ok(text) => Settings::from_json(&text),
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("settings: can't read {}: {e}", path.display());
                }
                Settings::default()
            }
        }
    }

    /// Saves in the background (in call order).
    pub fn save(&self, settings: &Settings) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(SaveMsg::Write(settings.to_json()));
        }
    }

    /// Waits until every earlier [`save`](Self::save) is on disk (tests, app exit).
    pub fn flush(&self) {
        if let Some(tx) = &self.tx {
            let (done_tx, done_rx) = mpsc::channel();
            if tx.send(SaveMsg::Flush(done_tx)).is_ok() {
                let _ = done_rx.recv();
            }
        }
    }
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut s = Settings {
            theme: ThemeChoice::Light,
            index_cache_enabled: false,
            tree_group_size: 500,
            path_format: PathFormat::Jq,
            js_root: "window.doc".to_owned(),
            copy_limit_bytes: 8 << 20,
            ..Settings::default()
        };
        s.push_recent(Path::new("/a.json"));
        s.push_recent(Path::new("/b.json"));
        assert_eq!(Settings::from_json(&s.to_json()), s);
    }

    #[test]
    fn bad_values_reset_one_by_one() {
        let s = Settings::from_json(
            r#"{"theme": "purple", "index_cache_enabled": false, "index_cache_max_bytes": 5,
                "tree_group_size": "big", "max_stored_results": 2000, "extra": 1}"#,
        );
        let d = Settings::default();
        assert_eq!(s.theme, d.theme);
        assert!(!s.index_cache_enabled);
        assert_eq!(s.index_cache_max_bytes, d.index_cache_max_bytes);
        assert_eq!(s.tree_group_size, d.tree_group_size);
        assert_eq!(s.max_stored_results, 2000);
        assert_eq!(Settings::from_json("not json"), d);
        assert_eq!(Settings::from_json("[1, 2]"), d);
        assert_eq!(Settings::from_json(""), d);
    }

    #[test]
    fn copy_settings_are_validated() {
        let s = Settings::from_json(
            r#"{"path_format": "javascript", "js_root": "1bad", "copy_limit_bytes": 12}"#,
        );
        let d = Settings::default();
        assert_eq!(s.path_format, PathFormat::JavaScript);
        assert_eq!(s.js_root, d.js_root);
        assert_eq!(s.copy_limit_bytes, d.copy_limit_bytes);
        let s = Settings::from_json(r#"{"path_format": "xpath"}"#);
        assert_eq!(s.path_format, PathFormat::JsonPath);
        for ok in ["data", "$json", "window.data", "_", "a1"] {
            assert!(valid_js_root(ok), "{ok}");
        }
        for bad in ["", "1a", ".a", "a.", "a..b", "a b", "a[0]", "a;alert(1)"] {
            assert!(!valid_js_root(bad), "{bad}");
        }
        assert!(!valid_js_root(&"a".repeat(MAX_JS_ROOT + 1)));
    }

    #[test]
    fn search_history_is_capped_deduplicated_and_persisted() {
        let mut s = Settings::default();
        for i in 0..30 {
            s.push_history(&format!("q{i}"));
        }
        assert!(s.push_history("q5"));
        assert!(!s.push_history("q5"));
        assert!(!s.push_history(""));
        assert_eq!(s.search_history.len(), MAX_HISTORY);
        assert_eq!(s.search_history[0], "q5");
        assert_eq!(s.search_history[1], "q29");
        let back = Settings::from_json(&s.to_json());
        assert_eq!(back.search_history, s.search_history);
        let bad = Settings::from_json(r#"{"search_history": ["a", "", "a", "b"]}"#);
        assert_eq!(bad.search_history, ["a", "b"]);
    }

    #[test]
    fn recent_list_is_capped_and_deduplicated() {
        let mut s = Settings::default();
        for i in 0..20 {
            s.push_recent(Path::new(&format!("/f{i}.json")));
        }
        s.push_recent(Path::new("/f10.json"));
        assert_eq!(s.recent_files.len(), MAX_RECENT);
        assert_eq!(s.recent_files[0], Path::new("/f10.json"));
        assert_eq!(
            s.recent_files
                .iter()
                .filter(|p| p.ends_with("f10.json"))
                .count(),
            1
        );

        let json = r#"{"recent_files": ["/a", "/a", "", "/b"]}"#;
        assert_eq!(
            Settings::from_json(json).recent_files,
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    #[test]
    fn store_saves_in_background_and_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("settings.json");
        let store = SettingsStore::new(Some(path.clone()));
        assert_eq!(store.load(), Settings::default());
        let s = Settings {
            theme: ThemeChoice::Dark,
            ..Settings::default()
        };
        store.save(&Settings::default());
        store.save(&s);
        store.flush();
        assert_eq!(SettingsStore::new(Some(path)).load(), s);
        let leftovers = std::fs::read_dir(dir.path().join("sub")).unwrap().count();
        assert_eq!(leftovers, 1, "no temp files left");
    }

    #[test]
    fn default_path_is_absolute() {
        if let Some(p) = default_path() {
            assert!(p.is_absolute());
            assert!(p.ends_with("settings.json"));
        }
    }
}
