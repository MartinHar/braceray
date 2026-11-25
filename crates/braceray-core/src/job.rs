//! Cancellation and progress reporting shared between a long job and the UI.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::error::Error;

/// A cheap, clonable flag that asks a running job to stop.
///
/// # Example
/// ```
/// use braceray_core::CancelToken;
///
/// let token = CancelToken::new();
/// let for_job = token.clone();
/// token.cancel();
/// assert!(for_job.is_cancelled());
/// assert!(for_job.check().is_err());
/// ```
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A token that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks every holder of this token to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// `true` once [`cancel`](Self::cancel) was called.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// `Err(Error::Cancelled)` once cancelled, for use with `?` in job loops.
    pub fn check(&self) -> Result<(), Error> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Work done so far, in bytes (or any unit), out of a total. Updated by workers, read by
/// the UI.
///
/// # Example
/// ```
/// use braceray_core::Progress;
///
/// let p = Progress::new(200);
/// p.add(50);
/// assert_eq!(p.fraction(), 0.25);
/// ```
#[derive(Debug, Default)]
pub struct Progress {
    done: AtomicU64,
    total: AtomicU64,
}

impl Progress {
    /// Progress at 0 out of `total`.
    pub fn new(total: u64) -> Self {
        Self {
            done: AtomicU64::new(0),
            total: AtomicU64::new(total),
        }
    }

    /// Resets to 0 out of `total` (a job calls this when it starts).
    pub fn reset(&self, total: u64) {
        self.done.store(0, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
    }

    /// Adds `n` units of finished work.
    pub fn add(&self, n: u64) {
        self.done.fetch_add(n, Ordering::Relaxed);
    }

    /// Units done so far.
    pub fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    /// Total units.
    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// Done / total in `0.0..=1.0` (1.0 when the total is 0).
    pub fn fraction(&self) -> f32 {
        let total = self.total();
        if total == 0 {
            1.0
        } else {
            (self.done().min(total) as f64 / total as f64) as f32
        }
    }
}
