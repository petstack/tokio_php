//! Access logging.

use std::sync::atomic::{AtomicBool, Ordering};

/// Global access log enabled flag.
static ACCESS_LOG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Initialize access logging.
pub fn init(enabled: bool) {
    ACCESS_LOG_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Check if access logging is enabled.
#[inline]
pub fn is_enabled() -> bool {
    ACCESS_LOG_ENABLED.load(Ordering::Relaxed)
}

/// Log an HTTP request using the unified log format.
#[allow(clippy::too_many_arguments)]
pub fn log_request(
    ts: &str,
    request_id: &str,
    ip: &str,
    method: &str,
    path: &str,
    query: Option<&str>,
    http: &str,
    status: u16,
    bytes: u64,
    duration_ms: f64,
    ua: Option<&str>,
    referer: Option<&str>,
    xff: Option<&str>,
    tls: Option<&str>,
) {
    crate::logging::log_access(
        ts, request_id, ip, method, path, query, http, status, bytes, duration_ms, ua, referer,
        xff, tls,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_log_enabled() {
        init(true);
        assert!(is_enabled());
        init(false);
        assert!(!is_enabled());
    }
}
