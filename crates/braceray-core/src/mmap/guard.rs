//! SIGBUS guard for mapped files (Unix only, ADR 028).
//!
//! When another process truncates a file while we have it mapped, reading a page past the
//! new end raises `SIGBUS` and would kill the app (I/O errors on removed or network volumes
//! do the same). Every mapping registers its address range here. A process-wide `SIGBUS`
//! handler checks whether the faulting address lies in a registered range; if it does, it
//! maps anonymous zero pages over the rest of that range (`MAP_FIXED`) and marks the
//! mapping as faulted, so the faulting read is retried and sees zeros. The document then
//! reports itself as changed on disk and the app offers a reload. Faults outside our
//! ranges go to the handler that was installed before ours (Rust's stack-overflow handler)
//! or to the default action.

use std::ptr;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

/// Number of mappings that can be guarded at the same time (open documents + cache entries).
const SLOTS: usize = 256;

/// One registered range. `start` / `len` are published under a seqlock (`seq` odd while a
/// writer changes them), so the signal handler only ever uses a pair written together:
/// a slot released and reused by another mapping between its two loads is detected and
/// skipped instead of mixing one mapping's start with another's length.
struct Slot {
    /// Claimed by a `Guard` (only the owner writes `start` / `len`).
    owned: AtomicBool,
    /// Seqlock counter: odd while `start` / `len` are being written.
    seq: AtomicUsize,
    /// Start address; 0 = no range.
    start: AtomicUsize,
    /// Length in bytes; 0 = no range.
    len: AtomicUsize,
    /// A fault in this range was turned into zero pages.
    faulted: AtomicBool,
}

impl Slot {
    /// Writes `start` / `len` under the seqlock. Only the slot's owner calls this.
    fn publish(&self, start: usize, len: usize) {
        self.seq.fetch_add(1, Ordering::SeqCst); // odd: readers skip
        self.start.store(start, Ordering::SeqCst);
        self.len.store(len, Ordering::SeqCst);
        self.seq.fetch_add(1, Ordering::SeqCst); // even: consistent again
    }

    /// A consistent `(start, len)` snapshot, or `None` while empty or being written.
    /// Async-signal-safe (atomic loads only).
    fn snapshot(&self) -> Option<(usize, usize)> {
        let before = self.seq.load(Ordering::SeqCst);
        if before % 2 == 1 {
            return None;
        }
        let start = self.start.load(Ordering::SeqCst);
        let len = self.len.load(Ordering::SeqCst);
        let after = self.seq.load(Ordering::SeqCst);
        (before == after && start != 0 && len != 0).then_some((start, len))
    }
}

static REGISTRY: [Slot; SLOTS] = [const {
    Slot {
        owned: AtomicBool::new(false),
        seq: AtomicUsize::new(0),
        start: AtomicUsize::new(0),
        len: AtomicUsize::new(0),
        faulted: AtomicBool::new(false),
    }
}; SLOTS];

static PAGE: AtomicUsize = AtomicUsize::new(0);
static PREVIOUS: AtomicPtr<libc::sigaction> = AtomicPtr::new(ptr::null_mut());
static INSTALL: Once = Once::new();

/// A registered mapping. Unregisters on drop; drop it **before** the mapping is unmapped.
#[derive(Debug)]
pub(crate) struct Guard {
    slot: usize,
}

impl Guard {
    /// Registers `[start, start + len)`. `None` if every slot is taken (the mapping then
    /// works as before, just without the guard) or the handler couldn't be installed.
    pub(crate) fn register(start: *const u8, len: usize) -> Option<Guard> {
        if len == 0 || !install() {
            return None;
        }
        let start = start as usize;
        for (i, slot) in REGISTRY.iter().enumerate() {
            if slot
                .owned
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                slot.faulted.store(false, Ordering::SeqCst);
                slot.publish(start, len);
                return Some(Guard { slot: i });
            }
        }
        tracing::warn!("SIGBUS guard: all {SLOTS} slots in use; mapping not guarded");
        None
    }

    /// `true` once a read in this mapping faulted and was replaced by zero pages.
    pub(crate) fn faulted(&self) -> bool {
        REGISTRY[self.slot].faulted.load(Ordering::Acquire)
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let slot = &REGISTRY[self.slot];
        slot.publish(0, 0);
        // Only now can another mapping claim the slot.
        slot.owned.store(false, Ordering::SeqCst);
    }
}

/// Installs the handler once. `false` if `sigaction` failed.
fn install() -> bool {
    INSTALL.call_once(|| {
        // SAFETY: `sysconf` has no preconditions.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        PAGE.store(
            usize::try_from(page).unwrap_or(4096).max(1),
            Ordering::Relaxed,
        );
        // SAFETY: `sigaction` is plain old data; all-zero is a valid empty value.
        let mut new: libc::sigaction = unsafe { std::mem::zeroed() };
        new.sa_sigaction = on_sigbus as *const () as libc::sighandler_t;
        // SA_ONSTACK: run on the alternate stack Rust sets up per thread (stack overflows).
        new.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
        let previous = Box::into_raw(Box::new(
            // SAFETY: as above.
            unsafe { std::mem::zeroed::<libc::sigaction>() },
        ));
        // SAFETY: `new` is initialised, `previous` points to writable memory that lives
        // for the rest of the process (leaked on purpose: the handler reads it).
        let ok = unsafe {
            libc::sigemptyset(&mut new.sa_mask);
            libc::sigaction(libc::SIGBUS, &new, previous) == 0
        };
        if ok {
            PREVIOUS.store(previous, Ordering::Release);
        } else {
            tracing::warn!("SIGBUS guard: sigaction failed; mappings not guarded");
            // SAFETY: allocated by `Box::into_raw` above and never shared.
            drop(unsafe { Box::from_raw(previous) });
        }
    });
    !PREVIOUS.load(Ordering::Acquire).is_null()
}

/// The `SIGBUS` handler. Only async-signal-safe work: atomics, `mmap`, `sigaction`.
extern "C" fn on_sigbus(sig: libc::c_int, info: *mut libc::siginfo_t, ctx: *mut libc::c_void) {
    // SAFETY: the kernel passes a valid `siginfo_t` to an `SA_SIGINFO` handler.
    let addr = unsafe { (*info).si_addr() } as usize;
    if zero_fill(addr) {
        return; // The faulting instruction is retried and reads zeros.
    }
    forward(sig, info, ctx);
}

/// Maps zero pages from the page of `addr` to the end of its registered range.
fn zero_fill(addr: usize) -> bool {
    let page = PAGE.load(Ordering::Relaxed);
    for slot in &REGISTRY {
        // `start` and `len` come from one consistent snapshot, and the range to replace is
        // computed from that snapshot only.
        let Some((start, len)) = slot.snapshot() else {
            continue;
        };
        if addr < start || addr - start >= len {
            continue;
        }
        let from = addr & !(page - 1);
        let end = (start + len).div_ceil(page) * page;
        // SAFETY: `[from, end)` lies inside a live read-only file mapping we own: the
        // snapshot is consistent (seqlock), so `start..start + len` is one registered range;
        // ranges of live mappings don't overlap, and `addr` is in it because a thread is
        // reading that mapping right now, so it can't be unregistered or unmapped meanwhile. Replacing those pages with private anonymous ones only
        // changes what later reads see (zeros instead of a fault); nothing in Braceray
        // holds a mutable reference into the mapping. `munmap` on drop still covers the
        // whole range.
        let r = unsafe {
            libc::mmap(
                from as *mut libc::c_void,
                end - from,
                libc::PROT_READ,
                libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_FIXED,
                -1,
                0,
            )
        };
        if r == libc::MAP_FAILED {
            return false;
        }
        slot.faulted.store(true, Ordering::Release);
        return true;
    }
    false
}

/// Hands a fault we don't own to the previous handler, or restores the default action so
/// the retried instruction terminates the process as it would have without us.
fn forward(sig: libc::c_int, info: *mut libc::siginfo_t, ctx: *mut libc::c_void) {
    let previous = PREVIOUS.load(Ordering::Acquire);
    // SAFETY: `previous` was filled by `sigaction` in `install` and is never freed.
    let prev = unsafe { &*previous };
    let handler = prev.sa_sigaction;
    if handler != libc::SIG_DFL && handler != libc::SIG_IGN {
        if prev.sa_flags & libc::SA_SIGINFO != 0 {
            // SAFETY: with SA_SIGINFO the previous handler has this three-argument signature.
            let f: extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void) =
                unsafe { std::mem::transmute(handler) };
            f(sig, info, ctx);
        } else {
            // SAFETY: without SA_SIGINFO the previous handler takes only the signal number.
            let f: extern "C" fn(libc::c_int) = unsafe { std::mem::transmute(handler) };
            f(sig);
        }
        return;
    }
    // SAFETY: an all-zero `sigaction` with SIG_DFL (0) is valid; `sigaction` is
    // async-signal-safe.
    unsafe {
        let mut dfl: libc::sigaction = std::mem::zeroed();
        dfl.sa_sigaction = libc::SIG_DFL;
        libc::sigaction(sig, &dfl, ptr::null_mut());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_reused() {
        let buf = [0u8; 16];
        for _ in 0..(SLOTS * 4) {
            let g = Guard::register(buf.as_ptr(), buf.len()).unwrap();
            assert!(!g.faulted());
        }
    }

    #[test]
    fn snapshots_are_consistent_while_slots_churn() {
        use std::sync::Arc;
        let stop = Arc::new(AtomicBool::new(false));
        let writers: Vec<_> = (0..4)
            .map(|t| {
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    let buf = vec![0u8; 4096 * (t + 1)];
                    while !stop.load(Ordering::Relaxed) {
                        drop(Guard::register(buf.as_ptr(), buf.len()));
                    }
                    (buf.as_ptr() as usize, buf.len())
                })
            })
            .collect();
        // Every snapshot must be a (start, len) pair that some writer registered together.
        let mut seen = Vec::new();
        for _ in 0..200_000 {
            for slot in &REGISTRY {
                if let Some(pair) = slot.snapshot() {
                    seen.push(pair);
                }
            }
            if seen.len() > 50_000 {
                break;
            }
        }
        stop.store(true, Ordering::Relaxed);
        let pairs: Vec<_> = writers.into_iter().map(|w| w.join().unwrap()).collect();
        for p in seen {
            // Other tests in this binary may register their own ranges; only pairs with a
            // writer's start must carry that writer's length.
            if let Some(w) = pairs.iter().find(|w| w.0 == p.0) {
                assert_eq!(w.1, p.1, "torn snapshot");
            }
        }
    }

    #[test]
    fn empty_ranges_are_not_registered() {
        assert!(Guard::register(ptr::null(), 0).is_none());
    }
}
