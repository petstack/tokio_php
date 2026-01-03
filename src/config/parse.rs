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
    let s = s.trim().to_lowercase();

    if s == "off" || s == "0" || s.is_empty() {
        return Ok(None);
    }

    // Try to split into number and unit
    let (num_str, unit) = if s.ends_with('s') {
        (&s[..s.len() - 1], "s")
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], "m")
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], "h")
    } else if s.ends_with('d') {
        (&s[..s.len() - 1], "d")
    } else if s.ends_with('w') {
        (&s[..s.len() - 1], "w")
    } else if s.ends_with('y') {
        (&s[..s.len() - 1], "y")
    } else {
        // Try parsing as seconds
        return s
            .parse::<u64>()
            .map(|secs| Some(Duration::from_secs(secs)))
            .map_err(|_| format!("invalid duration: {}", s));
    };

    let num: u64 = num_str
        .parse()
        .map_err(|_| format!("invalid number: {}", num_str))?;

    let secs = match unit {
        "s" => num,
        "m" => num * 60,
        "h" => num * 3600,
        "d" => num * 86400,
        "w" => num * 86400 * 7,
        "y" => num * 86400 * 365,
        _ => return Err(format!("invalid unit: {}", unit)),
    };

    Ok(Some(Duration::from_secs(secs)))
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
}
