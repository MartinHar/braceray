//! Machine description, page-cache eviction and peak-RSS sampling for `bench-big`.

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

/// What the report says about the machine.
#[derive(Debug, Clone)]
pub struct MachineInfo {
    pub cpu: String,
    pub cores: usize,
    pub ram: String,
    pub os: String,
}

impl MachineInfo {
    /// Best-effort description from the OS tools (`sysctl`, `/proc`, PowerShell).
    pub fn detect() -> Self {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        Self {
            cpu: cpu().unwrap_or_else(|| "unknown CPU".to_owned()),
            cores,
            ram: ram_bytes().map_or_else(
                || "unknown".to_owned(),
                |b| format!("{:.0} GB", b as f64 / 1e9),
            ),
            os: os().unwrap_or_else(|| std::env::consts::OS.to_owned()),
        }
    }
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

fn powershell(expr: &str) -> Option<String> {
    run("powershell", &["-NoProfile", "-Command", expr])
}

fn cpu() -> Option<String> {
    match std::env::consts::OS {
        "macos" => run("sysctl", &["-n", "machdep.cpu.brand_string"]),
        "linux" => std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|v| v.trim().to_owned())
        }),
        "windows" => powershell("(Get-CimInstance Win32_Processor).Name"),
        _ => None,
    }
}

fn ram_bytes() -> Option<u64> {
    match std::env::consts::OS {
        "macos" => run("sysctl", &["-n", "hw.memsize"])?.parse().ok(),
        "linux" => std::fs::read_to_string("/proc/meminfo").ok().and_then(|s| {
            let kb: u64 = s
                .lines()
                .find(|l| l.starts_with("MemTotal:"))?
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()?;
            Some(kb * 1024)
        }),
        "windows" => powershell("(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory")?
            .parse()
            .ok(),
        _ => None,
    }
}

fn os() -> Option<String> {
    match std::env::consts::OS {
        "macos" => Some(format!("macOS {}", run("sw_vers", &["-productVersion"])?)),
        "linux" => std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                    .map(|v| v.trim_matches('"').to_owned())
            }),
        "windows" => powershell("(Get-CimInstance Win32_OperatingSystem).Caption"),
        _ => None,
    }
}

/// Tries to drop the OS page cache so the next read comes from disk. Needs root on
/// macOS (`purge`) and Linux (`/proc/sys/vm/drop_caches`); `sudo -n` never prompts.
/// Returns `false` when it isn't possible here (the cold numbers are then skipped).
pub fn drop_page_cache() -> bool {
    let ok = |cmd: &str, args: &[&str]| {
        Command::new(cmd)
            .args(args)
            .status()
            .is_ok_and(|s| s.success())
    };
    match std::env::consts::OS {
        "macos" => ok("purge", &[]) || ok("sudo", &["-n", "purge"]),
        "linux" => {
            ok("sync", &[])
                && (std::fs::write("/proc/sys/vm/drop_caches", "3").is_ok()
                    || ok(
                        "sudo",
                        &["-n", "sh", "-c", "echo 3 > /proc/sys/vm/drop_caches"],
                    ))
        }
        _ => false,
    }
}

/// Resident set size now (bytes), if the platform reports it.
pub fn rss() -> Option<u64> {
    memory_stats::memory_stats().map(|m| m.physical_mem as u64)
}

/// Samples the resident set every 5 ms on a thread and keeps the maximum.
pub struct PeakRss {
    stop: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl PeakRss {
    /// Starts sampling.
    pub fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(AtomicU64::new(rss().unwrap_or(0)));
        let thread = {
            let (stop, peak) = (Arc::clone(&stop), Arc::clone(&peak));
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if let Some(now) = rss() {
                        peak.fetch_max(now, Ordering::Relaxed);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
        };
        Self {
            stop,
            peak,
            thread: Some(thread),
        }
    }

    /// Stops sampling and returns the peak (bytes; 0 if the platform can't tell).
    pub fn finish(mut self) -> u64 {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            // The sampler only reads counters; a panic there is not ours to handle.
            let _ = t.join();
        }
        self.peak.load(Ordering::Relaxed)
    }
}
