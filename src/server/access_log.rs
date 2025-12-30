//! Access logging.

// Note: Global state has been moved to config::MiddlewareConfig.access_log.
// The access_log_enabled flag is now passed via ConnectionContext.

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
    trace_id: Option<&str>,
    span_id: Option<&str>,
) {
    crate::logging::log_access(
        ts, request_id, ip, method, path, query, http, status, bytes, duration_ms, ua, referer,
        xff, tls, trace_id, span_id,
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
