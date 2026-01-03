//! Custom error pages cache.
//!
//! Loads HTML error pages from a directory at startup and serves them
//! for 4xx/5xx responses when the client accepts text/html.

use bytes::Bytes;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Cache of custom error pages loaded at startup.
#[derive(Clone, Default)]
pub struct ErrorPages {
    /// Map of status code -> HTML content
    pages: Arc<HashMap<u16, Bytes>>,
}

impl ErrorPages {
    /// Create an empty error pages cache.
    pub fn new() -> Self {
        Self {
            pages: Arc::new(HashMap::new()),
        }
    }

    /// Load error pages from a directory.
    ///
    /// Scans the directory for files matching `{status_code}.html` pattern
    /// (e.g., `404.html`, `500.html`) and caches them in memory.
    pub fn from_directory(dir: &str) -> Self {
        let path = Path::new(dir);

        if !path.exists() {
            warn!("Error pages directory not found: {}", dir);
            return Self::new();
        }

        if !path.is_dir() {
            warn!("Error pages path is not a directory: {}", dir);
            return Self::new();
        }

        let mut pages = HashMap::new();

        // Scan for {status_code}.html files
        let entries = match std::fs::read_dir(path) {
            Ok(entries) => entries,
            Err(e) => {
                warn!("Failed to read error pages directory: {}", e);
                return Self::new();
            }
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let file_path = entry.path();

            // Check if it's an HTML file
            if file_path.extension().and_then(|e| e.to_str()) != Some("html") {
                continue;
            }

            // Extract status code from filename
            let file_stem = match file_path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };

            let status_code: u16 = match file_stem.parse() {
                Ok(code) if (400..600).contains(&code) => code,
                _ => continue,
            };

            // Read file content
            match std::fs::read(&file_path) {
                Ok(content) => {
                    debug!(
                        "Loaded error page: {} ({} bytes)",
                        file_path.display(),
                        content.len()
                    );
                    pages.insert(status_code, Bytes::from(content));
                }
                Err(e) => {
                    warn!("Failed to read error page {}: {}", file_path.display(), e);
                }
            }
        }

        if !pages.is_empty() {
            let codes: Vec<_> = pages.keys().collect();
            info!("Loaded {} error pages: {:?}", pages.len(), codes);
        }

        Self {
            pages: Arc::new(pages),
        }
    }

    /// Get the HTML content for a status code, if available.
    #[inline]
    pub fn get(&self, status_code: u16) -> Option<&Bytes> {
        self.pages.get(&status_code)
    }

}

/// Get the default reason phrase for an HTTP status code.
/// Returns human-readable text like "Not Found" for 404, "Bad Gateway" for 502.
#[inline]
pub fn status_reason_phrase(status: u16) -> &'static str {
    match status {
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Entity",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        510 => "Not Extended",
        511 => "Network Authentication Required",
        _ => "Error",
    }
}

/// Check if the Accept header includes text/html.
#[inline]
pub fn accepts_html(accept_header: &str) -> bool {
    if accept_header.is_empty() {
        return false;
    }

    // Fast path for common cases
    if accept_header == "*/*" || accept_header.starts_with("text/html") {
        return true;
    }

    // Parse Accept header parts
    accept_header
        .split(',')
        .map(|part| part.split(';').next().unwrap_or("").trim())
        .any(|mime| mime == "text/html" || mime == "text/*" || mime == "*/*")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accepts_html() {
        assert!(accepts_html("text/html"));
        assert!(accepts_html("text/html, application/json"));
        assert!(accepts_html("application/json, text/html"));
        assert!(accepts_html("text/html; q=0.9"));
        assert!(accepts_html("*/*"));
        assert!(accepts_html("text/*"));

        assert!(!accepts_html(""));
        assert!(!accepts_html("application/json"));
        assert!(!accepts_html("text/plain"));
    }
}
