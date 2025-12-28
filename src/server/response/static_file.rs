//! Static file serving with HTTP caching support.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};

use super::compression::{compress_brotli, should_compress_mime, MAX_COMPRESSION_SIZE, MIN_COMPRESSION_SIZE};
use super::EMPTY_BODY;
use crate::server::config::StaticCacheTtl;

/// Format SystemTime as HTTP-date (RFC 7231).
/// Example: "Sun, 06 Nov 1994 08:49:37 GMT"
fn format_http_date(time: SystemTime) -> String {
    let secs = time.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

    // Calculate date/time components
    let days = secs / 86400;
    let day_secs = secs % 86400;

    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    // Calculate year/month/day from days since epoch
    let mut y = 1970i64;
    let mut remaining_days = days as i64;

    loop {
        let year_days = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if remaining_days < year_days {
            break;
        }
        remaining_days -= year_days;
        y += 1;
    }

    let is_leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days: [i64; 12] = if is_leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 1;
    for (i, &days_in_month) in month_days.iter().enumerate() {
        if remaining_days < days_in_month {
            m = i + 1;
            break;
        }
        remaining_days -= days_in_month;
    }
    let d = remaining_days + 1;

    // Calculate day of week (0 = Thursday for Unix epoch)
    // (days + 4) % 7 gives: 0=Sun, 1=Mon, ...
    let dow = ((days + 4) % 7) as usize;
    let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let month_names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        day_names[dow],
        d,
        month_names[m - 1],
        y,
        hours,
        minutes,
        seconds
    )
}

/// Parse HTTP-date (RFC 7231) to SystemTime.
/// Supports format: "Sun, 06 Nov 1994 08:49:37 GMT"
fn parse_http_date(s: &str) -> Option<SystemTime> {
    // Format: "Day, DD Mon YYYY HH:MM:SS GMT"
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 6 || parts[5] != "GMT" {
        return None;
    }

    let day: u64 = parts[1].parse().ok()?;
    let month = match parts[2] {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: i64 = parts[3].parse().ok()?;

    let time_parts: Vec<&str> = parts[4].split(':').collect();
    if time_parts.len() != 3 {
        return None;
    }
    let hours: u64 = time_parts[0].parse().ok()?;
    let minutes: u64 = time_parts[1].parse().ok()?;
    let seconds: u64 = time_parts[2].parse().ok()?;

    // Calculate days since epoch
    let mut total_days: i64 = 0;
    for y in 1970..year {
        total_days += if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
    }

    let is_leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days: [i64; 12] = if is_leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    for m in 0..(month - 1) {
        total_days += month_days[m as usize];
    }
    total_days += day as i64 - 1;

    let total_secs = total_days as u64 * 86400 + hours * 3600 + minutes * 60 + seconds;
    Some(UNIX_EPOCH + Duration::from_secs(total_secs))
}

/// Generate ETag from file size and modification time.
/// Format: "size-mtime_hex"
fn generate_etag(size: u64, mtime: SystemTime) -> String {
    let mtime_secs = mtime.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    format!("\"{:x}-{:x}\"", size, mtime_secs)
}

/// Check if client's cached version is still valid.
/// Returns true if we should return 304 Not Modified.
fn is_cache_valid(
    if_none_match: Option<&str>,
    if_modified_since: Option<&str>,
    etag: &str,
    mtime: SystemTime,
) -> bool {
    // If-None-Match takes precedence (RFC 7232 Section 6)
    if let Some(client_etag) = if_none_match {
        // Handle multiple ETags: "etag1", "etag2" or *
        if client_etag == "*" {
            return true;
        }
        // Check if any of the client's ETags match
        for tag in client_etag.split(',') {
            let tag = tag.trim();
            // Strip W/ prefix for weak comparison
            let tag = tag.strip_prefix("W/").unwrap_or(tag);
            if tag == etag {
                return true;
            }
        }
        return false;
    }

    // Fall back to If-Modified-Since
    if let Some(date_str) = if_modified_since {
        if let Some(client_time) = parse_http_date(date_str) {
            // File not modified if mtime <= client_time
            // Compare at second granularity (HTTP-date has no sub-second precision)
            let mtime_secs = mtime.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
            let client_secs = client_time.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
            return mtime_secs <= client_secs;
        }
    }

    false
}

/// Serve a static file from the filesystem with optional caching headers.
/// Supports conditional requests (If-None-Match, If-Modified-Since).
pub async fn serve_static_file(
    file_path: &Path,
    use_brotli: bool,
    cache_ttl: &StaticCacheTtl,
    if_none_match: Option<&str>,
    if_modified_since: Option<&str>,
) -> Response<Full<Bytes>> {
    // Get file metadata for caching headers
    let metadata = match tokio::fs::metadata(file_path).await {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("Failed to read file metadata {:?}: {}", file_path, e);
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "text/plain")
                .body(Full::new(EMPTY_BODY.clone()))
                .unwrap();
        }
    };

    let size = metadata.len();
    let mtime = metadata.modified().unwrap_or(UNIX_EPOCH);
    let etag = generate_etag(size, mtime);
    let last_modified = format_http_date(mtime);

    // Check conditional request headers
    if cache_ttl.is_enabled() && is_cache_valid(if_none_match, if_modified_since, &etag, mtime) {
        // Return 304 Not Modified
        let ttl_secs = cache_ttl.as_secs();
        let expires_time = SystemTime::now() + std::time::Duration::from_secs(ttl_secs);

        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header("Cache-Control", format!("public, max-age={}", ttl_secs))
            .header("Expires", format_http_date(expires_time))
            .header("ETag", &etag)
            .header("Last-Modified", &last_modified)
            .header("Server", "tokio_php/0.1.0")
            .body(Full::new(EMPTY_BODY.clone()))
            .unwrap();
    }

    // Read and serve the file
    match tokio::fs::read(file_path).await {
        Ok(contents) => {
            let mime = mime_guess::from_path(file_path)
                .first_or_octet_stream()
                .to_string();

            // Check if we should compress this file
            let should_compress = use_brotli
                && contents.len() >= MIN_COMPRESSION_SIZE
                && contents.len() <= MAX_COMPRESSION_SIZE
                && should_compress_mime(&mime);

            let (final_body, is_compressed) = if should_compress {
                if let Some(compressed) = compress_brotli(&contents) {
                    (Bytes::from(compressed), true)
                } else {
                    (Bytes::from(contents), false)
                }
            } else {
                (Bytes::from(contents), false)
            };

            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", &mime)
                .header("Server", "tokio_php/0.1.0");

            if is_compressed {
                builder = builder
                    .header("Content-Encoding", "br")
                    .header("Vary", "Accept-Encoding");
            }

            // Add caching headers if enabled
            if cache_ttl.is_enabled() {
                let ttl_secs = cache_ttl.as_secs();

                builder = builder
                    .header("Cache-Control", format!("public, max-age={}", ttl_secs))
                    .header("Expires", format_http_date(SystemTime::now() + std::time::Duration::from_secs(ttl_secs)))
                    .header("ETag", &etag)
                    .header("Last-Modified", &last_modified);
            }

            builder.body(Full::new(final_body)).unwrap()
        }
        Err(e) => {
            tracing::error!("Failed to read file {:?}: {}", file_path, e);
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "text/plain")
                .body(Full::new(EMPTY_BODY.clone()))
                .unwrap()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_http_date() {
        // Unix epoch
        let epoch = UNIX_EPOCH;
        assert_eq!(format_http_date(epoch), "Thu, 01 Jan 1970 00:00:00 GMT");

        // Known date: 2024-01-15 12:30:45 UTC
        let time = UNIX_EPOCH + std::time::Duration::from_secs(1705322445);
        assert_eq!(format_http_date(time), "Mon, 15 Jan 2024 12:30:45 GMT");
    }

    #[test]
    fn test_parse_http_date() {
        let date = "Mon, 15 Jan 2024 12:30:45 GMT";
        let parsed = parse_http_date(date).unwrap();
        let expected = UNIX_EPOCH + std::time::Duration::from_secs(1705322445);
        assert_eq!(parsed, expected);
    }

    #[test]
    fn test_generate_etag() {
        let mtime = UNIX_EPOCH + std::time::Duration::from_secs(1705322445);
        let etag = generate_etag(1024, mtime);
        assert_eq!(etag, "\"400-65a51a2d\"");
    }

    #[test]
    fn test_is_cache_valid_etag_match() {
        let mtime = UNIX_EPOCH + std::time::Duration::from_secs(1705322445);
        let etag = generate_etag(1024, mtime);

        // Exact match
        assert!(is_cache_valid(Some(&etag), None, &etag, mtime));

        // No match
        assert!(!is_cache_valid(Some("\"other\""), None, &etag, mtime));

        // Wildcard
        assert!(is_cache_valid(Some("*"), None, &etag, mtime));

        // Multiple ETags
        let multi = format!("\"other\", {}", etag);
        assert!(is_cache_valid(Some(&multi), None, &etag, mtime));
    }

    #[test]
    fn test_is_cache_valid_modified_since() {
        let mtime = UNIX_EPOCH + std::time::Duration::from_secs(1705322445);
        let etag = generate_etag(1024, mtime);

        // Same time - not modified
        assert!(is_cache_valid(
            None,
            Some("Mon, 15 Jan 2024 12:30:45 GMT"),
            &etag,
            mtime
        ));

        // Later time - not modified
        assert!(is_cache_valid(
            None,
            Some("Mon, 15 Jan 2024 12:31:00 GMT"),
            &etag,
            mtime
        ));

        // Earlier time - modified
        assert!(!is_cache_valid(
            None,
            Some("Mon, 15 Jan 2024 12:30:00 GMT"),
            &etag,
            mtime
        ));
    }
}
