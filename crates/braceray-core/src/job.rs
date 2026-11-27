//! Cancellation, progress and background jobs shared between long work and the UI
//! (architecture.md §5).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

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

/// Called after a job posts an event, so the UI wakes up and polls (the app passes
/// `ctx.request_repaint()`; core stays UI-free).
pub type Repaint = Arc<dyn Fn() + Send + Sync>;

/// A [`Repaint`] that does nothing (tests, command-line tools).
pub fn no_repaint() -> Repaint {
    Arc::new(|| {})
}

/// What a job reports to its owner.
#[derive(Debug)]
pub enum JobEvent<P, T> {
    /// An intermediate result; more events follow.
    Partial(P),
    /// The final result. No events follow.
    Done(T),
    /// The job failed. No events follow.
    Failed(Error),
    /// The job stopped because its token was cancelled. No events follow.
    Cancelled,
}

/// The worker side of a job: post partial results and check for cancellation.
pub struct JobContext<P> {
    cancel: CancelToken,
    progress: Arc<Progress>,
    tx: Sender<Message<P>>,
    repaint: Repaint,
}

impl<P> JobContext<P> {
    /// The job's cancellation token.
    pub fn cancel(&self) -> &CancelToken {
        &self.cancel
    }

    /// The job's progress, shared with the UI.
    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    /// Posts an intermediate result and wakes the UI. Ignored if the handle was dropped.
    pub fn partial(&self, value: P) {
        if self.tx.send(Message::Partial(value)).is_ok() {
            (self.repaint)();
        }
    }
}

enum Message<P> {
    Partial(P),
    End,
}

/// The UI side of a running job: poll it each frame with [`try_recv`](Self::try_recv).
///
/// Dropping the handle cancels the job.
///
/// # Example
/// ```
/// use braceray_core::job::{JobEvent, JobHandle, no_repaint};
///
/// let mut job = JobHandle::spawn("sum", 100, no_repaint(), |ctx| {
///     ctx.partial("half way");
///     ctx.progress().add(100);
///     Ok(42)
/// });
/// let mut events = Vec::new();
/// while let Some(event) = job.wait() {
///     events.push(event);
/// }
/// assert!(matches!(events[0], JobEvent::Partial("half way")));
/// assert!(matches!(events[1], JobEvent::Done(42)));
/// assert_eq!(job.progress().fraction(), 1.0);
/// ```
pub struct JobHandle<P, T> {
    cancel: CancelToken,
    progress: Arc<Progress>,
    rx: Receiver<Message<P>>,
    result: Arc<std::sync::Mutex<Option<JobEvent<P, T>>>>,
    started: Instant,
    finished: bool,
}

impl<P: Send + 'static, T: Send + 'static> JobHandle<P, T> {
    /// Runs `work` on a new thread named `name`. `total` initializes the progress (the job
    /// may [`reset`](Progress::reset) it). `Err(Error::Cancelled)` from `work`, or any
    /// result after the token was cancelled, ends as [`JobEvent::Cancelled`].
    pub fn spawn<F>(name: &str, total: u64, repaint: Repaint, work: F) -> Self
    where
        F: FnOnce(&JobContext<P>) -> Result<T, Error> + Send + 'static,
    {
        let cancel = CancelToken::new();
        let progress = Arc::new(Progress::new(total));
        let (tx, rx) = mpsc::channel();
        let result = Arc::new(std::sync::Mutex::new(None));
        let ctx = JobContext {
            cancel: cancel.clone(),
            progress: progress.clone(),
            tx,
            repaint,
        };
        let slot = result.clone();
        let body = move || {
            let outcome = work(&ctx);
            let event = match outcome {
                _ if ctx.cancel.is_cancelled() => JobEvent::Cancelled,
                Ok(value) => JobEvent::Done(value),
                Err(Error::Cancelled) => JobEvent::Cancelled,
                Err(e) => JobEvent::Failed(e),
            };
            if let Ok(mut slot) = slot.lock() {
                *slot = Some(event);
            }
            // The receiver may be gone (handle dropped); nothing to do then.
            let _ = ctx.tx.send(Message::End);
            (ctx.repaint)();
        };
        let spawned = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(body);
        let mut handle = JobHandle {
            cancel,
            progress,
            rx,
            result,
            started: Instant::now(),
            finished: false,
        };
        if let Err(e) = spawned {
            handle.finish_with(JobEvent::Failed(Error::Thread { source: e }));
        }
        handle
    }
}

impl<P, T> JobHandle<P, T> {
    fn finish_with(&mut self, event: JobEvent<P, T>) {
        if let Ok(mut slot) = self.result.lock() {
            *slot = Some(event);
        }
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(Message::End);
        self.rx = rx;
    }

    /// The next event without blocking, or `None` if there is none yet (or the job ended
    /// and its final event was already returned).
    pub fn try_recv(&mut self) -> Option<JobEvent<P, T>> {
        if self.finished {
            return None;
        }
        match self.rx.try_recv() {
            Ok(Message::Partial(p)) => Some(JobEvent::Partial(p)),
            Ok(Message::End) | Err(TryRecvError::Disconnected) => self.take_final(),
            Err(TryRecvError::Empty) => None,
        }
    }

    /// Blocks until the next event (tests and tools; never call it on the UI thread).
    /// `None` once the final event was returned.
    pub fn wait(&mut self) -> Option<JobEvent<P, T>> {
        if self.finished {
            return None;
        }
        match self.rx.recv() {
            Ok(Message::Partial(p)) => Some(JobEvent::Partial(p)),
            Ok(Message::End) | Err(_) => self.take_final(),
        }
    }

    fn take_final(&mut self) -> Option<JobEvent<P, T>> {
        self.finished = true;
        let event = self.result.lock().ok().and_then(|mut slot| slot.take());
        // A worker that panicked never fills the slot.
        Some(event.unwrap_or(JobEvent::Failed(Error::JobPanicked)))
    }

    /// Asks the job to stop; it ends with [`JobEvent::Cancelled`] soon after.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// The job's cancellation token.
    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// The job's progress.
    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    /// Time since the job was spawned.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// `true` once the final event was returned by [`try_recv`](Self::try_recv) or
    /// [`wait`](Self::wait).
    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

impl<P, T> Drop for JobHandle<P, T> {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl<P, T> std::fmt::Debug for JobHandle<P, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobHandle")
            .field("progress", &self.progress)
            .field("cancelled", &self.cancel.is_cancelled())
            .field("finished", &self.finished)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn drain<P, T>(job: &mut JobHandle<P, T>) -> Vec<JobEvent<P, T>> {
        std::iter::from_fn(|| job.wait()).collect()
    }

    #[test]
    fn done_after_partials_and_repaints() {
        let repaints = Arc::new(AtomicUsize::new(0));
        let r = repaints.clone();
        let repaint: Repaint = Arc::new(move || {
            r.fetch_add(1, Ordering::Relaxed);
        });
        let mut job = JobHandle::spawn("t", 10, repaint, |ctx| {
            ctx.partial(1);
            ctx.partial(2);
            ctx.progress().add(10);
            Ok("done")
        });
        let events = drain(&mut job);
        assert!(matches!(
            events.as_slice(),
            [
                JobEvent::Partial(1),
                JobEvent::Partial(2),
                JobEvent::Done("done")
            ]
        ));
        assert!(job.is_finished());
        assert!(job.try_recv().is_none());
        assert_eq!(repaints.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn failed_and_cancelled() {
        let mut job: JobHandle<(), ()> =
            JobHandle::spawn("t", 0, no_repaint(), |_| Err(Error::EmptyFile));
        assert!(matches!(
            drain(&mut job).as_slice(),
            [JobEvent::Failed(Error::EmptyFile)]
        ));

        let mut job: JobHandle<(), ()> = JobHandle::spawn("t", 0, no_repaint(), |ctx| {
            while !ctx.cancel().is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            ctx.cancel().check()
        });
        std::thread::sleep(Duration::from_millis(5));
        assert!(job.try_recv().is_none());
        job.cancel();
        assert!(matches!(drain(&mut job).as_slice(), [JobEvent::Cancelled]));
    }

    #[test]
    fn result_after_cancel_counts_as_cancelled() {
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let mut job: JobHandle<(), u32> = JobHandle::spawn("t", 0, no_repaint(), move |_| {
            let _ = go_rx.recv();
            Ok(7)
        });
        job.cancel();
        go_tx.send(()).unwrap();
        assert!(matches!(drain(&mut job).as_slice(), [JobEvent::Cancelled]));
    }

    #[test]
    fn panic_is_reported_not_propagated() {
        let mut job: JobHandle<(), ()> =
            JobHandle::spawn("t", 0, no_repaint(), |_| panic!("boom in a test job"));
        assert!(matches!(
            drain(&mut job).as_slice(),
            [JobEvent::Failed(Error::JobPanicked)]
        ));
    }

    #[test]
    fn dropping_the_handle_cancels() {
        let (seen_tx, seen_rx) = mpsc::channel();
        let job: JobHandle<(), ()> = JobHandle::spawn("t", 0, no_repaint(), move |ctx| {
            while !ctx.cancel().is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            let _ = seen_tx.send(());
            Ok(())
        });
        drop(job);
        assert!(seen_rx.recv_timeout(Duration::from_secs(5)).is_ok());
    }
}
