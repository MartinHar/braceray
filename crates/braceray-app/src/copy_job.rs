//! Copying values (step 11): small ones right away, big ones on a worker with progress,
//! the size guard ("This value is 1.2 GB…"), and "Export…" of a value's raw bytes.

use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use braceray_core::copy::{CopyStyle, render, write_to};
use braceray_core::job::{JobEvent, JobHandle, Repaint};
use braceray_core::nav::Span;
use braceray_core::{CancelToken, Document, Error, Progress};

use crate::format;

/// Values up to this size are copied on the UI thread (≈ 1 ms even pretty).
pub const SYNC_BYTES: u64 = 1 << 20;

/// How long a copy notice ("Copied 12.4 MB") stays.
pub const NOTICE: Duration = Duration::from_millis(2500);

/// Indentation of "Copy formatted".
pub const PRETTY: CopyStyle = CopyStyle::Pretty { indent: 2 };

/// What is copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    /// A value in some style.
    Value,
    /// A key (decoded, without quotes).
    Key,
    /// A path, in the named format.
    Path(&'static str),
}

/// One copy request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    /// The bytes to copy (a value, or a key with its quotes).
    pub span: Span,
    /// How.
    pub style: CopyStyle,
    /// Value or key (for the notice).
    pub what: What,
}

impl Request {
    /// Size of the source bytes.
    pub fn size(&self) -> u64 {
        self.span.end.saturating_sub(self.span.start)
    }
}

/// A copy waiting for the user: the value is over the copy limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guard {
    /// The request.
    pub request: Request,
    /// The tab's document length when asked (the guard is dropped if it changes).
    pub doc_len: u64,
}

enum Running {
    Copy(JobHandle<(), String>, What),
    Export(JobHandle<(), Option<PathBuf>>),
}

/// A notice in the breadcrumb bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// The text.
    pub text: String,
    /// Shown as an error.
    pub error: bool,
    /// Hidden after this.
    pub until: Instant,
}

/// Copy state of one tab.
#[derive(Default)]
pub struct CopyState {
    running: Option<Running>,
    /// The size question, while open.
    pub guard: Option<Guard>,
    /// The last notice.
    pub notice: Option<Notice>,
}

impl std::fmt::Debug for CopyState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CopyState")
            .field("busy", &self.is_busy())
            .field("guard", &self.guard)
            .field("notice", &self.notice)
            .finish()
    }
}

/// Most bytes a copy of `size` source bytes may produce (pretty output is bigger than its
/// input; deeply nested input much bigger, see `braceray_core::copy`).
pub fn max_output(size: u64) -> usize {
    usize::try_from(size.saturating_mul(3).saturating_add(64 << 20)).unwrap_or(usize::MAX)
}

impl CopyState {
    /// Copies `req` from `doc`: returns the text right away for small values, opens the
    /// guard above `limit` bytes, otherwise starts a worker ([`poll`](Self::poll) returns
    /// its text).
    pub fn request(
        &mut self,
        doc: &Arc<Document>,
        req: Request,
        limit: u64,
        repaint: &Repaint,
    ) -> Option<String> {
        if self.is_busy() {
            self.notify("Another copy is still running.".to_owned(), true);
            return None;
        }
        if req.size() > limit {
            self.guard = Some(Guard {
                request: req,
                doc_len: doc.len(),
            });
            return None;
        }
        self.copy_now(doc, req, repaint)
    }

    /// "Copy anyway" in the guard.
    pub fn copy_anyway(&mut self, doc: &Arc<Document>, repaint: &Repaint) -> Option<String> {
        let guard = self.guard.take()?;
        (guard.doc_len == doc.len())
            .then(|| self.copy_now(doc, guard.request, repaint))
            .flatten()
    }

    fn copy_now(&mut self, doc: &Arc<Document>, req: Request, repaint: &Repaint) -> Option<String> {
        if req.size() <= SYNC_BYTES {
            let max = max_output(req.size());
            return match render(
                doc.bytes(),
                req.span,
                req.style,
                max,
                &CancelToken::new(),
                &Progress::default(),
            ) {
                Ok(text) => {
                    self.copied(&text, req.what);
                    Some(text)
                }
                Err(e) => {
                    self.notify(e.to_string(), true);
                    None
                }
            };
        }
        let doc = Arc::clone(doc);
        let job = JobHandle::spawn("copy-value", req.size(), repaint.clone(), move |ctx| {
            render(
                doc.bytes(),
                req.span,
                req.style,
                max_output(req.size()),
                ctx.cancel(),
                ctx.progress(),
            )
        });
        self.running = Some(Running::Copy(job, req.what));
        None
    }

    /// Runs `work` on a worker and copies the text it returns (a deep path, I-016).
    pub fn spawn_text<F>(&mut self, what: What, repaint: &Repaint, work: F)
    where
        F: FnOnce() -> Result<String, braceray_core::Error> + Send + 'static,
    {
        let job = JobHandle::spawn("copy-text", 0, repaint.clone(), move |_| work());
        self.running = Some(Running::Copy(job, what));
    }

    /// "Export…" in the guard: asks for a file (the dialog is created here, on the UI
    /// thread, as macOS needs) and writes the value's raw bytes on a worker.
    pub fn export(&mut self, doc: &Arc<Document>, repaint: &Repaint) {
        let Some(guard) = self.guard.take() else {
            return;
        };
        if guard.doc_len != doc.len() {
            return;
        }
        let dialog = rfd::AsyncFileDialog::new()
            .set_title("Export value")
            .set_file_name("value.json")
            .add_filter("JSON", &["json"])
            .save_file();
        let doc = Arc::clone(doc);
        let span = guard.request.span;
        let job = JobHandle::spawn("copy-export", 0, repaint.clone(), move |ctx| {
            let Some(file) = pollster::block_on(dialog) else {
                return Ok(None);
            };
            let path = file.path().to_path_buf();
            export_to(&doc, span, &path, ctx.cancel(), ctx.progress())?;
            Ok(Some(path))
        });
        self.running = Some(Running::Export(job));
    }

    /// Cancels a running copy or export.
    pub fn cancel(&mut self) {
        self.running = None; // dropping a handle cancels its job
        self.notify("Copy cancelled.".to_owned(), false);
    }

    /// `true` while a copy or export runs.
    pub fn is_busy(&self) -> bool {
        self.running.is_some()
    }

    /// Progress of the running job and what it does.
    pub fn progress(&self) -> Option<(f32, &'static str)> {
        match self.running.as_ref()? {
            Running::Copy(j, _) => Some((j.progress().fraction(), "Copying…")),
            Running::Export(j) => Some((j.progress().fraction(), "Exporting…")),
        }
    }

    /// Every frame: returns the text of a finished copy (for the clipboard).
    pub fn poll(&mut self) -> Option<String> {
        if self
            .notice
            .as_ref()
            .is_some_and(|n| Instant::now() >= n.until)
        {
            self.notice = None;
        }
        let failed = match self.running.as_mut()? {
            Running::Copy(job, what) => {
                let what = *what;
                match job.try_recv()? {
                    JobEvent::Done(text) => {
                        self.running = None;
                        self.copied(&text, what);
                        return Some(text);
                    }
                    JobEvent::Failed(e) => Some(e),
                    JobEvent::Cancelled => None,
                    JobEvent::Partial(()) => return None,
                }
            }
            Running::Export(job) => match job.try_recv()? {
                JobEvent::Done(path) => {
                    self.running = None;
                    if let Some(path) = path {
                        let name = path.file_name().map_or_else(
                            || path.display().to_string(),
                            |n| n.to_string_lossy().into_owned(),
                        );
                        self.notify(format!("Exported to {name}"), false);
                    }
                    return None;
                }
                JobEvent::Failed(e) => Some(e),
                JobEvent::Cancelled => None,
                JobEvent::Partial(()) => return None,
            },
        };
        self.running = None;
        if let Some(e) = failed {
            self.notify(e.to_string(), true);
        }
        None
    }

    fn copied(&mut self, text: &str, what: What) {
        let text = match what {
            What::Key => "Copied key".to_owned(),
            What::Value => format!("Copied {}", format::bytes(text.len() as u64)),
            What::Path(label) => format!("Copied {label}"),
        };
        self.notify(text, false);
    }

    /// Shows `text` in the breadcrumb bar for a moment.
    pub fn notify(&mut self, text: String, error: bool) {
        self.notice = Some(Notice {
            text,
            error,
            until: Instant::now() + NOTICE,
        });
    }
}

/// Writes `doc[span]` to a new file at `path`; removes the partial file on failure or
/// cancel.
pub fn export_to(
    doc: &Document,
    span: Span,
    path: &Path,
    cancel: &CancelToken,
    progress: &Progress,
) -> Result<(), Error> {
    let file = std::fs::File::create(path).map_err(|source| Error::Write {
        path: path.to_path_buf(),
        source,
    })?;
    let mut w = BufWriter::new(file);
    let result = write_to(
        doc.bytes(),
        span,
        CopyStyle::Raw,
        &mut w,
        path,
        cancel,
        progress,
    );
    drop(w);
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(bytes: &[u8]) -> (tempfile::TempDir, Arc<Document>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.json");
        std::fs::write(&path, bytes).unwrap();
        let d = Document::open(&path).unwrap();
        (dir, d)
    }

    fn span(s: u64, e: u64) -> Span {
        Span { start: s, end: e }
    }

    #[test]
    fn small_values_copy_now_big_ones_ask() {
        let (_dir, d) = doc(br#"{"k": [1, 2], "s": "a\tb"}"#);
        let repaint = braceray_core::job::no_repaint();
        let mut c = CopyState::default();
        let req = Request {
            span: span(6, 12),
            style: CopyStyle::Minified,
            what: What::Value,
        };
        assert_eq!(c.request(&d, req, 100, &repaint).as_deref(), Some("[1,2]"));
        assert_eq!(c.notice.as_ref().unwrap().text, "Copied 5 B");
        // Over the limit: the guard opens; "Copy anyway" copies.
        assert_eq!(c.request(&d, req, 5, &repaint), None);
        assert_eq!(c.guard.unwrap().request, req);
        assert_eq!(c.copy_anyway(&d, &repaint).as_deref(), Some("[1,2]"));
        assert!(c.guard.is_none());
        // A key, decoded.
        let key = Request {
            span: span(19, 25),
            style: CopyStyle::StringContents,
            what: What::Key,
        };
        assert_eq!(c.request(&d, key, 100, &repaint).as_deref(), Some("a\tb"));
        assert_eq!(c.notice.as_ref().unwrap().text, "Copied key");
    }

    #[test]
    fn guard_is_dropped_when_the_document_changed() {
        let (_dir, d) = doc(b"[1, 2, 3]");
        let (_dir2, other) = doc(b"[1, 2, 3, 4]");
        let repaint = braceray_core::job::no_repaint();
        let mut c = CopyState::default();
        let req = Request {
            span: span(0, 9),
            style: CopyStyle::Raw,
            what: What::Value,
        };
        assert_eq!(c.request(&d, req, 1, &repaint), None);
        assert_eq!(c.copy_anyway(&other, &repaint), None);
        assert!(c.guard.is_none());
    }

    #[test]
    fn big_values_copy_on_a_worker_and_cancel() {
        let mut bytes = b"[\"".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', 2 * SYNC_BYTES as usize));
        bytes.extend_from_slice(b"\"]");
        let (_dir, d) = doc(&bytes);
        let repaint = braceray_core::job::no_repaint();
        let mut c = CopyState::default();
        let req = Request {
            span: span(1, bytes.len() as u64 - 1),
            style: CopyStyle::StringContents,
            what: What::Value,
        };
        assert_eq!(c.request(&d, req, u64::MAX, &repaint), None);
        assert!(c.is_busy());
        // A second copy waits for the first.
        assert_eq!(c.request(&d, req, u64::MAX, &repaint), None);
        assert!(c.notice.as_ref().unwrap().error);
        let start = Instant::now();
        let text = loop {
            if let Some(t) = c.poll() {
                break t;
            }
            assert!(start.elapsed() < Duration::from_secs(20));
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(text.len(), 2 * SYNC_BYTES as usize);
        assert!(!c.is_busy());
        assert!(c.request(&d, req, u64::MAX, &repaint).is_none());
        c.cancel();
        assert!(!c.is_busy());
        assert_eq!(c.poll(), None);
    }

    #[test]
    fn export_writes_the_raw_bytes_and_cleans_up() {
        let (dir, d) = doc(br#"{"a": [1,  2]}"#);
        let out = dir.path().join("out.json");
        let (cancel, progress) = (CancelToken::new(), Progress::default());
        export_to(&d, span(6, 13), &out, &cancel, &progress).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"[1,  2]");
        // Cancelled: no partial file left.
        let cancelled = CancelToken::new();
        cancelled.cancel();
        let out2 = dir.path().join("out2.json");
        let r = export_to(&d, span(0, 14), &out2, &cancelled, &progress);
        assert!(matches!(r, Err(Error::Cancelled)));
        assert!(!out2.exists());
        // A path that can't be created names the file.
        let bad = dir.path().join("missing/dir/x.json");
        let e = export_to(&d, span(0, 14), &bad, &cancel, &progress).unwrap_err();
        assert!(e.to_string().contains("x.json"), "{e}");
    }
}
