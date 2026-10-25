//! Human-readable size formatting and parsing.

/// Format a byte count using binary units (KiB, MiB, ...).
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    format!("{value:.1} {}", UNITS[unit])
}

/// Parse a size such as `10`, `10K`, `10KB`, `1.5MiB`, or `2G`.
pub fn parse_size(input: &str) -> Result<u64, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("size is empty".to_owned());
    }

    let number_end = input
        .find(|character: char| !(character.is_ascii_digit() || character == '.'))
        .unwrap_or(input.len());
    let (number, suffix) = input.split_at(number_end);
    let value: f64 = number
        .parse()
        .map_err(|_| format!("invalid number in size '{input}'"))?;

    let multiplier = match suffix.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "k" | "kb" | "kib" => 1024.0,
        "m" | "mb" | "mib" => 1024.0 * 1024.0,
        "g" | "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tb" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        other => return Err(format!("unknown size suffix '{other}'")),
    };

    let bytes = value * multiplier;
    if !bytes.is_finite() || bytes < 0.0 || bytes > u64::MAX as f64 {
        return Err(format!("size '{input}' is out of range"));
    }

    Ok(bytes.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::{format_bytes, parse_size};

    #[test]
    fn formats_binary_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MiB");
    }

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("10"), Ok(10));
        assert_eq!(parse_size("10K"), Ok(10 * 1024));
        assert_eq!(parse_size("1.5MiB"), Ok(1536 * 1024));
        assert_eq!(parse_size("2G"), Ok(2 * 1024 * 1024 * 1024));
        assert!(parse_size("").is_err());
        assert!(parse_size("12XB").is_err());
        assert!(parse_size("nope").is_err());
    }
}
