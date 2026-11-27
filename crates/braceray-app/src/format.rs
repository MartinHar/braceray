//! Human-readable numbers for the UI: sizes, durations, rates, digit groups.

use std::time::Duration;

/// `1234567` → `"1,234,567"`.
pub fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Decimal units, as file managers show them: `"812 B"`, `"1.50 MB"`, `"2.00 GB"`.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if n < 1000 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = "B";
    for u in UNITS {
        if value < 1000.0 {
            break;
        }
        value /= 1000.0;
        unit = u;
    }
    if value >= 100.0 {
        format!("{value:.0} {unit}")
    } else if value >= 10.0 {
        format!("{value:.1} {unit}")
    } else {
        format!("{value:.2} {unit}")
    }
}

/// `"0.21 s"` under a second, `"3.8 s"` under a minute, `"2 min 05 s"` above.
pub fn duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 1.0 {
        format!("{secs:.2} s")
    } else if secs < 60.0 {
        format!("{secs:.1} s")
    } else {
        let s = d.as_secs();
        format!("{} min {:02} s", s / 60, s % 60)
    }
}

/// Bytes per second as `"1.65 GB/s"` (or MB/s below 1 GB/s).
pub fn rate(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1e9 {
        format!("{:.2} GB/s", bytes_per_sec / 1e9)
    } else {
        format!("{:.0} MB/s", bytes_per_sec / 1e6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1000), "1,000");
        assert_eq!(group_digits(12_345_678), "12,345,678");
    }

    #[test]
    fn sizes() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(1500), "1.50 KB");
        assert_eq!(bytes(2_000_000_000), "2.00 GB");
        assert_eq!(bytes(45_600_000), "45.6 MB");
        assert_eq!(bytes(471_000_000), "471 MB");
    }

    #[test]
    fn durations_and_rates() {
        assert_eq!(duration(Duration::from_millis(210)), "0.21 s");
        assert_eq!(duration(Duration::from_millis(3800)), "3.8 s");
        assert_eq!(duration(Duration::from_secs(125)), "2 min 05 s");
        assert_eq!(rate(1.65e9), "1.65 GB/s");
        assert_eq!(rate(250e6), "250 MB/s");
    }
}
