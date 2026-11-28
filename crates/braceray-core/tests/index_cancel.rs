//! Cancel latency of the index build. In its own test binary (I-011): cargo runs test
//! binaries one after another, so the proptests of `index_build.rs` can't load every core
//! while this measures.

use std::fs;
use std::time::{Duration, Instant};

use braceray_core::index::{self, IndexOptions};
use braceray_core::{CancelToken, Error, Progress};

#[test]
fn cancel_returns_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("orders.json");
    let opts = gen_fixtures::orders::RecordsOpts {
        size: 256 << 20,
        seed: 3,
        minified: false,
    };
    gen_fixtures::orders::generate_orders(&path, opts).unwrap();
    let bytes = fs::read(&path).unwrap();
    // The 50 ms target is for real machines; shared CI runners (3 slow vCPUs, debug
    // build, noisy neighbours) get headroom so the test catches "doesn't cancel", not noise.
    let limit = if std::env::var_os("CI").is_some() {
        Duration::from_millis(250)
    } else {
        Duration::from_millis(50)
    };
    for delay_ms in [0, 5, 30] {
        let cancel = CancelToken::new();
        let progress = Progress::default();
        let (result, latency) = std::thread::scope(|s| {
            let job = s
                .spawn(|| index::build_bytes(&bytes, &IndexOptions::default(), &cancel, &progress));
            std::thread::sleep(Duration::from_millis(delay_ms));
            let t = Instant::now();
            cancel.cancel();
            let r = job.join().unwrap();
            (r, t.elapsed())
        });
        // A very fast machine may finish before the cancel lands.
        if let Err(e) = result {
            assert!(matches!(e, Error::Cancelled));
        }
        assert!(latency < limit, "{delay_ms} ms: {latency:?}");
    }
}
