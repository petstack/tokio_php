//! Environment variable parsing utilities.

use std::time::Duration;

/// Get environment variable with default value.
pub fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Get optional environment variable (None if empty or missing).
pub fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// Parse environment variable as boolean.
/// Treats "1", "true" (case-insensitive) as true.
pub fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(default)
}

/// Parse duration string (e.g., "30s", "2m", "1h", "1d", "1w").
/// Returns None for "off" or "0".
pub fn parse_duration(s: &str) -> Result<Option<Duration>, String> {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;
    const YEAR: u64 = 365 * DAY;

    let s = s.trim().to_lowercase();

    if s == "off" || s == "0" || s.is_empty() {
        return Ok(None);
    }

    // Split at first non-digit character
    let split_idx = s.find(|c: char| !c.is_ascii_digit());

    let (num_str, unit) = match split_idx {
        Some(0) => return Err(format!("missing number in duration: {s}")),
        Some(idx) => (&s[..idx], &s[idx..]),
        None => (&s[..], "s"), // Plain number treated as seconds
    };

    let num: u64 = num_str
        .parse()
        .map_err(|_| format!("invalid number in duration: {s}"))?;

    let multiplier = match unit {
        "s" => 1,
        "m" => MINUTE,
        "h" => HOUR,
        "d" => DAY,
        "w" => WEEK,
        "y" => YEAR,
        _ => return Err(format!("unknown unit '{unit}' in duration: {s}")),
    };

    Ok(Some(Duration::from_secs(num * multiplier)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("off").unwrap(), None);
        assert_eq!(parse_duration("0").unwrap(), None);
        assert_eq!(parse_duration("").unwrap(), None);

        assert_eq!(
            parse_duration("30s").unwrap(),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            parse_duration("2m").unwrap(),
            Some(Duration::from_secs(120))
        );
        assert_eq!(
            parse_duration("1h").unwrap(),
            Some(Duration::from_secs(3600))
        );
        assert_eq!(
            parse_duration("1d").unwrap(),
            Some(Duration::from_secs(86400))
        );
        assert_eq!(
            parse_duration("1w").unwrap(),
            Some(Duration::from_secs(604800))
        );

        // Plain seconds
        assert_eq!(
            parse_duration("120").unwrap(),
            Some(Duration::from_secs(120))
        );
    }

    #[test]
    fn test_parse_duration_edge_cases() {
        // Missing number
        assert!(parse_duration("s").is_err());
        assert!(parse_duration("m").is_err());

        // Unknown unit
        assert!(parse_duration("5x").is_err());

        // Whitespace
        assert_eq!(
            parse_duration("  30s  ").unwrap(),
            Some(Duration::from_secs(30))
        );

        // Case insensitive
        assert_eq!(parse_duration("OFF").unwrap(), None);
        assert_eq!(
            parse_duration("30S").unwrap(),
            Some(Duration::from_secs(30))
        );
    }
}
