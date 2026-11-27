//! One open file (a tab): its document, the job that maps and indexes it, and its banners.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use braceray_core::cache::{IndexCache, IndexSource, LoadedIndex, load_or_build};
use braceray_core::index::IndexOptions;
use braceray_core::job::{JobEvent, JobHandle, Repaint};
use braceray_core::tree::preview::{PREVIEW_BYTES, Preview};
use braceray_core::{Document, Error, Validation};

use crate::format;
use crate::tree_view::{Source, TreeView};

/// How often the file is checked for changes on disk.
pub const CHANGE_CHECK_EVERY: Duration = Duration::from_secs(5);

/// The open + index job: `Partial` = the file is mapped, `Done` = the index is ready.
type OpenJob = JobHandle<Arc<Document>, LoadedIndex>;

/// Where a tab is in its life.
#[derive(Debug)]
pub enum TabState {
    /// Mapping the file and checking its encoding.
    Opening,
    /// Mapped; the index is being built (or loaded from the cache).
    Indexing {
        /// The mapped file.
        doc: Arc<Document>,
        /// When the file was mapped (for speed and ETA).
        since: Instant,
    },
    /// Indexed. The document may still be invalid JSON (see [`OpenDocument::banner`]).
    Ready {
        /// The mapped file.
        doc: Arc<Document>,
        /// The index and where it came from.
        loaded: LoadedIndex,
    },
    /// The file couldn't be opened (missing, empty, UTF-16, not JSON, …).
    Failed {
        /// User-facing message.
        message: String,
        /// Copyable technical details.
        details: String,
    },
    /// The user cancelled indexing.
    Cancelled,
}

/// What a banner is about (decides its colour and actions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerKind {
    /// The JSON is invalid; "Open anyway" browses the valid prefix.
    InvalidJson,
    /// The file changed on disk; "Reload" re-opens it.
    FileChanged,
}

/// A message bar above the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    /// Kind (colour, actions).
    pub kind: BannerKind,
    /// One-line message.
    pub message: String,
    /// Copyable details.
    pub details: String,
}

/// Progress numbers of a running index job, for the indexing panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndexProgress {
    /// `0.0..=1.0`.
    pub fraction: f32,
    /// Bytes of the file per second.
    pub bytes_per_sec: f64,
    /// Estimated time left, once there is enough data.
    pub eta: Option<Duration>,
}

/// What a tab needs to start its open + index job.
#[derive(Clone)]
pub struct OpenEnv {
    /// The index cache, if any (with the current settings).
    pub cache: Option<IndexCache>,
    /// Wakes the UI when the job posts an event.
    pub repaint: Repaint,
    /// Test hook: while set, the job waits after mapping the file (so tests can see the
    /// indexing state and cancel it).
    pub hold: Option<Arc<AtomicBool>>,
    /// Children per group row in the tree.
    pub group_size: u64,
}

impl std::fmt::Debug for OpenEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenEnv")
            .field("cache", &self.cache)
            .finish_non_exhaustive()
    }
}

/// One open file.
#[derive(Debug)]
pub struct OpenDocument {
    /// Stable id for egui widget ids.
    pub id: u64,
    /// The path as given by the user.
    pub path: PathBuf,
    /// Canonical path, to find a tab that already shows the same file.
    pub key: PathBuf,
    /// File name shown on the tab.
    pub name: String,
    /// Current state.
    pub state: TabState,
    /// Invalid-JSON or file-changed banner.
    pub banner: Option<Banner>,
    /// The user chose "Open anyway" on an invalid file.
    pub browse_invalid: bool,
    /// Start of the selected value, for line:col in the status bar.
    pub selection: Option<u64>,
    /// The tree (over the preview while indexing, then over the full index).
    pub tree: Option<TreeView>,
    /// When the file was opened (or reloaded).
    pub opened: Instant,
    /// Time from opening to the first frame with tree rows.
    pub first_rows: Option<Duration>,
    group_size: u64,
    preview_job: Option<JobHandle<(), Arc<Preview>>>,
    preview_started: bool,
    job: Option<OpenJob>,
    change_check: Option<JobHandle<(), bool>>,
    last_change_check: Instant,
}

impl OpenDocument {
    /// Starts mapping and indexing `path` on a worker thread.
    pub fn open(id: u64, path: &Path, env: OpenEnv) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let mut tab = OpenDocument {
            id,
            path: path.to_path_buf(),
            key: canonical_key(path),
            name,
            state: TabState::Opening,
            banner: None,
            browse_invalid: false,
            selection: None,
            tree: None,
            opened: Instant::now(),
            first_rows: None,
            group_size: env.group_size,
            preview_job: None,
            preview_started: false,
            job: None,
            change_check: None,
            last_change_check: Instant::now(),
        };
        tab.start(env);
        tab
    }

    /// Re-opens the file from scratch (after "Reload" or "Retry").
    pub fn reload(&mut self, env: OpenEnv) {
        self.state = TabState::Opening;
        self.banner = None;
        self.browse_invalid = false;
        self.selection = None;
        self.tree = None;
        self.opened = Instant::now();
        self.first_rows = None;
        self.preview_job = None;
        self.preview_started = false;
        self.change_check = None;
        self.group_size = env.group_size;
        self.start(env);
    }

    fn start(&mut self, env: OpenEnv) {
        let OpenEnv {
            cache,
            repaint,
            hold,
            group_size: _,
        } = env;
        let path = self.path.clone();
        self.job = Some(JobHandle::spawn("open-index", 0, repaint, move |ctx| {
            let doc = Document::open(&path)?;
            ctx.partial(doc.clone());
            while hold.as_ref().is_some_and(|h| h.load(Ordering::Relaxed)) {
                ctx.cancel().check()?;
                std::thread::sleep(Duration::from_millis(2));
            }
            load_or_build(
                &doc,
                &IndexOptions::default(),
                cache.as_ref(),
                ctx.cancel(),
                ctx.progress(),
            )
        }));
        self.last_change_check = Instant::now();
    }

    /// Polls the jobs (call every frame; never blocks).
    pub fn poll(&mut self, repaint: &Repaint) {
        while let Some(event) = self.job.as_mut().and_then(|j| j.try_recv()) {
            self.apply(event, repaint);
        }
        if self.job.as_ref().is_some_and(|j| j.is_finished()) {
            self.job = None;
        }
        self.poll_preview(repaint);
        if let Some(tree) = &mut self.tree {
            tree.poll(repaint);
            self.selection = tree.selected_spans().map(|(_, value)| value.start);
        }
        if let Some(changed) = self.change_check.as_mut().and_then(|j| j.try_recv()) {
            self.change_check = None;
            if let JobEvent::Done(true) = changed {
                self.banner = Some(Banner {
                    kind: BannerKind::FileChanged,
                    message: "File changed on disk. Reload?".to_owned(),
                    details: format!("{} changed after it was opened.", self.path.display()),
                });
            }
        }
    }

    /// While indexing, builds the preview on a worker and shows it once ready.
    fn poll_preview(&mut self, repaint: &Repaint) {
        let TabState::Indexing { doc, .. } = &self.state else {
            self.preview_job = None;
            return;
        };
        if !self.preview_started {
            self.preview_started = true;
            let doc = Arc::clone(doc);
            self.preview_job = Some(JobHandle::spawn(
                "preview",
                0,
                repaint.clone(),
                move |ctx| {
                    Ok(Arc::new(Preview::build(
                        doc.bytes(),
                        PREVIEW_BYTES,
                        ctx.cancel(),
                    )?))
                },
            ));
        }
        if let Some(JobEvent::Done(preview)) = self.preview_job.as_mut().and_then(|j| j.try_recv())
        {
            self.preview_job = None;
            if self.tree.is_none() {
                let doc = Arc::clone(doc);
                self.tree = Some(TreeView::new(
                    doc,
                    Source::Preview(preview),
                    self.group_size,
                    repaint,
                ));
            }
        }
    }

    /// Re-groups the tree rows (the group-size setting changed).
    pub fn set_group_size(&mut self, group_size: u64, repaint: &Repaint) {
        self.group_size = group_size;
        if let Some(tree) = &mut self.tree {
            tree.set_group_size(group_size, repaint);
        }
    }

    fn apply(&mut self, event: JobEvent<Arc<Document>, LoadedIndex>, repaint: &Repaint) {
        match event {
            JobEvent::Partial(doc) => {
                self.state = TabState::Indexing {
                    doc,
                    since: Instant::now(),
                };
            }
            JobEvent::Done(loaded) => {
                let doc = match std::mem::replace(&mut self.state, TabState::Opening) {
                    TabState::Indexing { doc, .. } => doc,
                    other => {
                        // `Done` always follows the `Partial` with the document.
                        self.state = other;
                        return;
                    }
                };
                if let Validation::Invalid(e) = loaded.index.validation() {
                    self.banner = Some(Banner {
                        kind: BannerKind::InvalidJson,
                        message: e.to_string(),
                        details: format!(
                            "{e}\n\nFile: {}\nThe {} containers before the error can still be browsed.",
                            self.path.display(),
                            format::group_digits(loaded.index.nodes().len() as u64),
                        ),
                    });
                }
                let index = Arc::clone(&loaded.index);
                match &mut self.tree {
                    Some(tree) => tree.switch_to_full(index, repaint),
                    None => {
                        let source = Source::Full(index);
                        self.tree = Some(TreeView::new(
                            Arc::clone(&doc),
                            source,
                            self.group_size,
                            repaint,
                        ));
                    }
                }
                self.preview_job = None;
                self.state = TabState::Ready { doc, loaded };
            }
            JobEvent::Failed(e) => {
                self.tree = None;
                self.state = TabState::Failed {
                    message: e.to_string(),
                    details: error_details(&self.path, &e),
                };
            }
            JobEvent::Cancelled => {
                self.tree = None;
                self.state = TabState::Cancelled;
            }
        }
    }

    /// Cancels indexing (no-op when not running).
    pub fn cancel(&mut self) {
        if let Some(job) = &self.job {
            job.cancel();
        }
    }

    /// `true` while the open/index job runs.
    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }

    /// The mapped document, once known.
    pub fn document(&self) -> Option<&Arc<Document>> {
        match &self.state {
            TabState::Indexing { doc, .. } | TabState::Ready { doc, .. } => Some(doc),
            _ => None,
        }
    }

    /// Speed and ETA while indexing.
    pub fn index_progress(&self) -> Option<IndexProgress> {
        let (TabState::Indexing { doc, since }, Some(job)) = (&self.state, &self.job) else {
            return None;
        };
        let fraction = job.progress().fraction();
        let secs = since.elapsed().as_secs_f64();
        let done_bytes = f64::from(fraction) * doc.len() as f64;
        let bytes_per_sec = if secs > 0.0 { done_bytes / secs } else { 0.0 };
        let eta = (fraction > 0.02 && secs > 0.2).then(|| {
            Duration::from_secs_f64(secs * f64::from(1.0 - fraction) / f64::from(fraction))
        });
        Some(IndexProgress {
            fraction,
            bytes_per_sec,
            eta,
        })
    }

    /// "Indexed 2.00 GB in 3.8 s · 12,345,678 nodes" once ready.
    pub fn summary(&self) -> Option<String> {
        let TabState::Ready { doc, loaded } = &self.state else {
            return None;
        };
        let verb = match loaded.source {
            IndexSource::Built => "Indexed",
            IndexSource::Cache => "Loaded the index of",
        };
        Some(format!(
            "{verb} {} in {} · {} nodes",
            format::bytes(doc.len()),
            format::duration(loaded.elapsed),
            format::group_digits(loaded.index.nodes().len() as u64),
        ))
    }

    /// Starts a change check if the last one is older than [`CHANGE_CHECK_EVERY`], or
    /// right away when `now` is set (window focus).
    pub fn maybe_check_changed(&mut self, now: bool, repaint: Repaint) {
        let due = now || self.last_change_check.elapsed() >= CHANGE_CHECK_EVERY;
        let already = self
            .banner
            .as_ref()
            .is_some_and(|b| b.kind == BannerKind::FileChanged);
        if !due || already || self.change_check.is_some() {
            return;
        }
        let Some(doc) = self.document().cloned() else {
            return;
        };
        self.last_change_check = Instant::now();
        self.change_check = Some(JobHandle::spawn("change-check", 0, repaint, move |_| {
            Ok(doc.has_changed_on_disk())
        }));
    }

    /// Waits for the background cache write of a freshly built index (tests).
    pub fn wait_for_cache_write(&mut self) {
        if let TabState::Ready { loaded, .. } = &mut self.state
            && let Some(w) = loaded.pending_write.take()
        {
            let _ = w.join();
        }
    }
}

/// Canonical form of `path` for de-duplicating tabs (the path itself if that fails).
pub fn canonical_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn error_details(path: &Path, e: &Error) -> String {
    let mut text = format!("{e}\n\nFile: {}", path.display());
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        text.push_str(&format!("\nCaused by: {s}"));
        source = s.source();
    }
    text.push_str(&format!("\nDebug: {e:?}"));
    text
}
