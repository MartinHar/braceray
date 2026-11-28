//! Markdown output of `bench-big` (rows in the format of `docs/PERF.md`).

use std::fmt::Write as _;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::Row;
use super::machine::MachineInfo;

/// All rows of one run plus the machine they ran on.
#[derive(Debug, Clone)]
pub struct Report {
    pub machine: MachineInfo,
    pub rows: Vec<Row>,
}

impl Report {
    pub(super) fn new(machine: MachineInfo) -> Self {
        Self {
            machine,
            rows: Vec::new(),
        }
    }

    /// `true` when no row missed its target.
    pub fn all_met(&self) -> bool {
        self.rows.iter().all(|r| r.met != Some(false))
    }

    /// Markdown: a *Machines* row and the *Measurements* rows for `docs/PERF.md`.
    /// `machine_id` is the short name used in the tables (e.g. `M3P`).
    pub fn markdown(&self, machine_id: &str, step: &str) -> String {
        let m = &self.machine;
        let date = today();
        let commit = commit().unwrap_or_else(|| "?".to_owned());
        let mut out = String::new();
        // `write!` to a String can't fail.
        let _ = writeln!(out, "Machine (for *Machines*):\n");
        let _ = writeln!(out, "| ID | CPU | RAM | Disk | OS |");
        let _ = writeln!(out, "|---|---|---|---|---|");
        let _ = writeln!(
            out,
            "| {machine_id} | {}, {} cores | {} | (fill in) | {} |\n",
            m.cpu, m.cores, m.ram, m.os
        );
        let _ = writeln!(out, "Rows (for *Measurements*):\n");
        let _ = writeln!(
            out,
            "| Date | Step | Commit | Benchmark | Fixture | Machine | Result | Notes |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|");
        for r in &self.rows {
            let mark = match r.met {
                Some(true) => " ✅",
                Some(false) => " ❌",
                None => "",
            };
            let _ = writeln!(
                out,
                "| {date} | {step} | {commit} | `bench-big` {} | {} | {machine_id} | **{}**{mark} | {} |",
                r.benchmark, r.fixture, r.result, r.notes
            );
        }
        out
    }
}

fn commit() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Today (UTC) as `YYYY-MM-DD`.
fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 → (year, month, day), proleptic Gregorian (H. Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_785), (2026, 11, 28));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn markdown_marks_targets() {
        let mut r = Report::new(MachineInfo {
            cpu: "CPU".into(),
            cores: 4,
            ram: "8 GB".into(),
            os: "OS".into(),
        });
        r.rows
            .push(Row::new("a", "f.json", "1 ms".into()).target(true, "< 2 ms"));
        r.rows
            .push(Row::new("b", "f.json", "3 ms".into()).target(false, "< 2 ms"));
        r.rows.push(Row::new("c", "f.json", "x".into()));
        let md = r.markdown("M", "12");
        assert!(md.contains("**1 ms** ✅ | target < 2 ms"));
        assert!(md.contains("**3 ms** ❌"));
        assert!(md.contains("| `bench-big` c | f.json | M | **x** |"));
        assert!(!r.all_met());
    }
}
