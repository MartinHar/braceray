//! The "Go to" dialog (Ctrl/Cmd+L): a line, `line:col`, `@offset` or a simple path. The
//! target is resolved on a worker (a key lookup may scan a big object); the raw view
//! scrolls there and the tree selects the value.

use braceray_core::goto::{self, GoTo, GoToError};
use braceray_core::job::{JobEvent, JobHandle, Repaint};
use braceray_core::nav::Span;
use eframe::egui::{self, Key, RichText};

use crate::theme;
use crate::tree_view::{TreeView, navigator};

/// Title of the dialog.
pub const TITLE: &str = "Go to";
/// Label of the confirm button.
pub const GO: &str = "Go";
/// Longest span flashed for a line target.
const LINE_FLASH: usize = 400;

/// Where a go-to request leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoToHit {
    /// Span to show and flash in the raw view.
    pub span: Span,
    /// Tree address of the innermost value there.
    pub address: Vec<u64>,
}

/// State of the open dialog.
#[derive(Default)]
pub struct GoToDialog {
    /// The text field.
    pub text: String,
    /// The last error.
    pub error: Option<String>,
    /// Tab the running request belongs to.
    pub tab_id: Option<u64>,
    job: Option<JobHandle<(), Result<GoToHit, String>>>,
    focused: bool,
}

impl std::fmt::Debug for GoToDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoToDialog")
            .field("text", &self.text)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl GoToDialog {
    /// `true` while a request is being resolved.
    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }

    /// Parses the text and resolves it on a worker for the tab `tab_id`.
    pub fn submit(&mut self, tab_id: u64, tree: &TreeView, repaint: &Repaint) {
        let target = match goto::parse(&self.text) {
            Ok(t) => t,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        self.error = None;
        self.tab_id = Some(tab_id);
        let (doc, source, cache) = tree.shared();
        self.job = Some(JobHandle::spawn("goto", 0, repaint.clone(), move |ctx| {
            let nav = navigator(&doc, &source, &cache);
            let bytes = nav.bytes();
            let at = |off: u64| nav.address_of(&nav.node_at_offset(off));
            let hit = match target {
                GoTo::Line { line, column } => match goto::resolve_line(&nav, line, column) {
                    Ok(off) => {
                        let end = if column.is_some() {
                            (off + 1).min(bytes.len() as u64)
                        } else {
                            let s = off as usize;
                            let window = &bytes[s..(s + LINE_FLASH).min(bytes.len())];
                            let line_len = window.iter().position(|&b| b == b'\n');
                            off + line_len.unwrap_or(window.len()) as u64
                        };
                        // The tree shows the value the line starts with, not the
                        // container its indentation belongs to.
                        let first = bytes[off as usize..end as usize]
                            .iter()
                            .position(|b| !b.is_ascii_whitespace())
                            .map_or(off, |p| off + p as u64);
                        Ok(GoToHit {
                            span: Span { start: off, end },
                            address: at(first),
                        })
                    }
                    Err(e) => Err(e),
                },
                GoTo::Offset(off) if off <= bytes.len() as u64 => Ok(GoToHit {
                    span: Span {
                        start: off,
                        end: (off + 1).min(bytes.len() as u64),
                    },
                    address: at(off),
                }),
                GoTo::Offset(off) => Err(GoToError::OutOfRange(format!(
                    "Offset {} is past the end: the file has {} bytes.",
                    crate::format::group_digits(off),
                    crate::format::group_digits(bytes.len() as u64)
                ))),
                GoTo::Path(steps) => {
                    goto::resolve_path(&nav, &steps, ctx.cancel()).and_then(|(address, child)| {
                        let child = child.or_else(|| nav.root()).ok_or_else(|| {
                            GoToError::NotFound("The document has no value.".to_owned())
                        })?;
                        let start = child.key.map_or(child.value.start, |k| k.start);
                        Ok(GoToHit {
                            span: Span {
                                start,
                                end: child.value.end,
                            },
                            address,
                        })
                    })
                }
            };
            match hit {
                Err(GoToError::Cancelled) => Err(braceray_core::Error::Cancelled),
                other => Ok(other.map_err(|e| e.to_string())),
            }
        }));
    }

    /// The resolved target once the worker is done (errors are kept in `error`).
    pub fn poll(&mut self) -> Option<GoToHit> {
        let event = self.job.as_mut()?.try_recv()?;
        self.job = None;
        match event {
            JobEvent::Done(Ok(hit)) => Some(hit),
            JobEvent::Done(Err(message)) => {
                self.error = Some(message);
                None
            }
            JobEvent::Failed(e) => {
                self.error = Some(e.to_string());
                None
            }
            _ => None,
        }
    }
}

/// What the user did in the dialog.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DialogOutput {
    /// Enter or "Go".
    pub submit: bool,
    /// Escape or the window's ×.
    pub close: bool,
}

/// Draws the dialog.
pub fn show(ctx: &egui::Context, dialog: &mut GoToDialog) -> DialogOutput {
    let mut out = DialogOutput::default();
    let mut open = true;
    egui::Window::new(TITLE)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0))
        .show(ctx, |ui| {
            ui.label("Line, line:column, @offset (@200m, @1.5g) or a path ($.items[3].id)");
            let edit = ui.add(
                egui::TextEdit::singleline(&mut dialog.text)
                    .hint_text("12345 · 120:4 · @1.5g · $.a.b[3]")
                    .desired_width(360.0),
            );
            if !dialog.focused {
                edit.request_focus();
                dialog.focused = true;
            }
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.horizontal(|ui| {
                if ui.button(GO).clicked() || enter {
                    out.submit = true;
                }
                if dialog.is_busy() {
                    ui.spinner();
                }
            });
            if let Some(e) = &dialog.error {
                ui.label(RichText::new(e).color(theme::error_color(ui.visuals().dark_mode)));
            }
        });
    if !open || ctx.input(|i| i.key_pressed(Key::Escape)) {
        out.close = true;
    }
    out
}
