//! Human-readable sizes: `500`, `10k`, `100m`, `2g` (binary units: 1k = 1024 bytes).

/// Parses a size such as `100m` or `2g` into bytes.
///
/// ```
/// assert_eq!(gen_fixtures::size::parse_size("10k").unwrap(), 10 * 1024);
/// assert_eq!(gen_fixtures::size::parse_size("2g").unwrap(), 2 << 30);
/// ```
pub fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim().to_ascii_lowercase();
    let (digits, shift) = match s.as_bytes().last() {
        Some(b'k') => (&s[..s.len() - 1], 10),
        Some(b'm') => (&s[..s.len() - 1], 20),
        Some(b'g') => (&s[..s.len() - 1], 30),
        Some(b'b') => (&s[..s.len() - 1], 0),
        _ => (&s[..], 0),
    };
    let n: u64 = digits
        .parse()
        .map_err(|_| format!("invalid size '{s}' (examples: 500, 10k, 100m, 2g)"))?;
    n.checked_mul(1 << shift)
        .ok_or_else(|| format!("size '{s}' is too large"))
}

/// Formats bytes as a short name suffix: `10m`, `2g`, `512k`, `100`.
pub fn size_label(bytes: u64) -> String {
    for (shift, unit) in [(30, 'g'), (20, 'm'), (10, 'k')] {
        if bytes >= 1 << shift && bytes.is_multiple_of(1 << shift) {
            return format!("{}{unit}", bytes >> shift);
        }
    }
    bytes.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_units() {
        assert_eq!(parse_size("500").unwrap(), 500);
        assert_eq!(parse_size("100M").unwrap(), 100 << 20);
        assert_eq!(parse_size("3g").unwrap(), 3 << 30);
        assert!(parse_size("abc").is_err());
        assert!(parse_size("").is_err());
        assert!(parse_size("99999999999999g").is_err());
    }

    #[test]
    fn labels() {
        assert_eq!(size_label(10 << 20), "10m");
        assert_eq!(size_label(2 << 30), "2g");
        assert_eq!(size_label(1500), "1500");
    }
}
