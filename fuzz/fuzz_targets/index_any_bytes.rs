//! Arbitrary bytes → build the index. Must not panic, the parallel builder must equal the
//! reference (with tiny and default chunks), and validity must agree with serde_json.
#![no_main]

use braceray_core::index::{self, IndexOptions, scalar};
use braceray_core::nav::Navigator;
use braceray_core::{CancelToken, Progress};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let cancel = CancelToken::new();
    let progress = Progress::default();
    let Ok(reference) = scalar::build(data, None, &cancel, &progress) else {
        return;
    };
    for chunk_size in [64, 65, 4096, index::DEFAULT_CHUNK_SIZE] {
        let opts = IndexOptions {
            chunk_size,
            threads: Some(2),
            ..IndexOptions::default()
        };
        let fast = index::build_bytes(data, &opts, &cancel, &progress).expect("not cancelled");
        assert_eq!(fast, reference, "chunk size {chunk_size}");
    }
    let serde_ok = serde_json::from_slice::<serde::de::IgnoredAny>(data).is_ok();
    assert_eq!(
        reference.is_valid(),
        serde_ok,
        "validity differs from serde_json"
    );

    // Navigation must not panic either, valid or not.
    let nav = Navigator::new(data, &reference);
    if let Some(root) = nav.root_container() {
        let n = nav.child_count(&root);
        for child in nav.children(&root, 0..n.min(64)) {
            if let Some(c) = nav.container_of(&child) {
                let _ = nav.children(&c, 0..8);
            }
        }
    }
    for off in [0, data.len() / 3, data.len() / 2, data.len()] {
        let loc = nav.node_at_offset(off as u64);
        let _ = nav.path_of(&loc);
    }
});
