//! Heap counter for measuring "app memory excluding the mapped file" (step 12, I-008).
//!
//! Only with `--features alloc-stats` (off by default, never in release downloads): the
//! binary then uses [`Counting`], a pass-through wrapper around mimalloc that counts live
//! and peak heap bytes. Without the feature [`heap`] returns `None`. ADR 027.

/// `(live, peak)` heap bytes, or `None` when the app wasn't built with `alloc-stats`.
pub fn heap() -> Option<(u64, u64)> {
    #[cfg(feature = "alloc-stats")]
    {
        use std::sync::atomic::Ordering;
        Some((
            counting::LIVE.load(Ordering::Relaxed) as u64,
            counting::PEAK.load(Ordering::Relaxed) as u64,
        ))
    }
    #[cfg(not(feature = "alloc-stats"))]
    None
}

#[cfg(feature = "alloc-stats")]
pub use counting::Counting;

#[cfg(feature = "alloc-stats")]
mod counting {
    use std::alloc::{GlobalAlloc, Layout};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(super) static LIVE: AtomicUsize = AtomicUsize::new(0);
    pub(super) static PEAK: AtomicUsize = AtomicUsize::new(0);

    /// mimalloc plus live / peak byte counters.
    pub struct Counting;

    fn added(n: usize) {
        let now = LIVE.fetch_add(n, Ordering::Relaxed) + n;
        PEAK.fetch_max(now, Ordering::Relaxed);
    }

    fn removed(n: usize) {
        LIVE.fetch_sub(n, Ordering::Relaxed);
    }

    // SAFETY: every method forwards the caller's arguments unchanged to mimalloc's
    // `GlobalAlloc` implementation, so the caller's guarantees (valid layout, pointer
    // from this allocator with the same layout) carry over as they are; the counters
    // are plain atomics and never touch the memory.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            // SAFETY: forwarded unchanged (see the impl comment).
            let p = unsafe { mimalloc::MiMalloc.alloc(layout) };
            if !p.is_null() {
                added(layout.size());
            }
            p
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            // SAFETY: forwarded unchanged (see the impl comment).
            let p = unsafe { mimalloc::MiMalloc.alloc_zeroed(layout) };
            if !p.is_null() {
                added(layout.size());
            }
            p
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: forwarded unchanged (see the impl comment).
            unsafe { mimalloc::MiMalloc.dealloc(ptr, layout) };
            removed(layout.size());
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            // SAFETY: forwarded unchanged (see the impl comment).
            let p = unsafe { mimalloc::MiMalloc.realloc(ptr, layout, new_size) };
            if !p.is_null() {
                removed(layout.size());
                added(new_size);
            }
            p
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Covers every `unsafe` forward: sizes are added on alloc / realloc and removed on
        /// dealloc. (Other test threads use the system allocator in the test binary, so
        /// only this test touches the counters.)
        #[test]
        fn counts_live_and_peak_bytes() {
            let a = Counting;
            let before = LIVE.load(Ordering::Relaxed);
            let layout = Layout::from_size_align(1000, 8).unwrap();
            // SAFETY: non-zero size; each pointer is freed once with its current layout.
            unsafe {
                let p = a.alloc(layout);
                assert!(!p.is_null());
                assert_eq!(LIVE.load(Ordering::Relaxed), before + 1000);
                let p = a.realloc(p, layout, 5000);
                assert!(!p.is_null());
                assert_eq!(LIVE.load(Ordering::Relaxed), before + 5000);
                assert!(PEAK.load(Ordering::Relaxed) >= before + 5000);
                a.dealloc(p, Layout::from_size_align(5000, 8).unwrap());
                let z = a.alloc_zeroed(layout);
                assert!(!z.is_null() && *z == 0);
                a.dealloc(z, layout);
            }
            assert_eq!(LIVE.load(Ordering::Relaxed), before);
            assert!(super::super::heap().is_some());
        }
    }
}
