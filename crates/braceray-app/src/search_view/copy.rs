//! "Copy all paths": the JSONPath of every stored match, on a worker with progress. Up
//! to [`COPY_TO_FILE_ABOVE`] rows go to the clipboard; more are written to a temp file
//! the user can open.

use std::io::Write as _;
use std::path::PathBuf;

use braceray_core::Error;
use braceray_core::job::{JobEvent, JobHandle, Repaint};
use braceray_core::path::jsonpath;
use braceray_core::search::SearchNav;

use crate::tree_view::{TreeView, navigator};

/// More rows than this are written to a file instead of the clipboard.
pub const COPY_TO_FILE_ABOVE: usize = 10_000;

/// Rows between cancel checks and progress updates.
const BATCH: usize = 4096;

/// What a finished copy produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyOutcome {
    /// The paths, one per line, for the clipboard.
    Clipboard {
        /// The text.
        text: String,
        /// Number of paths.
        rows: usize,
    },
    /// Written to this file.
    File {
        /// The file.
        path: PathBuf,
        /// Number of paths.
        rows: usize,
    },
    /// Writing failed.
    Failed(String),
}

/// State of "Copy all paths".
#[derive(Default)]
pub struct CopyPaths {
    job: Option<JobHandle<(), CopyOutcome>>,
    /// The last finished copy, until dismissed (the app takes clipboard text from it).
    pub outcome: Option<CopyOutcome>,
}

impl std::fmt::Debug for CopyPaths {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CopyPaths")
            .field("busy", &self.job.is_some())
            .field("outcome", &self.outcome)
            .finish()
    }
}

impl CopyPaths {
    /// Starts copying the paths of the `rows` first stored matches of `nav`.
    pub fn start(&mut self, nav: SearchNav, rows: usize, tree: &TreeView, repaint: &Repaint) {
        if self.job.is_some() || tree.is_preview() {
            return;
        }
        self.outcome = None;
        let (doc, source, cache) = tree.shared();
        self.job = Some(JobHandle::spawn(
            "copy-paths",
            rows as u64,
            repaint.clone(),
            move |ctx| {
                let tree_nav = navigator(&doc, &source, &cache);
                let bytes = tree_nav.bytes();
                let to_file = rows > COPY_TO_FILE_ABOVE;
                let mut text = String::new();
                let mut file = if to_file {
                    let path = std::env::temp_dir().join(format!(
                        "braceray-paths-{}-{}.txt",
                        std::process::id(),
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_millis())
                    ));
                    match std::fs::File::create(&path) {
                        Ok(f) => Some((path, std::io::BufWriter::new(f))),
                        Err(e) => {
                            return Ok(CopyOutcome::Failed(format!("{}: {e}", path.display())));
                        }
                    }
                } else {
                    None
                };
                for i in 0..rows {
                    if i % BATCH == 0 {
                        ctx.cancel().check()?;
                        ctx.progress().add(BATCH.min(rows - i) as u64);
                        if nav.is_cancelled() {
                            return Err(Error::Cancelled);
                        }
                    }
                    let Some(m) = nav.get(i) else { break };
                    let path =
                        jsonpath(bytes, &tree_nav.path_of(&tree_nav.node_at_offset(m.offset)));
                    match &mut file {
                        Some((p, w)) => {
                            if let Err(e) = writeln!(w, "{path}") {
                                return Ok(CopyOutcome::Failed(format!("{}: {e}", p.display())));
                            }
                        }
                        None => {
                            text.push_str(&path);
                            text.push('\n');
                        }
                    }
                }
                Ok(match file {
                    Some((path, mut w)) => match w.flush() {
                        Ok(()) => CopyOutcome::File { path, rows },
                        Err(e) => CopyOutcome::Failed(format!("{}: {e}", path.display())),
                    },
                    None => CopyOutcome::Clipboard { text, rows },
                })
            },
        ));
    }

    /// `true` while copying.
    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }

    /// `0.0..=1.0` while copying.
    pub fn progress(&self) -> f32 {
        self.job.as_ref().map_or(1.0, |j| j.progress().fraction())
    }

    /// Cancels a running copy.
    pub fn cancel(&mut self) {
        self.job = None;
    }

    pub(super) fn poll(&mut self) {
        let Some(event) = self.job.as_mut().and_then(JobHandle::try_recv) else {
            return;
        };
        self.job = None;
        self.outcome = match event {
            JobEvent::Done(o) => Some(o),
            JobEvent::Failed(e) => Some(CopyOutcome::Failed(e.to_string())),
            JobEvent::Cancelled | JobEvent::Partial(()) => None,
        };
    }
}
