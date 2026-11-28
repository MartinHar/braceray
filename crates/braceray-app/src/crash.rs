//! Crash handling: a panic hook that saves a crash log and shows "Braceray hit a bug".
//!
//! Release builds abort on panic (`panic = "abort"`), so the crashed process can't keep
//! running a UI. The hook writes the log to the config dir and starts a second copy of
//! the binary with `--crash-report <log>`, which shows the dialog: "Open issue on GitHub"
//! opens a prefilled issue page in the browser. Nothing is sent automatically.

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Command-line flag of the crash dialog process.
pub const CRASH_REPORT_FLAG: &str = "--crash-report";
/// Hidden flag that panics at startup (to check the crash handler by hand and in tests).
pub const CRASH_TEST_FLAG: &str = "--crash-test";
/// Set to `1` to skip the dialog process (tests, scripted runs).
pub const NO_DIALOG_ENV: &str = "BRACERAY_NO_CRASH_DIALOG";
/// New-issue page of the project.
pub const ISSUES_URL: &str = "https://github.com/MartinHar/braceray/issues/new";
/// Crash logs kept in the directory (older ones are deleted).
const KEEP_LOGS: usize = 10;

static REPORTED: AtomicBool = AtomicBool::new(false);

/// Installs the panic hook. `dir` is where crash logs go (the settings folder); without
/// one the log goes to the temp directory.
pub fn install(dir: Option<PathBuf>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // The default hook prints the message to stderr (useful when run from a terminal).
        previous(info);
        // Only the first panic is reported; a panic while reporting must not loop.
        if REPORTED.swap(true, Ordering::SeqCst) {
            return;
        }
        let message = panic_message(info.payload());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_owned());
        let thread = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_owned();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let report = CrashReport {
            message,
            location,
            thread,
            backtrace,
        };
        let dir = dir.clone().unwrap_or_else(std::env::temp_dir);
        match write_log(&dir, &report.log_text(unix_secs())) {
            Ok(path) => {
                eprintln!("Braceray hit a bug. Log saved to {}", path.display());
                if std::env::var_os(NO_DIALOG_ENV).is_none_or(|v| v != "1") {
                    spawn_dialog(&path);
                }
            }
            Err(e) => eprintln!("Braceray hit a bug, and the crash log could not be saved: {e}"),
        }
    }));
}

/// What a crash log and the issue link contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashReport {
    /// The panic message.
    pub message: String,
    /// `file:line:column` of the panic.
    pub location: String,
    /// Name of the panicking thread.
    pub thread: String,
    /// Captured backtrace (addresses only in stripped release builds).
    pub backtrace: String,
}

impl CrashReport {
    /// Text of the crash log file.
    pub fn log_text(&self, unix_secs: u64) -> String {
        let mut s = String::new();
        // `write!` to a String can't fail.
        let _ = writeln!(s, "Braceray crash report");
        let _ = writeln!(s, "version: {}", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(s, "os: {} {}", std::env::consts::OS, std::env::consts::ARCH);
        let _ = writeln!(s, "time (unix): {unix_secs}");
        let _ = writeln!(s, "thread: {}", self.thread);
        let _ = writeln!(s, "location: {}", self.location);
        let _ = writeln!(s, "message: {}", self.message);
        let _ = write!(s, "\nbacktrace:\n{}", self.backtrace);
        s
    }

    /// Reads the fields back from a crash log (for the dialog process).
    pub fn parse_log(text: &str) -> CrashReport {
        let field = |name: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(name))
                .unwrap_or("")
                .trim()
                .to_owned()
        };
        CrashReport {
            message: field("message:"),
            location: field("location:"),
            thread: field("thread:"),
            backtrace: text
                .split_once("\nbacktrace:\n")
                .map(|(_, b)| b.to_owned())
                .unwrap_or_default(),
        }
    }

    /// A new-issue URL with title and body filled in (message and location, no file
    /// contents; the user attaches the log if they want).
    pub fn issue_url(&self) -> String {
        let first_line: String = self
            .message
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(80)
            .collect();
        let title = format!("Crash: {first_line}");
        let message: String = self.message.chars().take(1000).collect();
        let body = format!(
            "**What I was doing:**\n\n(please describe)\n\n**Crash details**\n\
             - Version: {}\n- OS: {} {}\n- Thread: {}\n- Location: `{}`\n\n```\n{}\n```\n\n\
             The full log (with backtrace) is in the crash log file; attach it if you like.\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            self.thread,
            self.location,
            message
        );
        format!(
            "{ISSUES_URL}?title={}&body={}",
            percent_encode(&title),
            percent_encode(&body)
        )
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "(no message)".to_owned()
    }
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Writes `crash-<unix nanoseconds>.log` into `dir` (created if needed) and deletes all
/// but the newest [`KEEP_LOGS`] crash logs there. The fixed-width name sorts in time order.
pub fn write_log(dir: &Path, text: &str) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut tries = 0;
    let path = loop {
        let path = dir.join(format!("crash-{nanos:020}.log"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                io::Write::write_all(&mut f, text.as_bytes())?;
                break path;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && tries < 1000 => {
                nanos += 1;
                tries += 1;
            }
            Err(e) => return Err(e),
        }
    };
    prune_logs(dir);
    Ok(path)
}

/// Best effort: old logs that can't be listed or removed just stay.
fn prune_logs(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("crash-") && name.ends_with(".log")
        })
        .map(|e| e.path())
        .collect();
    if logs.len() <= KEEP_LOGS {
        return;
    }
    logs.sort();
    for path in &logs[..logs.len() - KEEP_LOGS] {
        let _ = std::fs::remove_file(path);
    }
}

fn spawn_dialog(log: &Path) {
    if let Ok(exe) = std::env::current_exe() {
        // Detached: this process aborts right after the hook returns.
        let _ = Command::new(exe).arg(CRASH_REPORT_FLAG).arg(log).spawn();
    }
}

/// Body of `braceray --crash-report <log>`: shows the dialog, opens the issue page on
/// request.
pub fn run_dialog(log: &Path) {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let report = CrashReport::parse_log(&text);
    let description = format!(
        "Braceray hit a bug and had to close.\n\n{}\n\nLog saved to:\n{}\n\n\
         \"Open issue on GitHub\" opens a prefilled issue in your browser. Nothing is sent \
         automatically.",
        report.message,
        log.display()
    );
    let open = "Open issue on GitHub".to_owned();
    let choice = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Braceray hit a bug")
        .set_description(description)
        .set_buttons(rfd::MessageButtons::OkCancelCustom(
            open.clone(),
            "Close".to_owned(),
        ))
        .show();
    if choice == rfd::MessageDialogResult::Custom(open) {
        open_url(&report.issue_url());
    }
}

/// Opens `url` in the default browser with the OS tool.
pub fn open_url(url: &str) {
    let result = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn()
    } else if cfg!(windows) {
        // `start` would treat `&` in the URL as a command separator; this doesn't.
        Command::new("rundll32")
            .arg("url.dll,FileProtocolHandler")
            .arg(url)
            .spawn()
    } else {
        Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(e) = result {
        eprintln!("can't open {url}: {e}");
    }
}

/// Percent-encodes everything except RFC 3986 unreserved characters.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> CrashReport {
        CrashReport {
            message: "index out of bounds: the len is 3 but the index is 7".into(),
            location: "crates/x/src/a.rs:10:5".into(),
            thread: "main".into(),
            backtrace: "   0: foo\n   1: bar\n".into(),
        }
    }

    #[test]
    fn log_round_trips() {
        let r = report();
        let text = r.log_text(1_700_000_000);
        assert!(text.contains("version: "));
        assert!(text.contains("time (unix): 1700000000"));
        assert_eq!(CrashReport::parse_log(&text), r);
    }

    #[test]
    fn percent_encoding() {
        assert_eq!(percent_encode("a b&c=d/é~"), "a%20b%26c%3Dd%2F%C3%A9~");
    }

    #[test]
    fn issue_url_is_prefilled_and_encoded() {
        let url = report().issue_url();
        assert!(url.starts_with(
            "https://github.com/MartinHar/braceray/issues/new?title=Crash%3A%20index"
        ));
        assert!(url.contains("&body="));
        assert!(url.contains("crates%2Fx%2Fsrc%2Fa.rs%3A10%3A5"));
        // Only one `?` and the separating `&`; everything else is encoded.
        assert_eq!(url.matches('?').count(), 1);
        assert_eq!(url.matches('&').count(), 1);
        assert!(!url.contains(' ') && !url.contains('\n'));
        // The backtrace stays in the log only.
        assert!(!url.contains("bar"));
    }

    #[test]
    fn long_messages_are_cut_in_the_url() {
        let r = CrashReport {
            message: "x".repeat(10_000),
            ..report()
        };
        assert!(r.issue_url().len() < 2500);
    }

    #[test]
    fn writes_logs_and_keeps_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for i in 0..(KEEP_LOGS + 3) {
            paths.push(write_log(dir.path(), &format!("log {i}")).unwrap());
        }
        assert_eq!(std::fs::read_to_string(&paths[0]).ok(), None);
        let last = paths.last().unwrap();
        assert_eq!(
            std::fs::read_to_string(last).unwrap(),
            format!("log {}", KEEP_LOGS + 2)
        );
        let count = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(count, KEEP_LOGS);
    }
}
