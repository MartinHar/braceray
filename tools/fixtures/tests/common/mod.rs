//! Shared helpers for fixture integration tests.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// A fresh temp directory, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        gen_fixtures::records::QUIET.store(true, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "genfix-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn read(p: &std::path::Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Parses `bytes` as one JSON value without building a DOM. Runs on a thread
/// with a large stack and no recursion limit so the 100k-deep fixture works.
pub fn parse_ok(bytes: Vec<u8>) -> Result<(), String> {
    std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(move || {
            let mut de = serde_json::Deserializer::from_slice(&bytes);
            de.disable_recursion_limit();
            <serde::de::IgnoredAny as serde::Deserialize>::deserialize(&mut de)
                .map_err(|e| e.to_string())?;
            de.end().map_err(|e| e.to_string())
        })
        .unwrap()
        .join()
        .unwrap()
}

/// Naive byte-exact substring count (overlaps can't happen for our needles).
pub fn count(hay: &[u8], needle: &[u8]) -> u64 {
    hay.windows(needle.len()).filter(|w| *w == needle).count() as u64
}

/// ASCII case-insensitive naive count.
pub fn count_ci(hay: &[u8], needle: &[u8]) -> u64 {
    hay.windows(needle.len())
        .filter(|w| w.eq_ignore_ascii_case(needle))
        .count() as u64
}
